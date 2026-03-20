//! Plugin channel: spawn an external process (e.g. Node.js), communicate via stdio JSON-RPC.
//!
//! Replaces the webhook-based Feishu channel with a plugin that handles WebSocket + Lark SDK
//! internally, forwarding events to the Rust host via stdout notifications.

pub mod transport;

use std::path::PathBuf;
use std::process::Stdio;
use std::sync::Arc;

use dashmap::DashMap;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::Command;
use tokio::sync::{mpsc, Mutex};

use crate::config;
use crate::im::daemon::OutboundHub;
use crate::im::log::prefix_channel;
use crate::im::worker::InboundMessage;
use crate::service::ServiceManager;
use transport::{PendingRequests, PluginTransport, StdinWriter};

/// Start a plugin-based IM bot.
///
/// 1. Reads config from settings.json (raw JSON for the channel)
/// 2. Spawns the plugin process (`node dist/main.js`)
/// 3. Sends `initialize` with the raw config
/// 4. Starts stdout reader → inbound_tx → run_worker
/// 5. Returns an abort handle for cleanup
pub async fn run_plugin_bot(
    plugin_dir: PathBuf,
    channel_name: &str,
    services: Arc<ServiceManager>,
) -> Option<tokio::task::AbortHandle> {
    let cfg = config::ensure_loaded();
    let prefix = prefix_channel(channel_name);

    // Get raw channel config from settings.json
    let raw_config = match cfg.channel_raw_config(channel_name) {
        Some(v) => v,
        None => {
            eprintln!("{} config=missing channels.{} — plugin disabled", prefix, channel_name);
            return None;
        }
    };

    // Verify plugin directory exists
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

    // Spawn plugin process
    let mut child = match Command::new("node")
        .arg(&entry_point)
        .current_dir(&plugin_dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
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
        let line = format!("{}\n", init_req);
        let mut writer = stdin_writer.lock().await;
        if let Err(e) = writer.write_all(line.as_bytes()).await {
            eprintln!("{} failed to send initialize: {}", prefix, e);
            return None;
        }
        let _ = writer.flush().await;
    }

    // Setup inbound channel and worker
    let (inbound_tx, inbound_rx) = mpsc::channel::<InboundMessage>(64);
    let busy_set: Arc<DashMap<String, ()>> = Arc::new(DashMap::new());
    let transport = Arc::new(PluginTransport::new(stdin_writer.clone(), pending.clone()));
    let outbound = OutboundHub::new(transport.clone());

    // Spawn worker
    tokio::spawn(crate::im::worker::run_worker(
        inbound_rx,
        outbound.clone(),
        busy_set.clone(),
        cfg.channel_verbose(channel_name),
        services,
    ));

    // Spawn stderr reader (forward plugin logs to host stderr)
    let prefix_stderr = prefix.clone();
    tokio::spawn(async move {
        let reader = BufReader::new(stderr);
        let mut lines = reader.lines();
        while let Ok(Some(line)) = lines.next_line().await {
            eprintln!("{} [plugin] {}", prefix_stderr, line);
        }
    });

    // Spawn stdout reader (JSON-RPC responses + notifications)
    // IMPORTANT: move `child` into this task to keep the process alive (kill_on_drop).
    let prefix_stdout = prefix.clone();
    let handle = tokio::spawn(async move {
        let _child = child; // prevent drop → keeps process alive
        let reader = BufReader::new(stdout);
        let mut lines = reader.lines();

        while let Ok(Some(line)) = lines.next_line().await {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }

            let msg: serde_json::Value = match serde_json::from_str(trimmed) {
                Ok(v) => v,
                Err(e) => {
                    eprintln!("{} [plugin] invalid JSON from stdout: {} — {}", prefix_stdout, e, trimmed);
                    continue;
                }
            };

            // JSON-RPC response (has "id" + "result" or "error")
            if let Some(id) = msg.get("id").and_then(|v| v.as_u64()) {
                if let Some((_, tx)) = pending.remove(&id) {
                    if let Some(err) = msg.get("error") {
                        let err_msg = err
                            .get("message")
                            .and_then(|m| m.as_str())
                            .unwrap_or("unknown error");
                        let _ = tx.send(Err(err_msg.to_string()));
                    } else {
                        let result = msg.get("result").cloned().unwrap_or(serde_json::Value::Null);
                        let _ = tx.send(Ok(result));
                    }
                }
                continue;
            }

            // JSON-RPC notification (has "method" but no "id")
            let method = msg.get("method").and_then(|m| m.as_str()).unwrap_or("");
            let params = msg.get("params").cloned().unwrap_or(serde_json::Value::Null);

            match method {
                "on_message" => {
                    if let Some(inbound) = parse_on_message(&params) {
                        if let Err(e) = inbound_tx.send(inbound).await {
                            eprintln!("{} inbound channel closed: {}", prefix_stdout, e);
                            break;
                        }
                    }
                }
                "on_reaction" => {
                    // Log for now; reaction handling can be added later
                    eprintln!("{} [plugin] reaction: {}", prefix_stdout, params);
                }
                "on_callback" => {
                    // Card callback → treat as inbound message with the action value as text
                    if let Some(inbound) = parse_on_callback(&params) {
                        let _ = inbound_tx.send(inbound).await;
                    }
                }
                _ => {
                    eprintln!("{} [plugin] unknown notification: {}", prefix_stdout, method);
                }
            }
        }

        eprintln!("{} [plugin] stdout reader exited", prefix_stdout);
    });

    eprintln!("{} event=plugin_ready dir={}", prefix, plugin_dir.display());
    Some(handle.abort_handle())
}

/// Parse an `on_message` notification into an InboundMessage.
fn parse_on_message(params: &serde_json::Value) -> Option<InboundMessage> {
    let channel_id = params.get("channelId")?.as_str()?.to_string();
    let message_id = params.get("messageId")?.as_str()?.to_string();
    let text = params.get("text")?.as_str()?.to_string();

    let _sender_id = params
        .get("sender")
        .and_then(|s| s.get("id"))
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    let reply_to = params
        .get("replyTo")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    let chat_type = params
        .get("chatType")
        .and_then(|v| v.as_str())
        .unwrap_or("p2p")
        .to_string();

    let mentioned_bot = params
        .get("mentionedBot")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    // In group chats, only respond if bot was @mentioned
    if chat_type == "group" && !mentioned_bot {
        return None;
    }

    Some(InboundMessage {
        channel_id,
        text,
        attachments: vec![],
        parent_id: reply_to,
        user_message_id: Some(message_id),
    })
}

/// Parse an `on_callback` notification (card button click) into an InboundMessage.
fn parse_on_callback(params: &serde_json::Value) -> Option<InboundMessage> {
    let channel_id = params.get("channelId")?.as_str()?.to_string();
    let _sender_id = params
        .get("sender")
        .and_then(|s| s.get("id"))
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    // Extract the action value — typically {"action": "/some_command"}
    let action_text = params
        .get("data")
        .and_then(|d| d.get("action"))
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    if action_text.is_empty() {
        return None;
    }

    Some(InboundMessage {
        channel_id,
        text: action_text,
        attachments: vec![],
        parent_id: None,
        user_message_id: None,
    })
}
