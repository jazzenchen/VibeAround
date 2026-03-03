//! OpenCode JSONL event parser.
//! Parses JSON events from `opencode run --format json` stdout into AgentEvents.
//!
//! Known event types from OpenCode:
//!   {"type":"step_start", ...}
//!   {"type":"text", "part":{"text":"..."}}
//!   {"type":"tool_start", "part":{"tool":"...", "input":{...}}}
//!   {"type":"tool_finish", "part":{"output":"..."}}
//!   {"type":"step_finish", "part":{"reason":"stop", "cost":0.0, "tokens":{...}}}

use tokio::sync::broadcast;
use super::AgentEvent;

pub fn parse_event(msg: &serde_json::Value, event_tx: &broadcast::Sender<AgentEvent>) {
    let msg_type = msg.get("type").and_then(|v| v.as_str()).unwrap_or("");
    let part = msg.get("part");

    match msg_type {
        "text" => {
            if let Some(text) = part.and_then(|p| p.get("text")).and_then(|v| v.as_str()) {
                let _ = event_tx.send(AgentEvent::Text(text.to_string()));
            }
        }
        "thinking" | "reasoning" => {
            if let Some(text) = part.and_then(|p| p.get("text")).and_then(|v| v.as_str()) {
                let _ = event_tx.send(AgentEvent::Thinking(text.to_string()));
            }
        }
        "tool_start" => {
            let name = part.and_then(|p| p.get("tool")).and_then(|v| v.as_str()).unwrap_or("unknown").to_string();
            let id = part.and_then(|p| p.get("id")).and_then(|v| v.as_str()).unwrap_or("tool_0").to_string();
            let input = part.and_then(|p| p.get("input")).map(|v| v.to_string());
            let _ = event_tx.send(AgentEvent::ToolUse { name, id, input });
        }
        "tool_finish" => {
            let id = part.and_then(|p| p.get("id")).and_then(|v| v.as_str()).unwrap_or("tool_0").to_string();
            let output = part.and_then(|p| p.get("output")).and_then(|v| v.as_str()).map(String::from);
            let is_error = part.and_then(|p| p.get("error")).and_then(|v| v.as_bool()).unwrap_or(false);
            let _ = event_tx.send(AgentEvent::ToolResult { id, output, is_error });
        }
        "step_finish" => {
            let cost = part.and_then(|p| p.get("cost")).and_then(|v| v.as_f64());
            let _ = event_tx.send(AgentEvent::TurnComplete { session_id: None, cost_usd: cost });
        }
        "error" => {
            let text = part.and_then(|p| p.get("message").or(p.get("text")))
                .and_then(|v| v.as_str()).unwrap_or("Unknown error");
            let _ = event_tx.send(AgentEvent::Error(text.to_string()));
        }
        _ => {}
    }
}
