//! IM worker: receive InboundMessage, dispatch to active ACP agent, stream events back to IM.
//!
//! All legacy session/project/classify_intent routing has been removed.
//! The worker starts a default Claude agent on first message and sends everything directly to it.
//! Use `/cli claude` or `/cli gemini` to switch agents.

use std::sync::Arc;

use dashmap::DashMap;
use tokio::sync::mpsc;

use super::daemon::{OutboundHub, OutboundMsg};
use crate::agent::{self, AgentBackend, AgentEvent, AgentKind};
use crate::config;

/// Attachment metadata from Feishu file/image messages.
#[derive(Debug, Clone)]
pub struct FeishuAttachment {
    pub message_id: String,
    pub file_key: String,
    pub file_name: String,
    /// "file" or "image"
    pub resource_type: String,
}

/// Inbound message from any IM channel to the worker.
#[derive(Debug, Clone)]
pub struct InboundMessage {
    pub channel_id: String,
    pub text: String,
    pub attachments: Vec<FeishuAttachment>,
    pub parent_id: Option<String>,
}

impl InboundMessage {
    pub fn text_only(channel_id: String, text: String) -> Self {
        Self { channel_id, text, attachments: vec![], parent_id: None }
    }
}

pub async fn run_worker<T>(
    mut inbound_rx: mpsc::Receiver<InboundMessage>,
    outbound: Arc<OutboundHub<T>>,
    busy_set: Arc<DashMap<String, ()>>,
    _feishu_transport: Option<Arc<crate::im::channels::feishu::FeishuTransport>>,
) where
    T: crate::im::transport::ImTransport + 'static,
{
    let working_dir = config::ensure_loaded().working_dir.clone();

    // Default agent: Claude, started lazily on first message.
    let mut active_agent: Option<Box<dyn AgentBackend>> = None;

    while let Some(msg) = inbound_rx.recv().await {
        let channel_id = msg.channel_id.clone();
        busy_set.insert(channel_id.clone(), ());

        // --- /cli command: switch agent ---
        if let Some(kind) = parse_cli_command(&msg.text) {
            switch_agent(&mut active_agent, kind, &working_dir, &channel_id, &outbound).await;
            busy_set.remove(&channel_id);
            continue;
        }

        // --- Ensure an agent is running (lazy start Claude on first message) ---
        if active_agent.is_none() {
            eprintln!("[VibeAround][im][worker] no active agent, starting default (claude)...");
            switch_agent(&mut active_agent, AgentKind::Claude, &working_dir, &channel_id, &outbound).await;
            if active_agent.is_none() {
                // Failed to start — error already sent by switch_agent
                busy_set.remove(&channel_id);
                continue;
            }
        }

        // --- Send message to agent ---
        let agent = active_agent.as_ref().unwrap();
        run_with_agent(agent.as_ref(), &msg, &channel_id, &outbound).await;

        busy_set.remove(&channel_id);
    }
}

/// Parse `/cli <agent>` command. Returns the AgentKind if matched.
fn parse_cli_command(text: &str) -> Option<AgentKind> {
    let text = text.trim();
    let rest = text.strip_prefix("/cli ")
        .or_else(|| text.strip_prefix("/cli\t"))?;
    AgentKind::from_str_loose(rest.trim())
}

/// Shut down the current agent (if any) and start a new one.
async fn switch_agent<T>(
    active_agent: &mut Option<Box<dyn AgentBackend>>,
    kind: AgentKind,
    working_dir: &std::path::Path,
    channel_id: &str,
    outbound: &Arc<OutboundHub<T>>,
) where
    T: crate::im::transport::ImTransport + 'static,
{
    // Shut down existing
    if let Some(mut old) = active_agent.take() {
        old.shutdown().await;
    }

    // Start new
    let mut backend = agent::create_backend(kind);
    match backend.start(working_dir).await {
        Ok(()) => {
            let _ = outbound.send(channel_id, OutboundMsg::StreamPart(
                channel_id.to_string(),
                format!("[✓ {} agent started in workspace root]", kind),
            )).await;
            *active_agent = Some(backend);
        }
        Err(e) => {
            let _ = outbound.send(channel_id, OutboundMsg::StreamPart(
                channel_id.to_string(),
                format!("[✗ Failed to start {} agent: {}]", kind, e),
            )).await;
        }
    }
    let _ = outbound.send(channel_id, OutboundMsg::StreamEnd(channel_id.to_string())).await;
}

/// Send a message to the active agent and stream events back to IM.
async fn run_with_agent<T>(
    agent: &dyn AgentBackend,
    msg: &InboundMessage,
    channel_id: &str,
    outbound: &Arc<OutboundHub<T>>,
) where
    T: crate::im::transport::ImTransport + 'static,
{
    let mut rx = agent.subscribe();

    if let Err(e) = agent.send_message(&msg.text).await {
        let _ = outbound.send(channel_id, OutboundMsg::StreamPart(
            channel_id.to_string(), format!("[Agent error: {}]", e),
        )).await;
        let _ = outbound.send(channel_id, OutboundMsg::StreamEnd(channel_id.to_string())).await;
        return;
    }

    loop {
        match rx.recv().await {
            Ok(event) => match event {
                AgentEvent::Text(text) => {
                    let _ = outbound.send(channel_id, OutboundMsg::StreamPart(
                        channel_id.to_string(), text,
                    )).await;
                }
                AgentEvent::Progress(status) => {
                    let _ = outbound.send(channel_id, OutboundMsg::StreamProgress(
                        channel_id.to_string(), status,
                    )).await;
                }
                AgentEvent::ToolUse { name } => {
                    let _ = outbound.send(channel_id, OutboundMsg::StreamProgress(
                        channel_id.to_string(), format!("🔧 {}...", name),
                    )).await;
                }
                AgentEvent::TurnComplete { cost_usd, .. } => {
                    if let Some(cost) = cost_usd {
                        let _ = outbound.send(channel_id, OutboundMsg::StreamProgress(
                            channel_id.to_string(), format!("💰 ${:.4}", cost),
                        )).await;
                    }
                    break;
                }
                AgentEvent::Error(err) => {
                    let _ = outbound.send(channel_id, OutboundMsg::StreamPart(
                        channel_id.to_string(), format!("[Error: {}]", err),
                    )).await;
                }
            },
            Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                eprintln!("[VibeAround][im][worker] agent event stream lagged by {}", n);
            }
            Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                let _ = outbound.send(channel_id, OutboundMsg::StreamPart(
                    channel_id.to_string(), "[Agent process ended unexpectedly]".to_string(),
                )).await;
                break;
            }
        }
    }

    let _ = outbound.send(channel_id, OutboundMsg::StreamEnd(channel_id.to_string())).await;
}
