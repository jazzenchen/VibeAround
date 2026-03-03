//! Codex CLI JSONL event parser.
//! Parses JSON events from `codex exec --json` stdout into AgentEvents.
//! Reference: https://github.com/openai/codex

use tokio::sync::broadcast;
use super::AgentEvent;

/// Parse a single JSONL line from Codex CLI and emit AgentEvents.
/// Codex `--json` emits events like:
///   {"type": "message", "role": "assistant", "content": "..."}
///   {"type": "function_call", "name": "...", "arguments": "..."}
///   {"type": "function_call_output", "output": "..."}
///   {"type": "error", "message": "..."}
pub fn parse_event(msg: &serde_json::Value, event_tx: &broadcast::Sender<AgentEvent>) {
    let msg_type = msg.get("type").and_then(|v| v.as_str()).unwrap_or("");

    match msg_type {
        "message" => {
            let role = msg.get("role").and_then(|v| v.as_str()).unwrap_or("");
            if role == "assistant" || role == "" {
                if let Some(text) = msg.get("content").and_then(|v| v.as_str()) {
                    let _ = event_tx.send(AgentEvent::Text(text.to_string()));
                }
            }
        }
        "function_call" | "tool_call" => {
            let name = msg.get("name").and_then(|v| v.as_str()).unwrap_or("unknown").to_string();
            let id = msg.get("call_id").or(msg.get("id")).and_then(|v| v.as_str()).unwrap_or("tool_0").to_string();
            let input = msg.get("arguments").or(msg.get("input")).map(|v| {
                if let Some(s) = v.as_str() { s.to_string() } else { v.to_string() }
            });
            let _ = event_tx.send(AgentEvent::ToolUse { name, id, input });
        }
        "function_call_output" | "tool_output" => {
            let id = msg.get("call_id").or(msg.get("id")).and_then(|v| v.as_str()).unwrap_or("tool_0").to_string();
            let output = msg.get("output").and_then(|v| v.as_str()).map(String::from);
            let is_error = msg.get("is_error").and_then(|v| v.as_bool()).unwrap_or(false);
            let _ = event_tx.send(AgentEvent::ToolResult { id, output, is_error });
        }
        "error" => {
            let text = msg.get("message").and_then(|v| v.as_str()).unwrap_or("Unknown error");
            let _ = event_tx.send(AgentEvent::Error(text.to_string()));
        }
        _ => {
            // Try to extract text content from unknown events
            if let Some(text) = msg.get("content").and_then(|v| v.as_str()) {
                if !text.is_empty() {
                    let _ = event_tx.send(AgentEvent::Text(text.to_string()));
                }
            }
        }
    }
}
