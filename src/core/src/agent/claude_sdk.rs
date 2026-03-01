//! Claude Code Simple SDK — pure Claude CLI bidirectional protocol wrapper.
//!
//! Spawns `claude --input-format stream-json --output-format stream-json` and provides:
//! - Process lifecycle management (spawn, shutdown)
//! - Bidirectional message I/O (send user messages, receive events)
//! - Control protocol handling (initialize, can_use_tool auto-allow, hook_callback)
//!
//! This module knows NOTHING about ACP. It only speaks the Claude CLI private protocol.
//! The ACP translation layer lives in `claude_acp.rs`.

use std::path::Path;
use std::process::Stdio;
use std::sync::Arc;

use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, Command};
use tokio::sync::{mpsc, Mutex};

// ---------------------------------------------------------------------------
// SDK event types — Claude CLI protocol → structured Rust events
// ---------------------------------------------------------------------------

/// A content block from a Claude `assistant` message.
#[derive(Debug, Clone)]
pub enum ContentBlock {
    Text { text: String },
    Thinking { text: String },
    ToolUse { id: String, name: String },
}

/// Events emitted by the Claude CLI, parsed from stdout NDJSON.
#[derive(Debug, Clone)]
pub enum SdkEvent {
    /// Claude produced an assistant message with content blocks.
    AssistantMessage { content: Vec<ContentBlock> },
    /// A turn completed with a `result` message.
    TurnResult {
        session_id: Option<String>,
        is_error: bool,
        error_text: Option<String>,
    },
    /// System init message received.
    SystemInit { session_id: Option<String> },
    /// Control request that was auto-handled (logged for debugging).
    ControlHandled { subtype: String },
}

// ---------------------------------------------------------------------------
// ClaudeSdk — the main handle
// ---------------------------------------------------------------------------

/// A running Claude CLI subprocess with bidirectional communication.
pub struct ClaudeSdk {
    /// Send raw JSON lines to claude's stdin.
    write_tx: mpsc::Sender<String>,
    /// Receive parsed SDK events from claude's stdout.
    event_rx: Mutex<mpsc::Receiver<SdkEvent>>,
    /// The subprocess handle.
    child: Mutex<Option<Child>>,
    /// Current session ID (updated on `result` messages).
    session_id: Arc<Mutex<String>>,
}

impl ClaudeSdk {
    /// Spawn the Claude CLI and return a ready-to-use SDK handle.
    ///
    /// The subprocess is initialized (control handshake sent) before returning.
    /// Events start flowing immediately into the internal channel.
    pub async fn spawn(cwd: &Path) -> Result<Self, String> {
        let mut child = Command::new("claude")
            .args([
                "--input-format", "stream-json",
                "--output-format", "stream-json",
                "--verbose",
                "--dangerously-skip-permissions",
            ])
            .current_dir(cwd)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .env("CLAUDE_CODE_ENTRYPOINT", "sdk-rs")
            .kill_on_drop(true)
            .spawn()
            .map_err(|e| format!("Failed to spawn claude: {}", e))?;

        let stdin = child.stdin.take().ok_or("No stdin")?;
        let stdout = child.stdout.take().ok_or("No stdout")?;

        // Writer task: drains write_tx → stdin
        let (write_tx, mut write_rx) = mpsc::channel::<String>(64);
        let stdin: Arc<Mutex<ChildStdin>> = Arc::new(Mutex::new(stdin));
        let stdin_w = stdin.clone();
        tokio::task::spawn_local(async move {
            while let Some(line) = write_rx.recv().await {
                let mut w = stdin_w.lock().await;
                if w.write_all(line.as_bytes()).await.is_err() { break; }
                if w.write_all(b"\n").await.is_err() { break; }
                let _ = w.flush().await;
            }
        });

        // Send initialize control request
        let init_msg = serde_json::json!({
            "type": "control_request",
            "request_id": "req_init_1",
            "request": { "subtype": "initialize", "hooks": null, "agents": null }
        });
        write_tx.send(init_msg.to_string()).await
            .map_err(|e| format!("Failed to send init: {}", e))?;

        // Reader task: parses stdout NDJSON → SdkEvent
        let (event_tx, event_rx) = mpsc::channel::<SdkEvent>(256);
        let session_id = Arc::new(Mutex::new("claude-0".to_string()));
        let session_id_for_reader = session_id.clone();
        let write_tx_for_reader = write_tx.clone();

        tokio::task::spawn_local(async move {
            let reader = BufReader::new(stdout);
            let mut lines = reader.lines();

            while let Ok(Some(line)) = lines.next_line().await {
                if line.trim().is_empty() { continue; }
                let msg: serde_json::Value = match serde_json::from_str(&line) {
                    Ok(v) => v,
                    Err(_) => continue,
                };

                let msg_type = msg.get("type").and_then(|v| v.as_str()).unwrap_or("");

                match msg_type {
                    "assistant" => {
                        let blocks = parse_content_blocks(&msg);
                        if !blocks.is_empty() {
                            let _ = event_tx.send(SdkEvent::AssistantMessage { content: blocks }).await;
                        }
                    }

                    "control_request" => {
                        handle_control_request(&msg, &write_tx_for_reader, &event_tx).await;
                    }

                    "result" => {
                        let new_sid = msg.get("session_id").and_then(|v| v.as_str()).map(|s| s.to_string());
                        if let Some(ref s) = new_sid {
                            *session_id_for_reader.lock().await = s.clone();
                        }
                        let is_error = msg.get("is_error").and_then(|v| v.as_bool()).unwrap_or(false);
                        let error_text = if is_error {
                            msg.get("result").and_then(|v| v.as_str()).map(|s| s.to_string())
                        } else {
                            None
                        };
                        let _ = event_tx.send(SdkEvent::TurnResult {
                            session_id: new_sid,
                            is_error,
                            error_text,
                        }).await;
                    }

                    "system" => {
                        let sid = msg.get("session_id").and_then(|v| v.as_str()).map(|s| s.to_string());
                        if let Some(ref s) = sid {
                            *session_id_for_reader.lock().await = s.clone();
                        }
                        let _ = event_tx.send(SdkEvent::SystemInit { session_id: sid }).await;
                    }

                    // control_response, user — internal, no event needed
                    _ => {}
                }
            }
            eprintln!("[claude-sdk] stdout reader finished");
        });

        // Brief wait for init handshake
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        eprintln!("[claude-sdk] subprocess started");

        Ok(Self {
            write_tx,
            event_rx: Mutex::new(event_rx),
            child: Mutex::new(Some(child)),
            session_id,
        })
    }

    /// Send a user message to the Claude CLI.
    pub async fn send_user_message(&self, text: &str) -> Result<(), String> {
        let session_id = self.session_id.lock().await.clone();
        let user_msg = serde_json::json!({
            "type": "user",
            "session_id": session_id,
            "message": { "role": "user", "content": text },
            "parent_tool_use_id": null
        });
        self.write_tx.send(user_msg.to_string()).await
            .map_err(|e| format!("Failed to send user message: {}", e))
    }

    /// Receive the next SDK event. Returns `None` if the reader task has ended.
    pub async fn recv_event(&self) -> Option<SdkEvent> {
        self.event_rx.lock().await.recv().await
    }

    /// Get the current session ID.
    pub async fn session_id(&self) -> String {
        self.session_id.lock().await.clone()
    }

    /// Shut down the Claude CLI subprocess.
    pub async fn shutdown(&self) {
        if let Some(mut child) = self.child.lock().await.take() {
            let _ = child.kill().await;
        }
        eprintln!("[claude-sdk] shutdown");
    }
}

// ---------------------------------------------------------------------------
// Internal helpers — Claude CLI protocol parsing
// ---------------------------------------------------------------------------

/// Parse content blocks from an `assistant` message.
fn parse_content_blocks(msg: &serde_json::Value) -> Vec<ContentBlock> {
    let mut blocks = Vec::new();
    if let Some(content) = msg.pointer("/message/content").and_then(|v| v.as_array()) {
        for block in content {
            let bt = block.get("type").and_then(|v| v.as_str()).unwrap_or("");
            match bt {
                "text" => {
                    if let Some(text) = block.get("text").and_then(|v| v.as_str()) {
                        blocks.push(ContentBlock::Text { text: text.to_string() });
                    }
                }
                "thinking" => {
                    if let Some(text) = block.get("thinking").and_then(|v| v.as_str()) {
                        blocks.push(ContentBlock::Thinking { text: text.to_string() });
                    }
                }
                "tool_use" => {
                    let name = block.get("name").and_then(|v| v.as_str()).unwrap_or("unknown").to_string();
                    let id = block.get("id").and_then(|v| v.as_str()).unwrap_or("tool_0").to_string();
                    blocks.push(ContentBlock::ToolUse { id, name });
                }
                _ => {}
            }
        }
    }
    blocks
}

/// Handle a control_request from the Claude CLI (auto-allow tools, ack hooks).
async fn handle_control_request(
    msg: &serde_json::Value,
    write_tx: &mpsc::Sender<String>,
    event_tx: &mpsc::Sender<SdkEvent>,
) {
    let request_id = msg.get("request_id").and_then(|v| v.as_str()).unwrap_or("");
    let subtype = msg.pointer("/request/subtype").and_then(|v| v.as_str()).unwrap_or("");

    let response = match subtype {
        "can_use_tool" => serde_json::json!({
            "type": "control_response",
            "response": {
                "subtype": "success",
                "request_id": request_id,
                "response": { "behavior": "allow" }
            }
        }),
        "hook_callback" => serde_json::json!({
            "type": "control_response",
            "response": {
                "subtype": "success",
                "request_id": request_id,
                "response": {}
            }
        }),
        _ => return,
    };

    let _ = write_tx.send(response.to_string()).await;
    let _ = event_tx.send(SdkEvent::ControlHandled { subtype: subtype.to_string() }).await;
}
