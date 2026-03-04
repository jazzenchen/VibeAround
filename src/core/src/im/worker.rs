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
use crate::config::{self, ImVerboseConfig};

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
    /// The platform message_id of the user's message (for reactions and reply-quoting).
    pub user_message_id: Option<String>,
}

impl InboundMessage {
    pub fn text_only(channel_id: String, text: String) -> Self {
        Self { channel_id, text, attachments: vec![], parent_id: None, user_message_id: None }
    }
}

pub async fn run_worker<T>(
    mut inbound_rx: mpsc::Receiver<InboundMessage>,
    outbound: Arc<OutboundHub<T>>,
    busy_set: Arc<DashMap<String, ()>>,
    _feishu_transport: Option<Arc<crate::im::channels::feishu::FeishuTransport>>,
    verbose: ImVerboseConfig,
) where
    T: crate::im::transport::ImTransport + 'static,
{
    let working_dir = config::ensure_loaded().working_dir.clone();

    // Default agent: Claude, started lazily on first message.
    let mut active_agent: Option<Box<dyn AgentBackend>> = None;

    while let Some(msg) = inbound_rx.recv().await {
        let channel_id = msg.channel_id.clone();
        busy_set.insert(channel_id.clone(), ());

        // --- /help command ---
        if msg.text.trim() == "/help" {
            send_help(&channel_id, &outbound).await;
            busy_set.remove(&channel_id);
            continue;
        }

        // --- /start command: quick agent picker ---
        if msg.text.trim() == "/start" {
            send_start(&channel_id, &outbound).await;
            busy_set.remove(&channel_id);
            continue;
        }

        // --- /cli command: switch agent ---
        if let Some(kind) = parse_cli_command(&msg.text) {
            switch_agent(&mut active_agent, kind, &working_dir, &channel_id, &outbound, msg.user_message_id.as_deref()).await;
            busy_set.remove(&channel_id);
            continue;
        }

        // --- Ensure an agent is running (lazy start default agent on first message) ---
        if active_agent.is_none() {
            let default_kind = agent::AgentKind::from_str_loose(&config::ensure_loaded().default_agent)
                .unwrap_or(AgentKind::Claude);
            eprintln!("[VibeAround][im][worker] no active agent, starting default ({})...", default_kind);
            switch_agent(&mut active_agent, default_kind, &working_dir, &channel_id, &outbound, msg.user_message_id.as_deref()).await;
            if active_agent.is_none() {
                busy_set.remove(&channel_id);
                continue;
            }
        }

        // --- Send message to agent ---
        let agent = active_agent.as_ref().unwrap();
        run_with_agent(agent.as_ref(), &msg, &channel_id, &outbound, &verbose).await;

        busy_set.remove(&channel_id);
    }
}

/// Parse `/cli_<agent>` command. Returns the AgentKind if matched.
fn parse_cli_command(text: &str) -> Option<AgentKind> {
    let text = text.trim();
    let rest = text.strip_prefix("/cli_")?;
    AgentKind::from_str_loose(rest.trim())
}

/// Shut down the current agent (if any) and start a new one.
/// Sends an immediate status message, then marks it done (reaction) when complete.
async fn switch_agent<T>(
    active_agent: &mut Option<Box<dyn AgentBackend>>,
    kind: AgentKind,
    working_dir: &std::path::Path,
    channel_id: &str,
    outbound: &Arc<OutboundHub<T>>,
    _user_message_id: Option<&str>,
) where
    T: crate::im::transport::ImTransport + 'static,
{
    // Send an immediate status message so the user sees something right away.
    let status_msg = format!("Starting {} agent...", kind);
    let status_message_id: Option<String> = outbound.send_direct(channel_id, &status_msg).await;

    // Shut down existing agent
    if let Some(mut old) = active_agent.take() {
        old.shutdown().await;
    }

    // Start new agent
    let mut backend = agent::create_backend(kind);
    match backend.start(working_dir).await {
        Ok(()) => {
            *active_agent = Some(backend);
            // Edit the status message to show success
            if let Some(ref mid) = status_message_id {
                outbound.edit_direct(channel_id, mid, &format!("{} agent started ✅", kind)).await;
            }
        }
        Err(e) => {
            // Edit the status message to show failure, or send a new one if no message_id
            let err_msg = format!("Failed to start {} agent: {}", kind, e);
            if let Some(ref mid) = status_message_id {
                outbound.edit_direct(channel_id, mid, &err_msg).await;
            } else {
                let _ = outbound.send(channel_id, OutboundMsg::Send(
                    channel_id.to_string(), err_msg,
                )).await;
            }
        }
    };
}

/// Truncate tool output for display in IM (avoid flooding).
fn truncate_tool_output(text: &str, max_len: usize) -> String {
    if text.len() <= max_len {
        text.to_string()
    } else {
        format!("{}…({} bytes)", &text[..max_len], text.len())
    }
}

/// Send a message to the active agent and stream events back to IM.
/// Emits all agent output: text, thinking, tool calls, tool results.
/// Uses reactions on bot messages to indicate processing/done state.
/// Replies to the user's message for the first block.
async fn run_with_agent<T>(
    agent: &dyn AgentBackend,
    msg: &InboundMessage,
    channel_id: &str,
    outbound: &Arc<OutboundHub<T>>,
    verbose: &ImVerboseConfig,
) where
    T: crate::im::transport::ImTransport + 'static,
{
    let caps = outbound.capabilities();
    let mut rx = agent.subscribe();

    // Set reply_to so the first flushed bot message quotes the user's message
    if let Some(ref user_mid) = msg.user_message_id {
        outbound.set_reply_to(channel_id, user_mid.clone()).await;
    }

    // Add processing reaction to the user's message to indicate we're working on it
    if let Some(ref user_mid) = msg.user_message_id {
        let _ = outbound.send(channel_id, OutboundMsg::AddReaction(
            channel_id.to_string(), user_mid.clone(), caps.processing_reaction.to_string(),
        )).await;
    }

    if let Err(e) = agent.send_message(&msg.text).await {
        let _ = outbound.send(channel_id, OutboundMsg::StreamPart(
            channel_id.to_string(), format!("[Agent error: {}]", e),
        )).await;
        let _ = outbound.send(channel_id, OutboundMsg::StreamDone(channel_id.to_string())).await;
        // Remove processing reaction on error
        if let Some(ref user_mid) = msg.user_message_id {
            let _ = outbound.send(channel_id, OutboundMsg::RemoveReaction(
                channel_id.to_string(), user_mid.clone(), caps.processing_reaction.to_string(),
            )).await;
        }
        return;
    }

    // Track current block type so we can flush buffer between different content types.
    // Each block (thinking, text, tool_use, tool_result) becomes a separate IM message.
    #[derive(PartialEq, Clone, Copy)]
    enum Block { None, Thinking, Text, Tool }
    let mut current_block = Block::None;

    /// Flush the current buffer block (sends StreamEnd so daemon flushes accumulated parts).
    async fn flush_block<T2: crate::im::transport::ImTransport + 'static>(
        channel_id: &str, outbound: &Arc<OutboundHub<T2>>,
    ) {
        let _ = outbound.send(channel_id, OutboundMsg::StreamEnd(channel_id.to_string())).await;
    }

    loop {
        match rx.recv().await {
            Ok(event) => match event {
                AgentEvent::Text(text) => {
                    if current_block != Block::Text {
                        if current_block != Block::None {
                            flush_block(channel_id, outbound).await;
                        }
                        current_block = Block::Text;
                    }
                    let _ = outbound.send(channel_id, OutboundMsg::StreamPart(
                        channel_id.to_string(), text,
                    )).await;
                }
                AgentEvent::Thinking(text) => {
                    if !verbose.show_thinking {
                        continue;
                    }
                    if current_block != Block::Thinking {
                        if current_block != Block::None {
                            flush_block(channel_id, outbound).await;
                        }
                        current_block = Block::Thinking;
                        let _ = outbound.send(channel_id, OutboundMsg::StreamPart(
                            channel_id.to_string(), "🧠 Thinking...\n".to_string(),
                        )).await;
                    }
                    let _ = outbound.send(channel_id, OutboundMsg::StreamPart(
                        channel_id.to_string(), text,
                    )).await;
                }
                AgentEvent::Progress(status) => {
                    let _ = outbound.send(channel_id, OutboundMsg::StreamProgress(
                        channel_id.to_string(), status,
                    )).await;
                }
                AgentEvent::ToolUse { name, id: _, input } => {
                    if !verbose.show_tool_use {
                        continue;
                    }
                    if current_block != Block::None {
                        flush_block(channel_id, outbound).await;
                    }
                    current_block = Block::Tool;
                    let mut tool_msg = format!("🔧 **{}**", name);
                    if let Some(ref inp) = input {
                        let summary = truncate_tool_output(inp, 300);
                        tool_msg.push_str(&format!("\n```\n{}\n```", summary));
                    }
                    tool_msg.push('\n');
                    let _ = outbound.send(channel_id, OutboundMsg::StreamPart(
                        channel_id.to_string(), tool_msg,
                    )).await;
                }
                AgentEvent::ToolResult { id: _, output, is_error } => {
                    if !verbose.show_tool_use {
                        continue;
                    }
                    if current_block != Block::None {
                        flush_block(channel_id, outbound).await;
                    }
                    current_block = Block::Tool;
                    let pfx = if is_error { "❌" } else { "✅" };
                    let mut result_msg = format!("{} Tool result", pfx);
                    if let Some(ref out) = output {
                        let summary = truncate_tool_output(out, 500);
                        if !summary.is_empty() {
                            result_msg.push_str(&format!(":\n```\n{}\n```", summary));
                        }
                    }
                    result_msg.push('\n');
                    let _ = outbound.send(channel_id, OutboundMsg::StreamPart(
                        channel_id.to_string(), result_msg,
                    )).await;
                }
                AgentEvent::TurnComplete { .. } => {
                    break;
                }
                AgentEvent::Error(err) => {
                    let _ = outbound.send(channel_id, OutboundMsg::StreamPart(
                        channel_id.to_string(), format!("[Error: {}]\n", err),
                    )).await;
                }
            },
            Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                eprintln!("[VibeAround][im][worker] agent event stream lagged by {}", n);
            }
            Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                let _ = outbound.send(channel_id, OutboundMsg::StreamPart(
                    channel_id.to_string(), "[Agent process ended unexpectedly]\n".to_string(),
                )).await;
                break;
            }
        }
    }

    // StreamDone: flush final block
    let _ = outbound.send(channel_id, OutboundMsg::StreamDone(channel_id.to_string())).await;

    // Remove processing reaction from the user's message now that we're done
    if let Some(ref user_mid) = msg.user_message_id {
        let _ = outbound.send(channel_id, OutboundMsg::RemoveReaction(
            channel_id.to_string(), user_mid.clone(), caps.processing_reaction.to_string(),
        )).await;
    }
}

/// Send a /help card with all commands and descriptions (card format, no buttons).
async fn send_help<T>(
    channel_id: &str,
    outbound: &Arc<OutboundHub<T>>,
) where
    T: crate::im::transport::ImTransport + 'static,
{
    let prompt = concat!(
        "`/cli_claude` — Anthropic Claude Code\n",
        "`/cli_gemini` — Google Gemini CLI\n",
        "`/cli_opencode` — OpenCode AI Agent\n",
        "`/cli_codex` — OpenAI Codex CLI\n",
        "🚀 `/start` — Quick agent picker\n",
        "❓ `/help` — Show this help",
    );

    // Send as interactive card with no buttons (prompt-only card)
    let _ = outbound.send(channel_id, OutboundMsg::SendInteractive {
        channel_id: channel_id.to_string(),
        prompt: prompt.to_string(),
        options: vec![],
        reply_to: None,
    }).await;
}

/// Send a /start interactive card — compact agent picker with help button on separate row.
async fn send_start<T>(
    channel_id: &str,
    outbound: &Arc<OutboundHub<T>>,
) where
    T: crate::im::transport::ImTransport + 'static,
{
    use crate::im::transport::{ButtonStyle, InteractiveOption};

    let prompt = "**Select an agent to start:**";
    let options = vec![
        InteractiveOption { label: "Claude".into(), value: "/cli_claude".into(), style: ButtonStyle::Primary, group: 0 },
        InteractiveOption { label: "Gemini".into(), value: "/cli_gemini".into(), style: ButtonStyle::Default, group: 0 },
        InteractiveOption { label: "OpenCode".into(), value: "/cli_opencode".into(), style: ButtonStyle::Default, group: 0 },
        InteractiveOption { label: "Codex".into(), value: "/cli_codex".into(), style: ButtonStyle::Default, group: 0 },
        InteractiveOption { label: "❓ Help".into(), value: "/help".into(), style: ButtonStyle::Default, group: 1 },
    ];

    let _ = outbound.send(channel_id, OutboundMsg::SendInteractive {
        channel_id: channel_id.to_string(),
        prompt: prompt.to_string(),
        options,
        reply_to: None,
    }).await;
}
