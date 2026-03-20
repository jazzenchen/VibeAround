//! WebSocket handler for agent chat sessions.
//!
//! - GET /ws/chat — interactive chat with AI agents (Claude, Gemini, etc.)

use axum::extract::{
    ws::{Message, WebSocket, WebSocketUpgrade},
    State,
};
use axum::response::Response;
use futures_util::stream::StreamExt;
use futures_util::SinkExt;
use std::path::PathBuf;

use common::config;
use common::headless::wire;

use super::AppState;

/// WebSocket upgrade handler for agent chat.
pub async fn ws_chat_handler(State(state): State<AppState>, ws: WebSocketUpgrade) -> Response {
    let working_dir = state.working_dir.clone();
    ws.on_upgrade(move |socket| handle_chat_socket(socket, working_dir))
}

/// Full chat session lifecycle: agent selection, message routing, event streaming.
async fn handle_chat_socket(socket: WebSocket, working_dir: PathBuf) {
    use common::agent::{self, AgentBackend, AgentEvent, AgentKind};

    let (mut ws_tx, mut ws_rx) = socket.split();

    // Push enabled agents config to client on connect
    let default_agent_str = {
        let cfg = config::ensure_loaded();
        let agents: Vec<serde_json::Value> = cfg.enabled_agents.iter().map(|kind| {
            serde_json::json!({
                "id": kind.to_string(),
                "description": kind.description(),
            })
        }).collect();
        let config_msg = serde_json::json!({
            "type": "config",
            "agents": agents,
            "default_agent": cfg.default_agent,
        });
        let _ = ws_tx.send(Message::Text(config_msg.to_string().into())).await;
        cfg.default_agent.clone()
    };

    let mut active_agent: Option<Box<dyn AgentBackend>> = None;

    /// Start or switch agent backend. Returns true on success.
    async fn start_agent(
        active_agent: &mut Option<Box<dyn AgentBackend>>,
        kind: AgentKind,
        cwd: &std::path::Path,
    ) -> Result<(), String> {
        // Shut down existing
        if let Some(mut old) = active_agent.take() {
            old.shutdown().await;
        }
        let mut backend = agent::create_backend(kind);
        backend.start(cwd, None).await?;
        *active_agent = Some(backend);
        Ok(())
    }

    while let Some(Ok(msg)) = ws_rx.next().await {
        let Message::Text(user_msg) = msg else { continue };
        let prompt = user_msg.trim().to_string();
        if prompt.is_empty() {
            continue;
        }

        // Handle /cli_<agent> command — switch agent
        if let Some(rest) = prompt.strip_prefix("/cli_") {
            if let Some(kind) = AgentKind::from_str_loose(rest.trim()) {
                if kind.is_enabled() {
                    let _ = ws_tx.send(Message::Text(
                        wire::text_json(&format!("Switching to {} agent...\n", kind)).into()
                    )).await;
                    match start_agent(&mut active_agent, kind, &working_dir).await {
                        Ok(()) => {
                            let _ = ws_tx.send(Message::Text(
                                wire::text_json(&format!("{} agent started ✅\n", kind)).into()
                            )).await;
                            // Notify frontend of the switch
                            let switch_msg = serde_json::json!({
                                "type": "agent_switched",
                                "agent": kind.to_string(),
                            });
                            let _ = ws_tx.send(Message::Text(switch_msg.to_string().into())).await;
                        }
                        Err(e) => {
                            let _ = ws_tx.send(Message::Text(
                                wire::error_json(&format!("Failed to start {}: {}", kind, e)).into()
                            )).await;
                        }
                    }
                    let _ = ws_tx.send(Message::Text(wire::done_json().into())).await;
                    continue;
                }
            }
            // Unknown or disabled agent
            let _ = ws_tx.send(Message::Text(
                wire::error_json("Unknown or disabled agent").into()
            )).await;
            let _ = ws_tx.send(Message::Text(wire::done_json().into())).await;
            continue;
        }

        // Lazy-start default agent on first real message
        if active_agent.is_none() {
            let default_kind = AgentKind::from_str_loose(&default_agent_str)
                .unwrap_or(AgentKind::Claude);
            if let Err(e) = start_agent(&mut active_agent, default_kind, &working_dir).await {
                let _ = ws_tx.send(Message::Text(
                    wire::error_json(&format!("Failed to start default agent: {}", e)).into()
                )).await;
                let _ = ws_tx.send(Message::Text(wire::done_json().into())).await;
                continue;
            }
        }

        // Send message to active agent and stream events back
        let agent = active_agent.as_ref().unwrap();
        let mut rx = agent.subscribe();

        if let Err(e) = agent.send_message_fire(&prompt).await {
            // Check if agent died — try to restart
            let is_dead = e.contains("shut down") || e.contains("gone") || e.contains("ACP thread");
            let _ = ws_tx.send(Message::Text(
                wire::error_json(&e).into()
            )).await;
            if is_dead {
                let kind = agent.kind();
                let _ = ws_tx.send(Message::Text(
                    wire::text_json(&format!("⚠️ {} agent crashed, restarting...\n", kind)).into()
                )).await;
                if let Ok(()) = start_agent(&mut active_agent, kind, &working_dir).await {
                    let _ = ws_tx.send(Message::Text(
                        wire::text_json(&format!("{} agent restarted ✅\n", kind)).into()
                    )).await;
                    // Retry the message
                    let agent = active_agent.as_ref().unwrap();
                    rx = agent.subscribe();
                    if let Err(e2) = agent.send_message_fire(&prompt).await {
                        let _ = ws_tx.send(Message::Text(wire::error_json(&e2).into())).await;
                        let _ = ws_tx.send(Message::Text(wire::done_json().into())).await;
                        continue;
                    }
                } else {
                    let _ = ws_tx.send(Message::Text(wire::done_json().into())).await;
                    continue;
                }
            } else {
                let _ = ws_tx.send(Message::Text(wire::done_json().into())).await;
                continue;
            }
        }

        // Stream agent events to WebSocket
        loop {
            match rx.recv().await {
                Ok(event) => match event {
                    AgentEvent::Text(text) => {
                        let _ = ws_tx.send(Message::Text(wire::text_json(&text).into())).await;
                    }
                    AgentEvent::Thinking(_) => {
                        // Web chat doesn't show thinking blocks for now
                    }
                    AgentEvent::Progress(status) => {
                        let json = serde_json::json!({ "progress": status }).to_string();
                        let _ = ws_tx.send(Message::Text(json.into())).await;
                    }
                    AgentEvent::ToolUse { name, .. } => {
                        let json = serde_json::json!({ "progress": format!("Using tool: {}...", name) }).to_string();
                        let _ = ws_tx.send(Message::Text(json.into())).await;
                    }
                    AgentEvent::ToolResult { .. } => {}
                    AgentEvent::TurnComplete { .. } => {
                        break;
                    }
                    AgentEvent::Error(err) => {
                        let _ = ws_tx.send(Message::Text(wire::error_json(&err).into())).await;
                    }
                },
                Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                    eprintln!("[VibeAround][ws/chat] event stream lagged by {}", n);
                }
                Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                    let _ = ws_tx.send(Message::Text(
                        wire::error_json("Agent process ended unexpectedly").into()
                    )).await;
                    break;
                }
            }
        }

        let _ = ws_tx.send(Message::Text(wire::done_json().into())).await;
    }

    // Clean up agent on disconnect
    if let Some(mut agent) = active_agent.take() {
        agent.shutdown().await;
    }
}
