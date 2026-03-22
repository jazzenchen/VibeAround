//! ChannelManager: manages channel transports and protocol I/O.
//!
//! Responsibilities:
//! - Spawn external channel plugin processes (Node.js)
//! - Register internal channel transports
//! - Parse JSON-RPC messages from channel transports → InboundMessage
//! - Forward ChannelNotification → channel transport
//! - Route inbound messages to SessionHub

pub mod channels;

use std::path::PathBuf;
use std::sync::Arc;

use dashmap::DashMap;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::Command;
use tokio::sync::{broadcast, mpsc, oneshot, Mutex, OnceCell};
use tokio::task::AbortHandle;

use crate::config;
use crate::session_hub::types::*;
use crate::session_hub::SessionHub;

type StdinWriter = Arc<Mutex<tokio::process::ChildStdin>>;
type PendingRequests = Arc<DashMap<u64, oneshot::Sender<Result<serde_json::Value, String>>>>;

enum ChannelHandle {
    External {
        stdin: StdinWriter,
        abort: AbortHandle,
    },
    Internal {
        outbound_tx: mpsc::UnboundedSender<ChannelNotification>,
    },
}

pub struct ChannelManager {
    channels: DashMap<ChannelKind, ChannelHandle>,
    session_hub: OnceCell<Arc<SessionHub>>,
}

impl ChannelManager {
    pub fn new() -> Self {
        Self {
            channels: DashMap::new(),
            session_hub: OnceCell::new(),
        }
    }

    pub fn set_session_hub(self: &Arc<Self>, hub: Arc<SessionHub>) {
        let _ = self.session_hub.set(Arc::clone(&hub));
        self.spawn_channel_event_bridge(hub);
    }

    fn session_hub(&self) -> &Arc<SessionHub> {
        self.session_hub.get().expect("SessionHub not initialized")
    }

    fn spawn_channel_event_bridge(self: &Arc<Self>, session_hub: Arc<SessionHub>) {
        let this = Arc::clone(self);
        let mut rx = session_hub.subscribe_channel_events();
        tokio::spawn(async move {
            loop {
                match rx.recv().await {
                    Ok(event) => this.handle_channel_event(event).await,
                    Err(broadcast::error::RecvError::Lagged(n)) => {
                        eprintln!("[ChannelManager] channel event stream lagged by {} events", n);
                    }
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }
        });
    }

    async fn handle_channel_event(&self, event: ChannelEvent) {
        match event {
            ChannelEvent::OnAgentSessionReady {
                channel_kind,
                chat_id,
                message_id,
                cli_kind,
                cli_session_id,
                profile,
            } => {
                let text = format!(
                    "CLI session is now active.\n\nAgent: {}\nSession ID: {}\nProfile: {}",
                    cli_kind, cli_session_id, profile
                );
                self.send_notification(ChannelNotification::SendText {
                    channel_kind,
                    chat_id,
                    text,
                    reply_to: Some(message_id),
                })
                .await;
            }
            ChannelEvent::OnTurnStarted {
                channel_kind,
                chat_id,
                message_id,
            } => {
                self.send_notification(ChannelNotification::AgentStart {
                    channel_kind,
                    chat_id,
                    message_id,
                })
                .await;
            }
            ChannelEvent::OnTurnCompleted {
                channel_kind,
                chat_id,
            } => {
                self.send_notification(ChannelNotification::AgentEnd {
                    channel_kind,
                    chat_id,
                })
                .await;
            }
            ChannelEvent::OnSessionError {
                channel_kind,
                chat_id,
                error,
            } => {
                self.send_notification(ChannelNotification::AgentError {
                    channel_kind,
                    chat_id,
                    error,
                })
                .await;
            }
            ChannelEvent::OnSystemText {
                channel_kind,
                chat_id,
                text,
                reply_to,
            } => {
                self.send_notification(ChannelNotification::SendText {
                    channel_kind,
                    chat_id,
                    text,
                    reply_to,
                })
                .await;
            }
            ChannelEvent::OnAcpEvent {
                channel_kind,
                chat_id,
                message_id,
                payload,
            } => {
                if let Some(kind) = payload.get("kind").and_then(|v| v.as_str()) {
                    match kind {
                        "token" => {
                            if let Some(delta) = payload.get("delta").and_then(|v| v.as_str()) {
                                self.send_notification(ChannelNotification::AgentToken {
                                    channel_kind,
                                    chat_id,
                                    delta: delta.to_string(),
                                })
                                .await;
                            }
                        }
                        "thinking" => {
                            if let Some(text) = payload.get("text").and_then(|v| v.as_str()) {
                                self.send_notification(ChannelNotification::AgentThinking {
                                    channel_kind,
                                    chat_id,
                                    text: text.to_string(),
                                })
                                .await;
                            }
                        }
                        "tool_use" => {
                            self.send_notification(ChannelNotification::AgentToolUse {
                                channel_kind,
                                chat_id,
                                tool: payload
                                    .get("tool")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("")
                                    .to_string(),
                                input: payload
                                    .get("input")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("")
                                    .to_string(),
                            })
                            .await;
                        }
                        "tool_result" => {
                            self.send_notification(ChannelNotification::AgentToolResult {
                                channel_kind,
                                chat_id,
                                tool: payload
                                    .get("tool")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("")
                                    .to_string(),
                                output: payload
                                    .get("output")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("")
                                    .to_string(),
                            })
                            .await;
                        }
                        _ => {
                            let _ = message_id;
                        }
                    }
                }
            }
            ChannelEvent::OnSessionStarted { .. } | ChannelEvent::OnSessionClosed { .. } => {}
        }
    }

    pub async fn start_plugin(
        self: &Arc<Self>,
        plugin_dir: PathBuf,
        channel_name: &str,
    ) -> Option<AbortHandle> {
        let prefix = format!("[{}]", channel_name);
        let cfg = config::ensure_loaded();

        let raw_config = match cfg.channel_raw_config(channel_name) {
            Some(v) => v,
            None => {
                eprintln!("{} config=missing channels.{} — plugin disabled", prefix, channel_name);
                return None;
            }
        };

        let entry_point = plugin_dir.join("dist").join("main.js");
        if !entry_point.exists() {
            eprintln!(
                "{} plugin entry not found: {} — run `npm run build` in {}",
                prefix,
                entry_point.display(),
                plugin_dir.display()
            );
            return None;
        }

        eprintln!("{} spawning plugin process: node {}", prefix, entry_point.display());

        let mut child = match Command::new("node")
            .arg(entry_point.to_str().unwrap())
            .current_dir(&plugin_dir)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .kill_on_drop(true)
            .spawn()
        {
            Ok(c) => c,
            Err(e) => {
                eprintln!("{} failed to spawn plugin: {}", prefix, e);
                return None;
            }
        };

        let stdin = child.stdin.take().expect("stdin piped");
        let stdout = child.stdout.take().expect("stdout piped");
        let stderr = child.stderr.take().expect("stderr piped");

        let stdin_writer: StdinWriter = Arc::new(Mutex::new(stdin));
        let pending: PendingRequests = Arc::new(DashMap::new());

        let init_req = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "config": raw_config,
                "hostVersion": env!("CARGO_PKG_VERSION"),
            }
        });
        {
            let mut guard = stdin_writer.lock().await;
            let line = serde_json::to_string(&init_req).unwrap() + "\n";
            if let Err(e) = guard.write_all(line.as_bytes()).await {
                eprintln!("{} failed to write initialize: {}", prefix, e);
                return None;
            }
            let _ = guard.flush().await;
        }

        let prefix_stderr = prefix.clone();
        tokio::spawn(async move {
            let reader = BufReader::new(stderr);
            let mut lines = reader.lines();
            while let Ok(Some(line)) = lines.next_line().await {
                eprintln!("{} [plugin] {}", prefix_stderr, line);
            }
        });

        let channel_name_owned = channel_name.to_string();
        let prefix_stdout = prefix.clone();
        let hub = Arc::clone(self);
        let handle = tokio::spawn(async move {
            let _child = child;
            let reader = BufReader::new(stdout);
            let mut lines = reader.lines();

            while let Ok(Some(line)) = lines.next_line().await {
                let line = line.trim().to_string();
                if line.is_empty() {
                    continue;
                }

                let msg: serde_json::Value = match serde_json::from_str(&line) {
                    Ok(v) => v,
                    Err(e) => {
                        eprintln!(
                            "{} invalid JSON from plugin: {} — {}",
                            prefix_stdout,
                            e,
                            &line[..line.len().min(120)]
                        );
                        continue;
                    }
                };

                if let Some(id) = msg.get("id") {
                    if let Some(id_val) = id.as_u64() {
                        if let Some((_, tx)) = pending.remove(&id_val) {
                            if let Some(err) = msg.get("error") {
                                let err_msg = err
                                    .get("message")
                                    .and_then(|m| m.as_str())
                                    .unwrap_or("unknown error")
                                    .to_string();
                                let _ = tx.send(Err(err_msg));
                            } else {
                                let result = msg.get("result").cloned().unwrap_or(serde_json::Value::Null);
                                let _ = tx.send(Ok(result));
                            }
                        }
                    }
                    if id.as_u64() == Some(1) {
                        eprintln!("{} event=plugin_ready", prefix_stdout);
                    }
                    continue;
                }

                hub.handle_inbound_jsonrpc(&channel_name_owned, msg).await;
            }

            eprintln!("{} stdout reader exited", prefix_stdout);
        });

        let abort = handle.abort_handle();

        self.channels.insert(
            channel_name.to_string(),
            ChannelHandle::External {
                stdin: stdin_writer,
                abort: abort.clone(),
            },
        );

        eprintln!("{} registered external channel", prefix);
        Some(abort)
    }

    pub fn start_internal_plugin(
        &self,
        channel_name: &str,
        outbound_tx: mpsc::UnboundedSender<ChannelNotification>,
    ) {
        self.channels.insert(
            channel_name.to_string(),
            ChannelHandle::Internal { outbound_tx },
        );
        eprintln!("[{}] registered internal channel", channel_name);
    }

    pub async fn handle_inbound_jsonrpc(&self, channel_name: &str, msg: serde_json::Value) {
        let prefix = format!("[{}]", channel_name);
        let method = msg.get("method").and_then(|m| m.as_str()).unwrap_or("");
        let params = msg.get("params").cloned().unwrap_or(serde_json::Value::Null);

        match method {
            "on_message" => {
                if let Some(inbound) = parse_on_message(&params, channel_name) {
                    eprintln!("{} on_message text={}", prefix, truncate(&inbound.text, 80));
                    self.session_hub().channel_request_message(inbound).await;
                }
            }
            "on_callback" => {
                if let Some(inbound) = parse_on_callback(&params, channel_name) {
                    eprintln!("{} on_callback text={}", prefix, truncate(&inbound.text, 80));
                    self.session_hub().channel_request_message(inbound).await;
                }
            }
            "plugin_log" => {
                let level = params.get("level").and_then(|v| v.as_str()).unwrap_or("info");
                let message = params.get("message").and_then(|v| v.as_str()).unwrap_or("");
                eprintln!("{} [channel][{}] {}", prefix, level, message);
            }
            other => {
                eprintln!("{} unknown notification: {}", prefix, other);
            }
        }
    }

    pub async fn send_notification(&self, notif: ChannelNotification) {
        let channel_kind = channel_kind_of_notification(&notif).to_string();

        if let Some(handle) = self.channels.get(&channel_kind) {
            match handle.value() {
                ChannelHandle::External { stdin, abort } => {
                    let _keep_alive = abort;
                    let json = notif.to_jsonrpc();
                    let line = serde_json::to_string(&json).unwrap() + "\n";
                    let mut guard = stdin.lock().await;
                    if let Err(e) = guard.write_all(line.as_bytes()).await {
                        eprintln!("[{}] failed to write to channel stdin: {}", channel_kind, e);
                    }
                    let _ = guard.flush().await;
                }
                ChannelHandle::Internal { outbound_tx } => {
                    if let Err(e) = outbound_tx.send(notif) {
                        eprintln!("[{}] failed to send to internal channel: {}", channel_kind, e);
                    }
                }
            }
        } else {
            eprintln!("[ChannelManager] no channel for kind '{}'", channel_kind);
        }
    }
}

fn channel_kind_of_notification(notif: &ChannelNotification) -> &str {
    match notif {
        ChannelNotification::AgentStart { channel_kind, .. } => channel_kind,
        ChannelNotification::AgentThinking { channel_kind, .. } => channel_kind,
        ChannelNotification::AgentToken { channel_kind, .. } => channel_kind,
        ChannelNotification::AgentToolUse { channel_kind, .. } => channel_kind,
        ChannelNotification::AgentToolResult { channel_kind, .. } => channel_kind,
        ChannelNotification::AgentEnd { channel_kind, .. } => channel_kind,
        ChannelNotification::AgentError { channel_kind, .. } => channel_kind,
        ChannelNotification::SendText { channel_kind, .. } => channel_kind,
    }
}

fn parse_on_message(params: &serde_json::Value, channel_name: &str) -> Option<InboundMessage> {
    let raw_channel_id = params.get("channelId")?.as_str()?.to_string();
    let text = params.get("text").and_then(|v| v.as_str()).unwrap_or("").to_string();
    let message_id = params.get("messageId").and_then(|v| v.as_str()).unwrap_or("").to_string();
    let sender_id = params
        .get("sender")
        .and_then(|s| s.get("id"))
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let reply_to = params.get("replyTo").and_then(|v| v.as_str()).map(|s| s.to_string());

    if text.is_empty() {
        return None;
    }

    let chat_id = raw_channel_id
        .strip_prefix(&format!("{}:", channel_name))
        .unwrap_or(&raw_channel_id)
        .to_string();

    Some(InboundMessage {
        channel_kind: channel_name.to_string(),
        chat_id,
        message_id,
        text,
        sender_id,
        attachments: vec![],
        parent_id: reply_to,
    })
}

fn parse_on_callback(params: &serde_json::Value, channel_name: &str) -> Option<InboundMessage> {
    let raw_channel_id = params.get("channelId")?.as_str()?.to_string();
    let sender_id = params
        .get("sender")
        .and_then(|s| s.get("id"))
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let action_value = params
        .get("data")
        .and_then(|d| d.get("value"))
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let action_text = format!("[button:{}]", action_value);

    let chat_id = raw_channel_id
        .strip_prefix(&format!("{}:", channel_name))
        .unwrap_or(&raw_channel_id)
        .to_string();

    Some(InboundMessage {
        channel_kind: channel_name.to_string(),
        chat_id,
        message_id: String::new(),
        text: action_text,
        sender_id,
        attachments: vec![],
        parent_id: None,
    })
}

fn truncate(s: &str, max: usize) -> &str {
    if s.len() <= max { s } else { &s[..max] }
}
