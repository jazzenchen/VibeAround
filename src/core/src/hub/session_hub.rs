//! SessionHub: session lifecycle, per-session message queue, slash commands.
//!
//! Responsibilities:
//! - Create/destroy sessions (keyed by channel_kind + chat_id)
//! - Per-session FIFO message queue with status tracking
//! - Dispatch messages to AgentHub when session is idle
//! - Handle slash commands (/help, /new, /status)
//! - Receive agent replies and forward to ChannelHub

use std::collections::{HashMap, VecDeque};
use std::sync::Arc;

use tokio::sync::{Mutex, OnceCell};

use crate::config;
use crate::hub::agent_hub::AgentHub;
use crate::hub::channel_hub::ChannelHub;
use crate::hub::types::*;

// ---------------------------------------------------------------------------
// Session state
// ---------------------------------------------------------------------------

/// Unique key for a session: "{channel_kind}:{chat_id}".
fn session_key(channel_kind: &str, chat_id: &str) -> String {
    format!("{}:{}", channel_kind, chat_id)
}

/// Per-session state.
struct Session {
    /// Channel kind (e.g. "feishu").
    channel_kind: ChannelKind,
    /// Chat id within the channel.
    chat_id: ChatId,
    /// CLI session id (set after agent spawn, updated on CLI switch).
    cli_session_id: Option<CliSessionId>,
    /// Agent CLI kind (e.g. "claude").
    cli_kind: Option<String>,
    /// Profile name (e.g. "default").
    profile: String,
    /// Whether the agent is currently processing a message.
    busy: bool,
    /// FIFO message queue for this session.
    queue: VecDeque<QueuedMessage>,
}

impl Session {
    fn new(channel_kind: ChannelKind, chat_id: ChatId) -> Self {
        Self {
            channel_kind,
            chat_id,
            cli_session_id: None,
            cli_kind: None,
            profile: "default".to_string(),
            busy: false,
            queue: VecDeque::new(),
        }
    }
}

// ---------------------------------------------------------------------------
// SessionHub
// ---------------------------------------------------------------------------

pub struct SessionHub {
    /// Sessions keyed by "{channel_kind}:{chat_id}".
    sessions: Mutex<HashMap<String, Session>>,
    /// Back-reference to ChannelHub (set after init).
    channel_hub: OnceCell<Arc<ChannelHub>>,
    /// Back-reference to AgentHub (set after init).
    agent_hub: OnceCell<Arc<AgentHub>>,
}

impl SessionHub {
    pub fn new() -> Self {
        Self {
            sessions: Mutex::new(HashMap::new()),
            channel_hub: OnceCell::new(),
            agent_hub: OnceCell::new(),
        }
    }

    /// Set the ChannelHub reference (two-phase init).
    pub fn set_channel_hub(&self, hub: Arc<ChannelHub>) {
        let _ = self.channel_hub.set(hub);
    }

    /// Set the AgentHub reference (two-phase init).
    pub fn set_agent_hub(&self, hub: Arc<AgentHub>) {
        let _ = self.agent_hub.set(hub);
    }

    fn channel_hub(&self) -> &Arc<ChannelHub> {
        self.channel_hub.get().expect("ChannelHub not initialized")
    }

    fn agent_hub(&self) -> &Arc<AgentHub> {
        self.agent_hub.get().expect("AgentHub not initialized")
    }

    // -----------------------------------------------------------------------
    // Inbound: receive message from ChannelHub
    // -----------------------------------------------------------------------

    /// Called by ChannelHub when a message arrives from a channel plugin.
    pub async fn receive(&self, msg: InboundMessage) {
        let text = msg.text.trim().to_string();

        // Check for slash commands
        if text.starts_with('/') {
            self.handle_command(&msg.channel_kind, &msg.chat_id, &text, Some(&msg.message_id)).await;
            return;
        }

        let key = session_key(&msg.channel_kind, &msg.chat_id);
        let pfx = format!("[SessionHub][{}]", key);

        let should_dispatch = {
            let mut sessions = self.sessions.lock().await;

            // Ensure session exists
            if !sessions.contains_key(&key) {
                eprintln!("{} creating new session", pfx);
                sessions.insert(key.clone(), Session::new(
                    msg.channel_kind.clone(),
                    msg.chat_id.clone(),
                ));
            }

            let session = sessions.get_mut(&key).unwrap();

            // Enqueue message
            session.queue.push_back(QueuedMessage {
                message: msg.clone(),
                status: MessageStatus::Unreplied,
            });

            eprintln!("{} enqueued msg_id={} queue_len={}", pfx, msg.message_id, session.queue.len());

            // If not busy, dispatch the front of the queue
            if !session.busy {
                if let Some(front) = session.queue.front_mut() {
                    front.status = MessageStatus::Processing;
                    session.busy = true;
                    true
                } else {
                    false
                }
            } else {
                eprintln!("{} agent busy, message queued", pfx);
                false
            }
        };

        if should_dispatch {
            // Get the message to dispatch (we know it's the front)
            let dispatch = {
                let sessions = self.sessions.lock().await;
                sessions.get(&key).and_then(|s| {
                    s.queue.front().map(|qm| {
                        (qm.message.clone(), s.cli_kind.clone(), Some(s.profile.clone()))
                    })
                })
            };

            if let Some((dispatch_msg, cli_kind, profile)) = dispatch {
                let verbose = config::ensure_loaded().channel_verbose(&dispatch_msg.channel_kind);
                self.agent_hub().dispatch(dispatch_msg, verbose, cli_kind, profile);
            }
        }
    }

    // -----------------------------------------------------------------------
    // Agent reply: receive events from AgentHub
    // -----------------------------------------------------------------------

    /// Called by AgentHub when an agent event arrives.
    pub async fn on_reply(&self, reply: AgentReply) {
        let key = session_key(&reply.channel_kind, &reply.chat_id);

        match &reply.event {
            AgentReplyEvent::Start => {
                self.channel_hub().send_notification(ChannelNotification::AgentStart {
                    channel_kind: reply.channel_kind.clone(),
                    chat_id: reply.chat_id.clone(),
                    message_id: reply.message_id.clone(),
                }).await;
            }
            AgentReplyEvent::Token { delta } => {
                self.channel_hub().send_notification(ChannelNotification::AgentToken {
                    channel_kind: reply.channel_kind.clone(),
                    chat_id: reply.chat_id.clone(),
                    delta: delta.clone(),
                }).await;
            }
            AgentReplyEvent::Thinking { text } => {
                self.channel_hub().send_notification(ChannelNotification::AgentThinking {
                    channel_kind: reply.channel_kind.clone(),
                    chat_id: reply.chat_id.clone(),
                    text: text.clone(),
                }).await;
            }
            AgentReplyEvent::ToolUse { tool, input } => {
                self.channel_hub().send_notification(ChannelNotification::AgentToolUse {
                    channel_kind: reply.channel_kind.clone(),
                    chat_id: reply.chat_id.clone(),
                    tool: tool.clone(),
                    input: input.clone(),
                }).await;
            }
            AgentReplyEvent::ToolResult { tool, output } => {
                self.channel_hub().send_notification(ChannelNotification::AgentToolResult {
                    channel_kind: reply.channel_kind.clone(),
                    chat_id: reply.chat_id.clone(),
                    tool: tool.clone(),
                    output: output.clone(),
                }).await;
            }
            AgentReplyEvent::Error { error } => {
                self.channel_hub().send_notification(ChannelNotification::AgentError {
                    channel_kind: reply.channel_kind.clone(),
                    chat_id: reply.chat_id.clone(),
                    error: error.clone(),
                }).await;
            }
            AgentReplyEvent::Complete { cli_session_id, cli_kind, profile } => {
                // Update session with CLI info
                {
                    let mut sessions = self.sessions.lock().await;
                    if let Some(session) = sessions.get_mut(&key) {
                        // If CLI returned a new session_id, it's a new session
                        if let Some(new_id) = cli_session_id {
                            if session.cli_session_id.as_ref() != Some(new_id) {
                                eprintln!("[SessionHub][{}] cli_session_id updated: {:?} → {}", key, session.cli_session_id, new_id);
                                session.cli_session_id = Some(new_id.clone());
                            }
                        }
                        session.cli_kind = Some(cli_kind.clone());
                        session.profile = profile.clone();
                    }
                }

                // Send agent_end to channel
                self.channel_hub().send_notification(ChannelNotification::AgentEnd {
                    channel_kind: reply.channel_kind.clone(),
                    chat_id: reply.chat_id.clone(),
                }).await;

                // Mark complete and try to dispatch next
                self.on_complete(&key).await;
            }
        }
    }

    /// Mark current message as replied and dispatch next in queue.
    async fn on_complete(&self, session_key: &str) {
        let next_msg = {
            let mut sessions = self.sessions.lock().await;
            if let Some(session) = sessions.get_mut(session_key) {
                // Pop the completed message
                if let Some(front) = session.queue.front() {
                    if front.status == MessageStatus::Processing {
                        session.queue.pop_front();
                    }
                }

                // Try to dispatch next
                if let Some(next) = session.queue.front_mut() {
                    next.status = MessageStatus::Processing;
                    session.busy = true;
                    Some((next.message.clone(), session.cli_kind.clone(), Some(session.profile.clone())))
                } else {
                    session.busy = false;
                    None
                }
            } else {
                None
            }
        };

        if let Some((msg, cli_kind, profile)) = next_msg {
            let verbose = config::ensure_loaded().channel_verbose(&msg.channel_kind);
            self.agent_hub().dispatch(msg, verbose, cli_kind, profile);
        }
    }

    // -----------------------------------------------------------------------
    // Slash commands
    // -----------------------------------------------------------------------

    async fn handle_command(&self, channel_kind: &str, chat_id: &str, text: &str, reply_to: Option<&str>) {
        let parts: Vec<&str> = text.split_whitespace().collect();
        let cmd = parts[0];
        let key = session_key(channel_kind, chat_id);

        match cmd {
            "/help" => {
                self.channel_hub().send_notification(ChannelNotification::SendText {
                    channel_kind: channel_kind.to_string(),
                    chat_id: chat_id.to_string(),
                    text: concat!(
                        "Available commands:\n",
                        "  /help — Show this help\n",
                        "  /new — Start a new session\n",
                        "  /status — Show current session info\n",
                        "  /agent <name> — Switch current agent\n",
                    ).to_string(),
                    reply_to: reply_to.map(|s| s.to_string()),
                }).await;
            }
            "/new" => {
                // Kill all agents for this chat
                self.agent_hub().kill_chat_agents(channel_kind, chat_id).await;
                {
                    let mut sessions = self.sessions.lock().await;
                    sessions.remove(&key);
                }
                self.channel_hub().send_notification(ChannelNotification::SendText {
                    channel_kind: channel_kind.to_string(),
                    chat_id: chat_id.to_string(),
                    text: "🆕 New session started. Send a message to begin.".to_string(),
                    reply_to: reply_to.map(|s| s.to_string()),
                }).await;
            }
            "/status" => {
                let status = {
                    let sessions = self.sessions.lock().await;
                    if let Some(session) = sessions.get(&key) {
                        format!(
                            "Session: {}\nCLI: {}\nProfile: {}\nBusy: {}\nQueue: {}",
                            session.cli_session_id.as_deref().unwrap_or("(not yet)"),
                            session.cli_kind.as_deref().unwrap_or("(not yet)"),
                            session.profile,
                            session.busy,
                            session.queue.len(),
                        )
                    } else {
                        "No active session.".to_string()
                    }
                };
                self.channel_hub().send_notification(ChannelNotification::SendText {
                    channel_kind: channel_kind.to_string(),
                    chat_id: chat_id.to_string(),
                    text: status,
                    reply_to: reply_to.map(|s| s.to_string()),
                }).await;
            }
            "/agent" => {
                let requested = parts.get(1).copied().unwrap_or("").trim();
                let Some(kind) = crate::agent::AgentKind::from_str_loose(requested) else {
                    self.channel_hub().send_notification(ChannelNotification::SendText {
                        channel_kind: channel_kind.to_string(),
                        chat_id: chat_id.to_string(),
                        text: format!("Unknown agent: {}", requested),
                        reply_to: reply_to.map(|s| s.to_string()),
                    }).await;
                    return;
                };
                if !kind.is_enabled() {
                    self.channel_hub().send_notification(ChannelNotification::SendText {
                        channel_kind: channel_kind.to_string(),
                        chat_id: chat_id.to_string(),
                        text: format!("Agent is disabled: {}", kind),
                        reply_to: reply_to.map(|s| s.to_string()),
                    }).await;
                    return;
                }

                {
                    let mut sessions = self.sessions.lock().await;
                    let session = sessions
                        .entry(key.clone())
                        .or_insert_with(|| Session::new(channel_kind.to_string(), chat_id.to_string()));
                    session.cli_kind = Some(kind.to_string());
                    session.cli_session_id = None;
                }
                self.agent_hub().kill_chat_agents(channel_kind, chat_id).await;
                self.channel_hub().send_notification(ChannelNotification::SendText {
                    channel_kind: channel_kind.to_string(),
                    chat_id: chat_id.to_string(),
                    text: format!("Switched agent to {}.", kind),
                    reply_to: reply_to.map(|s| s.to_string()),
                }).await;
            }
            _ => {
                eprintln!("[SessionHub][{}] unknown command '{}'", key, cmd);
            }
        }
    }
}
