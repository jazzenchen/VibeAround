//! WebSocket handler for web chat channel.
//!
//! - GET /ws/chat — websocket adapter for the internal `web` channel

use axum::extract::{
    ws::{Message, WebSocket, WebSocketUpgrade},
    State,
};
use axum::response::Response;
use futures_util::{SinkExt, StreamExt};
use uuid::Uuid;

use common::config;
use common::hub::types::ChannelNotification;

use super::AppState;

/// WebSocket upgrade handler for web chat.
pub async fn ws_chat_handler(State(state): State<AppState>, ws: WebSocketUpgrade) -> Response {
    ws.on_upgrade(move |socket| handle_chat_socket(socket, state))
}

async fn handle_chat_socket(socket: WebSocket, state: AppState) {
    let chat_id = Uuid::new_v4().to_string();
    let channel_id = format!("web:{}", chat_id);
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<ChannelNotification>();
    state.web_channel.register_connection(chat_id.clone(), tx);

    let (mut ws_tx, mut ws_rx) = socket.split();

    // Push config on connect so current UI can still render target info.
    let cfg = config::ensure_loaded();
    let agents: Vec<serde_json::Value> = cfg
        .enabled_agents
        .iter()
        .map(|kind| {
            serde_json::json!({
                "id": kind.to_string(),
                "description": kind.description(),
            })
        })
        .collect();
    let config_msg = serde_json::json!({
        "type": "config",
        "channelId": channel_id,
        "agents": agents,
        "default_agent": cfg.default_agent,
    });
    let _ = ws_tx.send(Message::Text(config_msg.to_string().into())).await;

    let outbound_task = tokio::spawn(async move {
        while let Some(notif) = rx.recv().await {
            let msg = notification_to_client_json(notif);
            if ws_tx.send(Message::Text(msg.to_string().into())).await.is_err() {
                break;
            }
        }
    });

    while let Some(Ok(msg)) = ws_rx.next().await {
        match msg {
            Message::Text(text) => {
                if let Some(value) = client_message_to_channel_json(&chat_id, &text) {
                    state.channel_hub.handle_inbound_jsonrpc("web", value).await;
                }
            }
            Message::Close(_) => break,
            _ => {}
        }
    }

    outbound_task.abort();
    state.web_channel.unregister_connection(&chat_id);
}

fn client_message_to_channel_json(chat_id: &str, text: &str) -> Option<serde_json::Value> {
    let parsed = serde_json::from_str::<serde_json::Value>(text);

    match parsed {
        Ok(v) => {
            let ty = v.get("type").and_then(|x| x.as_str()).unwrap_or("");
            match ty {
                "message" => {
                    let text = v.get("text").and_then(|x| x.as_str()).unwrap_or("").trim();
                    if text.is_empty() {
                        return None;
                    }
                    let message_id = v
                        .get("messageId")
                        .and_then(|x| x.as_str())
                        .map(ToOwned::to_owned)
                        .unwrap_or_else(|| Uuid::new_v4().to_string());
                    Some(serde_json::json!({
                        "jsonrpc": "2.0",
                        "method": "on_message",
                        "params": {
                            "channelId": format!("web:{}", chat_id),
                            "messageId": message_id,
                            "text": text,
                            "sender": { "id": "web-user" }
                        }
                    }))
                }
                _ => None,
            }
        }
        Err(_) => {
            let trimmed = text.trim();
            if trimmed.is_empty() {
                None
            } else {
                Some(serde_json::json!({
                    "jsonrpc": "2.0",
                    "method": "on_message",
                    "params": {
                        "channelId": format!("web:{}", chat_id),
                        "messageId": Uuid::new_v4().to_string(),
                        "text": trimmed,
                        "sender": { "id": "web-user" }
                    }
                }))
            }
        }
    }
}

fn notification_to_client_json(notif: ChannelNotification) -> serde_json::Value {
    match notif {
        ChannelNotification::AgentStart { .. } => serde_json::json!({ "type": "start" }),
        ChannelNotification::AgentThinking { text, .. } => serde_json::json!({ "progress": text }),
        ChannelNotification::AgentToken { delta, .. } => serde_json::json!({ "text": delta }),
        ChannelNotification::AgentToolUse { tool, .. } => {
            serde_json::json!({ "progress": format!("Using tool: {}...", tool) })
        }
        ChannelNotification::AgentToolResult { .. } => serde_json::json!({}),
        ChannelNotification::AgentEnd { .. } => serde_json::json!({ "done": true }),
        ChannelNotification::AgentError { error, .. } => serde_json::json!({ "error": error }),
        ChannelNotification::SendText { text, .. } => {
            serde_json::json!({ "type": "system_text", "text": text, "done": true })
        }
    }
}
