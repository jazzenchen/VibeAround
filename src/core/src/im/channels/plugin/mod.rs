//! Plugin channel: spawn an external process (e.g. Node.js), communicate via stdio JSON-RPC.
//!
//! Uses MessageHub for agent routing — no worker/daemon needed.

pub mod transport;

use std::path::PathBuf;
use std::process::Stdio;
use std::sync::Arc;

use dashmap::DashMap;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::Command;
use tokio::sync::{mpsc, Mutex};
use tokio::task::AbortHandle;

use crate::config;
use crate::im::message_hub::{InboundMessage, MessageHub, PluginNotification};
use crate::service::ServiceManager;
use transport::{PendingRequests, StdinWriter};

/// Start a plugin-based IM bot.
///
/// 1. Reads config from settings.json (raw JSON for the channel)
/// 2. Spawns the plugin process (node dist/main.js)
/// 3. Sends initialize request with config
/// 4. Spawns MessageHub for agent routing
/// 5. Reads stdout for JSON-RPC messages (on_message → Hub → Agent → plugin notifications)
/// 6. Reads stderr for plugin debug logs
///
/// Returns an AbortHandle to stop the plugin.
pub async fn run_plugin_bot(
    plugin_dir: PathBuf,
    channel_name: &str,
    services: Arc<ServiceManager>,
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

    let mut child = match Command::new("node")
        .arg(entry_point.to_str().unwrap())
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
        let mut guard = stdin_writer.lock().await;
        let line = serde_json::to_string(&init_req).unwrap() + "\n";
        if let Err(e) = guard.write_all(line.as_bytes()).await {
            eprintln!("{} failed to write initialize: {}", prefix, e);
            return None;
        }
        let _ = guard.flush().await;
    }

    // Spawn MessageHub
    let verbose = cfg.channel_verbose(channel_name);
    let (inbound_tx, outbound_rx) = MessageHub::spawn(Arc::clone(&services), verbose);

    // Spawn outbound forwarder: MessageHub notifications → plugin stdin
    let stdin_for_outbound = Arc::clone(&stdin_writer);
    let prefix_outbound = prefix.clone();
    tokio::spawn(async move {
        forward_outbound(outbound_rx, stdin_for_outbound, &prefix_outbound).await;
    });

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
    let prefix_stdout = prefix.clone();
    let channel_name_owned = channel_name.to_string();
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
                    eprintln!("{} invalid JSON from plugin: {} — {}", prefix_stdout, e, &line[..line.len().min(120)]);
                    continue;
                }
            };

            // JSON-RPC response (has "id" field) — resolve pending request
            if let Some(id) = msg.get("id") {
                if let Some(id_val) = id.as_u64() {
                    if let Some((_, tx)) = pending.remove(&id_val) {
                        // Check for JSON-RPC error
                        if let Some(err) = msg.get("error") {
                            let err_msg = err.get("message")
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
                // Log initialize response
                if id.as_u64() == Some(1) {
                    eprintln!("{} event=plugin_ready", prefix_stdout);
                }
                continue;
            }

            // JSON-RPC notification (no "id") — on_message, on_callback, etc.
            let method = msg.get("method").and_then(|m| m.as_str()).unwrap_or("");
            let params = msg.get("params").cloned().unwrap_or(serde_json::Value::Null);

            match method {
                "on_message" => {
                    if let Some(inbound) = parse_on_message(&params, &channel_name_owned) {
                        eprintln!("{} on_message text={}", prefix_stdout, truncate(&inbound.text, 80));
                        let _ = inbound_tx.send(inbound).await;
                    }
                }
                "on_callback" => {
                    if let Some(inbound) = parse_on_callback(&params, &channel_name_owned) {
                        eprintln!("{} on_callback text={}", prefix_stdout, truncate(&inbound.text, 80));
                        let _ = inbound_tx.send(inbound).await;
                    }
                }
                other => {
                    eprintln!("{} unknown notification: {}", prefix_stdout, other);
                }
            }
        }

        eprintln!("{} stdout reader exited", prefix_stdout);
    });

    eprintln!("{} registered im_bot", prefix);
    Some(handle.abort_handle())
}

/// Forward MessageHub notifications to plugin stdin as JSON-RPC.
async fn forward_outbound(
    mut rx: mpsc::UnboundedReceiver<PluginNotification>,
    stdin: StdinWriter,
    prefix: &str,
) {
    while let Some(notif) = rx.recv().await {
        let json = notif.to_jsonrpc();
        let line = serde_json::to_string(&json).unwrap() + "\n";
        let mut guard = stdin.lock().await;
        if let Err(e) = guard.write_all(line.as_bytes()).await {
            eprintln!("{} failed to write to plugin stdin: {}", prefix, e);
            break;
        }
        let _ = guard.flush().await;
    }
    eprintln!("{} outbound forwarder stopped", prefix);
}

/// Parse on_message notification params into InboundMessage.
fn parse_on_message(params: &serde_json::Value, channel_name: &str) -> Option<InboundMessage> {
    let channel_id = format!(
        "{}:{}",
        channel_name,
        params.get("channelId").and_then(|v| v.as_str()).unwrap_or("")
    );
    let text = params.get("text").and_then(|v| v.as_str()).unwrap_or("").to_string();
    let message_id = params.get("messageId").and_then(|v| v.as_str()).unwrap_or("").to_string();
    let sender_id = params.get("sender").and_then(|s| s.get("id")).and_then(|v| v.as_str()).unwrap_or("").to_string();
    let reply_to = params.get("replyTo").and_then(|v| v.as_str()).map(|s| s.to_string());

    if text.is_empty() {
        return None;
    }

    Some(InboundMessage {
        channel_id,
        text,
        attachments: vec![],
        parent_id: reply_to,
        user_message_id: Some(message_id),
        sender_id,
    })
}

/// Parse on_callback notification params into InboundMessage.
fn parse_on_callback(params: &serde_json::Value, channel_name: &str) -> Option<InboundMessage> {
    let channel_id = format!(
        "{}:{}",
        channel_name,
        params.get("channelId").and_then(|v| v.as_str()).unwrap_or("")
    );
    let _sender_id = params.get("sender").and_then(|s| s.get("id")).and_then(|v| v.as_str()).unwrap_or("").to_string();
    let action_value = params.get("data").and_then(|d| d.get("value")).and_then(|v| v.as_str()).unwrap_or("");
    let action_text = format!("[button:{}]", action_value);

    Some(InboundMessage {
        channel_id,
        text: action_text,
        attachments: vec![],
        parent_id: None,
        user_message_id: None,
        sender_id: _sender_id,
    })
}

fn truncate(s: &str, max: usize) -> &str {
    if s.len() <= max { s } else { &s[..max] }
}
