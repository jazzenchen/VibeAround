//! SessionHub: session lifecycle, per-session message buffering, and event fan-out.
//!
//! Responsibilities:
//! - Own session state keyed by channel_kind + chat_id
//! - Buffer inbound messages per session and advance the active turn when idle
//! - Expose request-style APIs for ChannelManager and AgentManager
//! - Publish session events for ChannelManager and AgentManager subscribers
//! - Provide lightweight read-only session runtime queries

pub mod types;

use std::collections::{HashMap, VecDeque};
use tokio::sync::{broadcast, Mutex};

use crate::session_hub::types::*;

/// Unique key for a session: "{channel_kind}:{chat_id}".
fn session_key(channel_kind: &str, chat_id: &str) -> String {
    format!("{}:{}", channel_kind, chat_id)
}

/// Per-session state.
struct Session {
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
    fn new() -> Self {
        Self {
            cli_session_id: None,
            cli_kind: None,
            profile: "default".to_string(),
            busy: false,
            queue: VecDeque::new(),
        }
    }
}

pub struct SessionHub {
    /// Sessions keyed by "{channel_kind}:{chat_id}".
    sessions: Mutex<HashMap<String, Session>>,
    /// Broadcast channel for SessionHub -> ChannelManager events.
    channel_event_tx: broadcast::Sender<ChannelEvent>,
    /// Broadcast channel for SessionHub -> AgentManager events.
    agent_event_tx: broadcast::Sender<AgentEvent>,
}

impl SessionHub {
    pub fn new() -> Self {
        let (channel_event_tx, _) = broadcast::channel(128);
        let (agent_event_tx, _) = broadcast::channel(128);
        Self {
            sessions: Mutex::new(HashMap::new()),
            channel_event_tx,
            agent_event_tx,
        }
    }

    pub fn subscribe_channel_events(&self) -> broadcast::Receiver<ChannelEvent> {
        self.channel_event_tx.subscribe()
    }

    pub fn subscribe_agent_events(&self) -> broadcast::Receiver<AgentEvent> {
        self.agent_event_tx.subscribe()
    }

    fn publish_channel_event(&self, event: ChannelEvent) {
        let _ = self.channel_event_tx.send(event);
    }

    pub fn publish_agent_event(&self, event: AgentEvent) {
        let _ = self.agent_event_tx.send(event);
    }

    pub async fn get_session_cli_kind(&self, channel_kind: &str, chat_id: &str) -> Option<String> {
        let key = session_key(channel_kind, chat_id);
        let sessions = self.sessions.lock().await;
        sessions.get(&key).and_then(|session| session.cli_kind.clone())
    }

    pub async fn get_session_profile(&self, channel_kind: &str, chat_id: &str) -> Option<String> {
        let key = session_key(channel_kind, chat_id);
        let sessions = self.sessions.lock().await;
        sessions.get(&key).map(|session| session.profile.clone())
    }

    /// Called by ChannelManager when a message arrives from a channel plugin.
    pub async fn channel_request_message(&self, msg: InboundMessage) {
        let key = session_key(&msg.channel_kind, &msg.chat_id);
        let pfx = format!("[SessionHub][{}]", key);

        {
            let mut sessions = self.sessions.lock().await;

            if !sessions.contains_key(&key) {
                eprintln!("{} creating new session", pfx);
                sessions.insert(key.clone(), Session::new());
            }

            let session = sessions.get_mut(&key).unwrap();
            session.queue.push_back(QueuedMessage {
                message: msg.clone(),
                status: MessageStatus::Unreplied,
            });

            eprintln!("{} enqueued msg_id={} queue_len={}", pfx, msg.message_id, session.queue.len());

            if session.busy {
                eprintln!("{} agent busy, message queued", pfx);
            }
        }

        self.try_advance_session_queue(&key).await;
    }

    /// Called by AgentManager when an agent session becomes usable.
    pub async fn agent_session_id_ready(&self, ready: AgentReady) {
        let key = session_key(&ready.channel_kind, &ready.chat_id);
        let first_ready = {
            let mut sessions = self.sessions.lock().await;
            if let Some(session) = sessions.get_mut(&key) {
                let first_ready = session.cli_session_id.is_none();
                session.cli_session_id = Some(ready.cli_session_id.clone());
                session.cli_kind = Some(ready.cli_kind.clone());
                session.profile = ready.profile.clone();
                first_ready
            } else {
                false
            }
        };

        if first_ready {
            self.publish_channel_event(ChannelEvent::OnAgentSessionReady {
                channel_kind: ready.channel_kind,
                chat_id: ready.chat_id,
                message_id: ready.message_id,
                cli_kind: ready.cli_kind,
                cli_session_id: ready.cli_session_id,
                profile: ready.profile,
            });
        }
    }

    /// Called by AgentManager when an agent starts processing a turn.
    pub async fn agent_started(&self, reply: AgentReply) {
        self.agent_acp_event(reply).await;
    }

    /// Called by AgentManager when an agent event arrives.
    pub async fn agent_acp_event(&self, reply: AgentReply) {
        match &reply.event {
            AgentReplyEvent::Start => {
                self.publish_channel_event(ChannelEvent::OnTurnStarted {
                    channel_kind: reply.channel_kind.clone(),
                    chat_id: reply.chat_id.clone(),
                    message_id: reply.message_id.clone(),
                });
            }
            AgentReplyEvent::Token { delta } => {
                self.publish_channel_event(ChannelEvent::OnAcpEvent {
                    channel_kind: reply.channel_kind.clone(),
                    chat_id: reply.chat_id.clone(),
                    message_id: reply.message_id.clone(),
                    payload: serde_json::json!({ "kind": "token", "delta": delta }),
                });
            }
            AgentReplyEvent::Thinking { text } => {
                self.publish_channel_event(ChannelEvent::OnAcpEvent {
                    channel_kind: reply.channel_kind.clone(),
                    chat_id: reply.chat_id.clone(),
                    message_id: reply.message_id.clone(),
                    payload: serde_json::json!({ "kind": "thinking", "text": text }),
                });
            }
            AgentReplyEvent::ToolUse { tool, input } => {
                self.publish_channel_event(ChannelEvent::OnAcpEvent {
                    channel_kind: reply.channel_kind.clone(),
                    chat_id: reply.chat_id.clone(),
                    message_id: reply.message_id.clone(),
                    payload: serde_json::json!({ "kind": "tool_use", "tool": tool, "input": input }),
                });
            }
            AgentReplyEvent::ToolResult { tool, output } => {
                self.publish_channel_event(ChannelEvent::OnAcpEvent {
                    channel_kind: reply.channel_kind.clone(),
                    chat_id: reply.chat_id.clone(),
                    message_id: reply.message_id.clone(),
                    payload: serde_json::json!({ "kind": "tool_result", "tool": tool, "output": output }),
                });
            }
            AgentReplyEvent::Error { error } => {
                self.publish_channel_event(ChannelEvent::OnSessionError {
                    channel_kind: reply.channel_kind.clone(),
                    chat_id: reply.chat_id.clone(),
                    error: error.clone(),
                });
            }
            AgentReplyEvent::Complete => {
                self.publish_channel_event(ChannelEvent::OnTurnCompleted {
                    channel_kind: reply.channel_kind.clone(),
                    chat_id: reply.chat_id.clone(),
                });
            }
        }
    }

    /// Called by AgentManager when the current turn is complete.
    pub async fn agent_turn_completed(&self, channel_kind: &str, chat_id: &str) {
        let session_key = session_key(channel_kind, chat_id);
        {
            let mut sessions = self.sessions.lock().await;
            if let Some(session) = sessions.get_mut(&session_key) {
                if let Some(front) = session.queue.front() {
                    if front.status == MessageStatus::Processing {
                        session.queue.pop_front();
                    }
                }
                session.busy = false;
            }
        }

        self.try_advance_session_queue(&session_key).await;
    }

    /// Called by AgentManager when a turn fails.
    pub async fn agent_failed(&self, reply: AgentReply) {
        self.agent_acp_event(reply).await;
    }

    /// Called by AgentManager when an agent session closes.
    pub async fn agent_stopped(&self, closed: AgentClosed) {
        eprintln!(
            "[SessionHub][{}] agent closed reason={} cli_kind={:?} cli_session_id={:?} profile={:?}",
            session_key(&closed.channel_kind, &closed.chat_id),
            closed.reason,
            closed.cli_kind,
            closed.cli_session_id,
            closed.profile,
        );
    }

    /// Requested by ChannelManager to stop the current runtime for a route.
    pub async fn channel_request_stop(&self, channel_kind: &str, chat_id: &str) {
        let key = session_key(channel_kind, chat_id);
        let had_session = {
            let sessions = self.sessions.lock().await;
            sessions.contains_key(&key)
        };

        self.publish_agent_event(AgentEvent::OnStopRuntime {
            channel_kind: channel_kind.to_string(),
            chat_id: chat_id.to_string(),
        });

        {
            let mut sessions = self.sessions.lock().await;
            if let Some(session) = sessions.get_mut(&key) {
                session.busy = false;
                for queued in session.queue.iter_mut() {
                    queued.status = MessageStatus::Unreplied;
                }
            }
        }

        eprintln!("[SessionHub][{}] channel requested stop handled had_session={}", key, had_session);
    }

    /// Requested by ChannelManager to close the current route.
    pub async fn channel_request_close(&self, channel_kind: &str, chat_id: &str) {
        let key = session_key(channel_kind, chat_id);
        self.publish_agent_event(AgentEvent::OnStopRuntime {
            channel_kind: channel_kind.to_string(),
            chat_id: chat_id.to_string(),
        });

        let removed = {
            let mut sessions = self.sessions.lock().await;
            sessions.remove(&key).is_some()
        };

        eprintln!("[SessionHub][{}] channel requested close handled removed_session={}", key, removed);
    }

    /// Requested by ChannelManager to switch runtime kind for a route.
    pub async fn channel_request_switch_agent_kind(
        &self,
        channel_kind: &str,
        chat_id: &str,
        agent_kind: &str,
    ) {
        let key = session_key(channel_kind, chat_id);
        let Some(kind) = crate::agent::AgentKind::from_str_loose(agent_kind) else {
            self.publish_channel_event(ChannelEvent::OnSystemText {
                channel_kind: channel_kind.to_string(),
                chat_id: chat_id.to_string(),
                text: format!("Unknown agent: {}", agent_kind),
                reply_to: None,
            });
            return;
        };

        if !kind.is_enabled() {
            self.publish_channel_event(ChannelEvent::OnSystemText {
                channel_kind: channel_kind.to_string(),
                chat_id: chat_id.to_string(),
                text: format!("Agent is disabled: {}", kind),
                reply_to: None,
            });
            return;
        }

        self.publish_agent_event(AgentEvent::OnStopRuntime {
            channel_kind: channel_kind.to_string(),
            chat_id: chat_id.to_string(),
        });

        {
            let mut sessions = self.sessions.lock().await;
            let session = sessions.entry(key.clone()).or_insert_with(Session::new);
            session.cli_kind = Some(kind.to_string());
            session.cli_session_id = None;
            session.busy = false;
        }

        self.publish_channel_event(ChannelEvent::OnSystemText {
            channel_kind: channel_kind.to_string(),
            chat_id: chat_id.to_string(),
            text: format!("Switched agent to {}.", kind),
            reply_to: None,
        });

        eprintln!(
            "[SessionHub][{}] channel requested switch_agent_kind handled new_kind={}",
            key, kind,
        );
    }

    async fn try_advance_session_queue(&self, key: &str) {
        let dispatch_msg = {
            let mut sessions = self.sessions.lock().await;
            let Some(session) = sessions.get_mut(key) else {
                return;
            };

            if session.busy {
                return;
            }

            let Some(front) = session.queue.front_mut() else {
                return;
            };

            front.status = MessageStatus::Processing;
            session.busy = true;
            front.message.clone()
        };

        self.publish_agent_event(AgentEvent::OnReceiveMessage {
            channel_kind: dispatch_msg.channel_kind.clone(),
            chat_id: dispatch_msg.chat_id.clone(),
            message: dispatch_msg,
        });
    }
}
