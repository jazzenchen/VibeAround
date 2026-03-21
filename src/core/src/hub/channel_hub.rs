//! ChannelHub: manages channel transports and protocol I/O.
//!
//! Responsibilities:
//! - Spawn external channel plugin processes (Node.js)
//! - Register internal channel transports
//! - Parse JSON-RPC messages from channel transports → InboundMessage
//! - Forward ChannelNotification → channel transport
//! - Route inbound messages to SessionHub

use std::path::PathBuf;
use std::sync::Arc;

use dashmap::DashMap;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::Command;
use tokio::sync::{mpsc, Mutex, OnceCell, oneshot};
use tokio::task::AbortHandle;

use crate::config;
use crate::hub::session_hub::SessionHub;
use crate::hub::types::*;

/// Shared writer to an external plugin's stdin.
type StdinWriter = Arc<Mutex<tokio::process::ChildStdin>>;

/// Pending JSON-RPC request map: id → oneshot sender for the response.
type PendingRequests = Arc<DashMap<u64, oneshot::Sender<Result<serde_json::Value, String>>>>;

/// Per-channel transport handle.
enum ChannelHandle {
    External {
        stdin: StdinWriter,
        abort: AbortHandle,
    },
    Internal {
        outbound_tx: mpsc::UnboundedSender<ChannelNotification>,
    },
}

// ---------------------------------------------------------------------------
// ChannelHub
// ---------------------------------------------------------------------------

pub struct ChannelHub {
    /// Channel handles keyed by channel_kind (e.g. "feishu", "web").
    channels: DashMap<ChannelKind, ChannelHandle>,
    /// Back-reference to SessionHub (set after init via set_session_hub).
    session_hub: OnceCell<Arc<SessionHub>>,
}

impl ChannelHub {
    pub fn new() -> Self {
        Self {
            channels: DashMap::new(),
            session_hub: OnceCell::new(),
        }
    }

    /// Set the SessionHub reference (two-phase init).
    pub fn set_session_hub(&self, hub: Arc<SessionHub>) {
        let _ = self.session_hub.set(hub);
    }

    fn session_hub(&self) -> &Arc<SessionHub> {
        self.session_hub.get().expect("SessionHub not initialized")
    }

    // -----------------------------------------------------------------------
    // Channel lifecycle
    // -----------------------------------------------------------------------

    /// Start an external channel plugin. Called by ServerDaemon for configured channels.
    pub async fn start_plugin(
        self: &Arc<Self>,
        plugin_dir: PathBuf,
        channel_name: &str,
    ) -> Option<AbortHandle> {
        let prefix = format!("[{}]", channel_name);
        let cfg = config::ensure_loaded();

        // Get raw channel config from settings.json
        let raw_config = match cfg.channel_raw_config(channel_name) {
            Some(v) => v,
            None => {
                eprintln!("{} config=missing channels.{} — plugin disabled", prefix, channel_name);
                return None;
            }
        };

        // Verify plugin entry point exists
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

        // Send initialize request
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

        // Spawn stderr reader (forward plugin logs to host stderr)
        let prefix_stderr = prefix.clone();
        tokio::spawn(async move {
            let reader = BufReader::new(stderr);
            let mut lines = reader.lines();
            while let Ok(Some(line)) = lines.next_line().await {
                eprintln!("{} [plugin] {}", prefix_stderr, line);
            }
        });

        // Spawn stdout reader (JSON-RPC responses + notifications from plugin)
        let channel_name_owned = channel_name.to_string();
        let prefix_stdout = prefix.clone();
        let hub = Arc::clone(self);
        let handle = tokio::spawn(async move {
            let _child = child; // prevent drop → keeps process alive (kill_on_drop)
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

                // JSON-RPC response (has "id" field) — resolve pending request
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

        // Store external channel handle
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

    /// Register an internal channel transport.
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

    // -----------------------------------------------------------------------
    // Inbound: receive structured messages from channel transports
    // -----------------------------------------------------------------------

    /// Handle a single inbound JSON-RPC value from a channel transport.
    pub async fn handle_inbound_jsonrpc(&self, channel_name: &str, msg: serde_json::Value) {
        let prefix = format!("[{}]", channel_name);

        // JSON-RPC notification (no "id")
        let method = msg.get("method").and_then(|m| m.as_str()).unwrap_or("");
        let params = msg.get("params").cloned().unwrap_or(serde_json::Value::Null);

        match method {
            "on_message" => {
                if let Some(inbound) = parse_on_message(&params, channel_name) {
                    eprintln!("{} on_message text={}", prefix, truncate(&inbound.text, 80));
                    self.session_hub().receive(inbound).await;
                }
            }
            "on_callback" => {
                if let Some(inbound) = parse_on_callback(&params, channel_name) {
                    eprintln!("{} on_callback text={}", prefix, truncate(&inbound.text, 80));
                    self.session_hub().receive(inbound).await;
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

    // -----------------------------------------------------------------------
    // Outbound: send notifications to channel transports
    // -----------------------------------------------------------------------

    /// Send a ChannelNotification to the appropriate channel transport.
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
            eprintln!("[ChannelHub] no channel for kind '{}'", channel_kind);
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

// ---------------------------------------------------------------------------
// Parsers (from channel JSON-RPC params → InboundMessage)
// ---------------------------------------------------------------------------

/// Parse on_message notification params into InboundMessage.
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

    // raw_channel_id from plugin is "{channel_kind}:{chat_id}" — extract chat_id
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

/// Parse on_callback notification params into InboundMessage.
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
