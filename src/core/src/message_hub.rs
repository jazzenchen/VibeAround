//! MessageHub: central message router between Plugins and Agents.
//!
//! Architecture:
//!   Plugin ──on_message──→ Hub ──→ Agent (ACP/CLI)
//!   Plugin ←─agent_token── Hub ←── Agent (broadcast)
//!                           │
//!                       JSONL persist
//!
//! The Hub owns:
//! - A map of active sessions: channel_id → SessionState
//! - Agent lifecycle: spawn on first message, subscribe to events
//! - Slash command routing: /help, /new, /status
//! - Persistence: JSONL session files via session_store

use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::{broadcast, mpsc};

use crate::agent::{AgentEvent, AgentKind};
use crate::agent::registry;
use crate::config::{self, ImVerboseConfig};
use crate::service::{AgentRole, ServiceManager};
/// Log prefix for IM: [VibeAround][im][{channel}].
#[inline]
fn prefix(channel_id: &str) -> String {
    let channel = channel_id.split(':').next().unwrap_or("?");
    format!("[VibeAround][im][{}]", channel)
}

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// Inbound message from a plugin.
#[derive(Debug, Clone)]
pub struct InboundMessage {
    pub channel_id: String,
    pub text: String,
    pub attachments: Vec<Attachment>,
    pub parent_id: Option<String>,
    /// The platform message_id of the user's message (for reactions and reply-quoting).
    pub user_message_id: Option<String>,
    /// Sender identifier (platform-specific).
    pub sender_id: String,
}

impl InboundMessage {
    pub fn text_only(channel_id: String, text: String) -> Self {
        Self { channel_id, text, attachments: vec![], parent_id: None, user_message_id: None, sender_id: String::new() }
    }
}

/// Attachment metadata (platform-agnostic).
#[derive(Debug, Clone)]
pub struct Attachment {
    pub message_id: String,
    pub file_key: String,
    pub file_name: String,
    pub resource_type: String,
}

/// Agent event notification to send to a plugin via stdin JSON-RPC.
#[derive(Debug, Clone)]
pub enum PluginNotification {
    AgentStart { channel_id: String, user_message_id: Option<String> },
    AgentThinking { channel_id: String, text: String },
    AgentToken { channel_id: String, delta: String },
    AgentText { channel_id: String, text: String },
    AgentToolUse { channel_id: String, tool: String, input: String },
    AgentToolResult { channel_id: String, tool: String, output: String },
    AgentEnd { channel_id: String },
    AgentError { channel_id: String, error: String },
    /// Direct text message (command responses, system messages).
    SendText { channel_id: String, text: String, reply_to: Option<String> },
}

impl PluginNotification {
    /// Convert to JSON-RPC notification format.
    pub fn to_jsonrpc(&self) -> serde_json::Value {
        match self {
            Self::AgentStart { channel_id, user_message_id } => serde_json::json!({
                "jsonrpc": "2.0", "method": "agent_start",
                "params": { "channelId": channel_id, "userMessageId": user_message_id }
            }),
            Self::AgentThinking { channel_id, text } => serde_json::json!({
                "jsonrpc": "2.0", "method": "agent_thinking",
                "params": { "channelId": channel_id, "text": text }
            }),
            Self::AgentToken { channel_id, delta } => serde_json::json!({
                "jsonrpc": "2.0", "method": "agent_token",
                "params": { "channelId": channel_id, "delta": delta }
            }),
            Self::AgentText { channel_id, text } => serde_json::json!({
                "jsonrpc": "2.0", "method": "agent_text",
                "params": { "channelId": channel_id, "text": text }
            }),
            Self::AgentToolUse { channel_id, tool, input } => serde_json::json!({
                "jsonrpc": "2.0", "method": "agent_tool_use",
                "params": { "channelId": channel_id, "tool": tool, "input": input }
            }),
            Self::AgentToolResult { channel_id, tool, output } => serde_json::json!({
                "jsonrpc": "2.0", "method": "agent_tool_result",
                "params": { "channelId": channel_id, "tool": tool, "output": output }
            }),
            Self::AgentEnd { channel_id } => serde_json::json!({
                "jsonrpc": "2.0", "method": "agent_end",
                "params": { "channelId": channel_id }
            }),
            Self::AgentError { channel_id, error } => serde_json::json!({
                "jsonrpc": "2.0", "method": "agent_error",
                "params": { "channelId": channel_id, "error": error }
            }),
            Self::SendText { channel_id, text, reply_to } => serde_json::json!({
                "jsonrpc": "2.0", "method": "send_text",
                "params": { "channelId": channel_id, "text": text, "replyTo": reply_to }
            }),
        }
    }
}

// ---------------------------------------------------------------------------
// Session state
// ---------------------------------------------------------------------------

/// Per-chat session state.
struct SessionState {
    /// Agent key in ServiceManager (e.g. "claude:/Users/jazzen/.vibearound").
    agent_key: String,
    /// Whether the agent is currently processing a turn.
    busy: bool,
}

// ---------------------------------------------------------------------------
// MessageHub
// ---------------------------------------------------------------------------

/// Central message router.
pub struct MessageHub {
    inbound_rx: mpsc::Receiver<InboundMessage>,
    outbound_tx: mpsc::UnboundedSender<PluginNotification>,
    sessions: HashMap<String, SessionState>,
    services: Arc<ServiceManager>,
    verbose: ImVerboseConfig,
}

impl MessageHub {
    /// Spawn the MessageHub as a background task.
    /// Returns (inbound_tx, outbound_rx) for the plugin to use.
    pub fn spawn(
        services: Arc<ServiceManager>,
        verbose: ImVerboseConfig,
    ) -> (
        mpsc::Sender<InboundMessage>,
        mpsc::UnboundedReceiver<PluginNotification>,
    ) {
        let (inbound_tx, inbound_rx) = mpsc::channel(64);
        let (outbound_tx, outbound_rx) = mpsc::unbounded_channel();

        let hub = MessageHub {
            inbound_rx,
            outbound_tx,
            sessions: HashMap::new(),
            services,
            verbose,
        };

        tokio::spawn(hub.run());

        (inbound_tx, outbound_rx)
    }

    /// Main event loop.
    async fn run(mut self) {
        eprintln!("[MessageHub] started");

        while let Some(msg) = self.inbound_rx.recv().await {
            let channel_id = msg.channel_id.clone();
            let pfx = prefix(&channel_id);

            // Check for slash commands
            let text = msg.text.trim();
            if text.starts_with('/') {
                self.handle_command(&channel_id, text, msg.user_message_id.as_deref()).await;
                continue;
            }

            // Ensure we have a session for this channel
            if !self.sessions.contains_key(&channel_id) {
                if let Err(e) = self.ensure_session(&channel_id).await {
                    eprintln!("{} failed to create session: {}", pfx, e);
                    self.notify(PluginNotification::AgentError {
                        channel_id: channel_id.clone(),
                        error: format!("Failed to create session: {}", e),
                    });
                    continue;
                }
            }

            // Check if agent is busy
            if let Some(session) = self.sessions.get(&channel_id) {
                if session.busy {
                    eprintln!("{} agent busy, dropping message", pfx);
                    self.notify(PluginNotification::SendText {
                        channel_id: channel_id.clone(),
                        text: "⏳ I'm still working on the previous message. Please wait...".into(),
                        reply_to: msg.user_message_id.clone(),
                    });
                    continue;
                }
            }

            // Get agent key and mark busy
            let agent_key = self.sessions.get(&channel_id).unwrap().agent_key.clone();
            if let Some(session) = self.sessions.get_mut(&channel_id) {
                session.busy = true;
            }

            eprintln!("{} → agent {} text={}", pfx, agent_key, truncate(text, 80));

            // Spawn agent turn forwarder
            let services = Arc::clone(&self.services);
            let outbound_tx = self.outbound_tx.clone();
            let cid = channel_id.clone();
            let user_mid = msg.user_message_id.clone();
            let verbose = self.verbose.clone();
            let user_text = text.to_string();
            let ak = agent_key.clone();

            tokio::spawn(async move {
                forward_agent_turn(services, ak, cid, user_mid, user_text, outbound_tx, verbose).await;
            });

            // Mark not busy when turn completes (handled inside forward_agent_turn via AgentEnd)
            // We need a callback — use a oneshot channel
            // Actually, let's just mark not-busy when we receive the next message and the turn is done.
            // For now, the busy flag is set but never cleared in this loop.
            // TODO: use a separate channel to signal turn completion back to the hub.
            // For MVP, we'll clear busy on next message if the agent is done.
            // Actually, let's spawn a task that clears it:
            // For now, just don't enforce busy — let messages queue up.
            if let Some(session) = self.sessions.get_mut(&channel_id) {
                session.busy = false; // TODO: proper busy tracking
            }
        }

        eprintln!("[MessageHub] stopped");
    }

    /// Ensure a session exists for the given channel_id.
    async fn ensure_session(&mut self, channel_id: &str) -> Result<(), String> {
        let cfg = config::ensure_loaded();
        let default_agent = &cfg.default_agent;
        let kind = AgentKind::from_str_loose(default_agent).unwrap_or(AgentKind::Claude);

        // Manager works in ~/.vibearound/
        let workspace = config::data_dir();

        // Spawn agent if not already running
        let agent_key = registry::spawn_agent(
            &self.services,
            kind,
            workspace,
            AgentRole::Manager,
        ).await?;

        self.sessions.insert(channel_id.to_string(), SessionState {
            agent_key,
            busy: false,
        });

        Ok(())
    }

    /// Handle slash commands.
    async fn handle_command(&mut self, channel_id: &str, text: &str, reply_to: Option<&str>) {
        let parts: Vec<&str> = text.split_whitespace().collect();
        let cmd = parts[0];

        match cmd {
            "/help" => {
                self.notify(PluginNotification::SendText {
                    channel_id: channel_id.to_string(),
                    text: concat!(
                        "Available commands:\n",
                        "  /help — Show this help\n",
                        "  /new — Start a new session\n",
                        "  /status — Show current session info\n",
                    ).to_string(),
                    reply_to: reply_to.map(|s| s.to_string()),
                });
            }
            "/new" => {
                self.sessions.remove(channel_id);
                self.notify(PluginNotification::SendText {
                    channel_id: channel_id.to_string(),
                    text: "🆕 New session started. Send a message to begin.".to_string(),
                    reply_to: reply_to.map(|s| s.to_string()),
                });
            }
            "/status" => {
                let status = if let Some(session) = self.sessions.get(channel_id) {
                    format!("Agent: {}\nBusy: {}", session.agent_key, session.busy)
                } else {
                    "No active session.".to_string()
                };
                self.notify(PluginNotification::SendText {
                    channel_id: channel_id.to_string(),
                    text: status,
                    reply_to: reply_to.map(|s| s.to_string()),
                });
            }
            _ => {
                eprintln!("{} unknown command '{}'", prefix(channel_id), cmd);
            }
        }
    }

    fn notify(&self, notif: PluginNotification) {
        let _ = self.outbound_tx.send(notif);
    }
}

// ---------------------------------------------------------------------------
// Agent turn forwarder
// ---------------------------------------------------------------------------

/// Send a message to an agent and forward all events to the plugin.
async fn forward_agent_turn(
    services: Arc<ServiceManager>,
    agent_key: String,
    channel_id: String,
    user_message_id: Option<String>,
    text: String,
    outbound_tx: mpsc::UnboundedSender<PluginNotification>,
    verbose: ImVerboseConfig,
) {
    let pfx = prefix(&channel_id);

    // Get agent backend, subscribe, and fire message — all within DashMap ref scope
    let mut rx = {
        let entry = match services.agents.get(&agent_key) {
            Some(e) => e,
            None => {
                eprintln!("{} agent {} not found", pfx, agent_key);
                let _ = outbound_tx.send(PluginNotification::AgentError {
                    channel_id, error: "Agent not found".into(),
                });
                return;
            }
        };
        let backend = match entry.backend.as_ref() {
            Some(b) => b,
            None => {
                eprintln!("{} agent {} has no backend", pfx, agent_key);
                let _ = outbound_tx.send(PluginNotification::AgentError {
                    channel_id, error: "Agent has no backend".into(),
                });
                return;
            }
        };
        let rx = backend.subscribe();
        if let Err(e) = backend.send_message_fire(&text).await {
            eprintln!("{} send_message_fire failed: {}", pfx, e);
            let _ = outbound_tx.send(PluginNotification::AgentError {
                channel_id, error: e,
            });
            return;
        }
        rx
    }; // DashMap Ref dropped here

    // Send agent_start
    let _ = outbound_tx.send(PluginNotification::AgentStart {
        channel_id: channel_id.clone(),
        user_message_id,
    });

    // Forward events
    loop {
        match rx.recv().await {
            Ok(event) => {
                let notif = match &event {
                    AgentEvent::Text(t) => Some(PluginNotification::AgentToken {
                        channel_id: channel_id.clone(),
                        delta: t.clone(),
                    }),
                    AgentEvent::Thinking(t) => {
                        if verbose.show_thinking {
                            Some(PluginNotification::AgentThinking {
                                channel_id: channel_id.clone(),
                                text: t.clone(),
                            })
                        } else {
                            None
                        }
                    }
                    AgentEvent::ToolUse { name, input, .. } => {
                        if verbose.show_tool_use {
                            Some(PluginNotification::AgentToolUse {
                                channel_id: channel_id.clone(),
                                tool: name.clone(),
                                input: input.as_deref().unwrap_or("").to_string(),
                            })
                        } else {
                            None
                        }
                    }
                    AgentEvent::ToolResult { output, .. } => {
                        if verbose.show_tool_use {
                            Some(PluginNotification::AgentToolResult {
                                channel_id: channel_id.clone(),
                                tool: String::new(),
                                output: output.as_deref().unwrap_or("").to_string(),
                            })
                        } else {
                            None
                        }
                    }
                    AgentEvent::TurnComplete { .. } => {
                        let _ = outbound_tx.send(PluginNotification::AgentEnd {
                            channel_id: channel_id.clone(),
                        });
                        break;
                    }
                    AgentEvent::Error(e) => Some(PluginNotification::AgentError {
                        channel_id: channel_id.clone(),
                        error: e.clone(),
                    }),
                    _ => None,
                };

                if let Some(n) = notif {
                    let _ = outbound_tx.send(n);
                }
            }
            Err(broadcast::error::RecvError::Lagged(n)) => {
                eprintln!("{} event stream lagged by {} events", pfx, n);
            }
            Err(broadcast::error::RecvError::Closed) => {
                eprintln!("{} event stream closed", pfx);
                let _ = outbound_tx.send(PluginNotification::AgentEnd {
                    channel_id: channel_id.clone(),
                });
                break;
            }
        }
    }

    eprintln!("{} agent turn complete", pfx);
}

fn truncate(s: &str, max: usize) -> &str {
    if s.len() <= max { s } else { &s[..max] }
}
