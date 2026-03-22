//! AgentManager: agent process lifecycle, profile loading, CLI spawn/kill.
//!
//! Responsibilities:
//! - Spawn agent CLI processes (claude, gemini, etc.)
//! - Maintain its own agent process table (keyed by channel:chat:profile:cli)
//! - Load agent profiles from ~/.vibearound/agents/<profile>/profile/
//! - Forward messages to agents and stream replies back to SessionHub
//! - Kill agents on session reset

use std::sync::Arc;

use dashmap::DashMap;
use tokio::sync::{broadcast, OnceCell};

use crate::agent::{self, AgentBackend, AgentEvent, AgentKind};
use crate::config::{self, ImVerboseConfig};
use crate::session_hub::types::*;
use crate::session_hub::SessionHub;

struct AgentProcess {
    backend: Box<dyn AgentBackend>,
    cli_session_id: Option<String>,
}

fn agent_key(channel_kind: &str, chat_id: &str, profile: &str, cli_kind: &str) -> String {
    format!("{}:{}:{}:{}", channel_kind, chat_id, profile, cli_kind)
}

pub struct AgentManager {
    agents: DashMap<String, AgentProcess>,
    session_hub: OnceCell<Arc<SessionHub>>,
    hub_tx: broadcast::Sender<HubEvent>,
}

impl AgentManager {
    pub fn new() -> Self {
        let (hub_tx, _) = broadcast::channel(64);
        Self {
            agents: DashMap::new(),
            session_hub: OnceCell::new(),
            hub_tx,
        }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<HubEvent> {
        self.hub_tx.subscribe()
    }

    pub fn set_session_hub(self: &Arc<Self>, hub: Arc<SessionHub>) {
        let _ = self.session_hub.set(Arc::clone(&hub));
        self.spawn_agent_event_bridge(hub);
    }

    fn session_hub(&self) -> &Arc<SessionHub> {
        self.session_hub.get().expect("SessionHub not initialized")
    }

    fn spawn_agent_event_bridge(self: &Arc<Self>, session_hub: Arc<SessionHub>) {
        let this = Arc::clone(self);
        let mut rx = session_hub.subscribe_agent_events();
        tokio::spawn(async move {
            loop {
                match rx.recv().await {
                    Ok(event) => this.handle_agent_event(event).await,
                    Err(broadcast::error::RecvError::Lagged(n)) => {
                        eprintln!("[AgentManager] agent event stream lagged by {} events", n);
                    }
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }
        });
    }

    async fn handle_agent_event(self: &Arc<Self>, event: crate::session_hub::types::AgentEvent) {
        match event {
            crate::session_hub::types::AgentEvent::OnReceiveMessage {
                channel_kind,
                chat_id,
                message,
            } => {
                let verbose = config::ensure_loaded().channel_verbose(&channel_kind);
                self.dispatch(
                    message,
                    verbose,
                    self.get_session_cli_kind(&channel_kind, &chat_id).await,
                    self.get_session_profile(&channel_kind, &chat_id).await,
                );
            }
            crate::session_hub::types::AgentEvent::OnStopRuntime { channel_kind, chat_id }
            | crate::session_hub::types::AgentEvent::OnCloseRuntime { channel_kind, chat_id, .. } => {
                self.kill_chat_agents(&channel_kind, &chat_id).await;
            }
            crate::session_hub::types::AgentEvent::OnStartRuntime { .. } => {}
        }
    }

    pub fn dispatch(
        self: &Arc<Self>,
        msg: InboundMessage,
        verbose: ImVerboseConfig,
        cli_kind: Option<String>,
        profile: Option<String>,
    ) {
        let this = Arc::clone(self);
        tokio::spawn(async move {
            this.dispatch_inner(msg, verbose, cli_kind, profile).await;
        });
    }

    async fn dispatch_inner(
        &self,
        msg: InboundMessage,
        verbose: ImVerboseConfig,
        cli_kind: Option<String>,
        profile: Option<String>,
    ) {
        let cfg = config::ensure_loaded();
        let cli_kind_owned = cli_kind.unwrap_or_else(|| cfg.default_agent.clone());
        let kind = AgentKind::from_str_loose(&cli_kind_owned).unwrap_or(AgentKind::Claude);
        let profile_owned = profile.unwrap_or_else(|| "default".to_string());
        let key = agent_key(&msg.channel_kind, &msg.chat_id, &profile_owned, &cli_kind_owned);
        let pfx = format!("[AgentManager][{}]", key);

        let startup_session_id = match self.ensure_agent(&key, kind, &profile_owned).await {
            Ok(session_id) => session_id,
            Err(e) => {
                eprintln!("{} failed to ensure agent: {}", pfx, e);
                self.session_hub()
                    .agent_acp_event(AgentReply {
                        channel_kind: msg.channel_kind,
                        chat_id: msg.chat_id,
                        message_id: msg.message_id,
                        session_id: String::new(),
                        event: AgentReplyEvent::Error {
                            error: format!("Failed to start agent: {}", e),
                        },
                    })
                    .await;
                return;
            }
        };

        eprintln!("{} → text={}", pfx, truncate(&msg.text, 80));

        let mut rx = {
            let entry = match self.agents.get(&key) {
                Some(e) => e,
                None => {
                    eprintln!("{} agent not found after ensure", pfx);
                    return;
                }
            };
            let rx = entry.backend.subscribe();
            if let Err(e) = entry.backend.send_message_fire(&msg.text).await {
                eprintln!("{} send_message_fire failed: {}", pfx, e);
                self.session_hub()
                    .agent_acp_event(AgentReply {
                        channel_kind: msg.channel_kind,
                        chat_id: msg.chat_id,
                        message_id: msg.message_id,
                        session_id: String::new(),
                        event: AgentReplyEvent::Error { error: e },
                    })
                    .await;
                return;
            }
            rx
        };

        let channel_kind = msg.channel_kind.clone();
        let chat_id = msg.chat_id.clone();
        let message_id = msg.message_id.clone();

        if let Some(session_id) = startup_session_id {
            self.session_hub()
                .agent_session_id_ready(AgentReady {
                    channel_kind: channel_kind.clone(),
                    chat_id: chat_id.clone(),
                    message_id: message_id.clone(),
                    session_id: String::new(),
                    cli_kind: cli_kind_owned.clone(),
                    cli_session_id: session_id,
                    profile: profile_owned.clone(),
                })
                .await;
        }

        self.session_hub()
            .agent_acp_event(AgentReply {
                channel_kind: channel_kind.clone(),
                chat_id: chat_id.clone(),
                message_id: message_id.clone(),
                session_id: String::new(),
                event: AgentReplyEvent::Start,
            })
            .await;

        let key_clone = key.clone();

        loop {
            match rx.recv().await {
                Ok(event) => {
                    if let AgentEvent::TurnComplete {
                        session_id: Some(real_session_id),
                        ..
                    } = &event
                    {
                        let should_notify_ready = if let Some(mut entry) = self.agents.get_mut(&key_clone)
                        {
                            if entry.cli_session_id.is_none() {
                                entry.cli_session_id = Some(real_session_id.clone());
                                true
                            } else {
                                false
                            }
                        } else {
                            false
                        };

                        if should_notify_ready {
                            self.session_hub()
                                .agent_session_id_ready(AgentReady {
                                    channel_kind: channel_kind.clone(),
                                    chat_id: chat_id.clone(),
                                    message_id: message_id.clone(),
                                    session_id: String::new(),
                                    cli_kind: cli_kind_owned.clone(),
                                    cli_session_id: real_session_id.clone(),
                                    profile: profile_owned.clone(),
                                })
                                .await;
                        }
                    }

                    let reply_event = match &event {
                        AgentEvent::Text(t) => Some(AgentReplyEvent::Token { delta: t.clone() }),
                        AgentEvent::Thinking(t) => {
                            if verbose.show_thinking {
                                Some(AgentReplyEvent::Thinking { text: t.clone() })
                            } else {
                                None
                            }
                        }
                        AgentEvent::ToolUse { name, input, .. } => {
                            if verbose.show_tool_use {
                                Some(AgentReplyEvent::ToolUse {
                                    tool: name.clone(),
                                    input: input.as_deref().unwrap_or("").to_string(),
                                })
                            } else {
                                None
                            }
                        }
                        AgentEvent::ToolResult { output, .. } => {
                            if verbose.show_tool_use {
                                Some(AgentReplyEvent::ToolResult {
                                    tool: String::new(),
                                    output: output.as_deref().unwrap_or("").to_string(),
                                })
                            } else {
                                None
                            }
                        }
                        AgentEvent::TurnComplete { .. } => Some(AgentReplyEvent::Complete),
                        AgentEvent::Error(e) => Some(AgentReplyEvent::Error { error: e.clone() }),
                        _ => None,
                    };

                    if let Some(re) = reply_event {
                        let is_complete = matches!(re, AgentReplyEvent::Complete);
                        self.session_hub()
                            .agent_acp_event(AgentReply {
                                channel_kind: channel_kind.clone(),
                                chat_id: chat_id.clone(),
                                message_id: message_id.clone(),
                                session_id: String::new(),
                                event: re,
                            })
                            .await;
                        if is_complete {
                            self.session_hub()
                                .agent_turn_completed(&channel_kind, &chat_id)
                                .await;
                            break;
                        }
                    }
                }
                Err(broadcast::error::RecvError::Lagged(n)) => {
                    eprintln!("{} event stream lagged by {} events", pfx, n);
                }
                Err(broadcast::error::RecvError::Closed) => {
                    eprintln!("{} event stream closed", pfx);
                    self.session_hub()
                        .agent_acp_event(AgentReply {
                            channel_kind: channel_kind.clone(),
                            chat_id: chat_id.clone(),
                            message_id: message_id.clone(),
                            session_id: String::new(),
                            event: AgentReplyEvent::Complete,
                        })
                        .await;
                    self.session_hub()
                        .agent_turn_completed(&channel_kind, &chat_id)
                        .await;
                    self.session_hub()
                        .agent_stopped(AgentClosed {
                            channel_kind: channel_kind.clone(),
                            chat_id: chat_id.clone(),
                            session_id: String::new(),
                            cli_kind: Some(cli_kind_owned.clone()),
                            cli_session_id: self
                                .agents
                                .get(&key_clone)
                                .and_then(|entry| entry.cli_session_id.clone()),
                            profile: Some(profile_owned.clone()),
                            reason: "event_stream_closed".to_string(),
                        })
                        .await;
                    break;
                }
            }
        }

        eprintln!("{} agent turn complete", pfx);
    }

    async fn ensure_agent(
        &self,
        key: &str,
        kind: AgentKind,
        profile: &str,
    ) -> Result<Option<String>, String> {
        if let Some(entry) = self.agents.get(key) {
            return Ok(entry.cli_session_id.clone());
        }

        let workspace = config::data_dir().join("workspaces");
        if !workspace.exists() {
            std::fs::create_dir_all(&workspace)
                .map_err(|e| format!("Failed to create workspace {:?}: {}", workspace, e))?;
        }

        let port = config::DEFAULT_PORT;
        crate::agent::manager_prompt::ensure_mcp_config(kind, &workspace, port);

        let system_prompt =
            load_agent_profile(profile).or_else(|| Some(crate::agent::manager_prompt::load_manager_prompt()));

        let mut backend = agent::create_backend(kind);
        let cli_session_id = backend.start(&workspace, system_prompt.as_deref()).await?;

        eprintln!("[AgentManager] spawned agent: {}", key);

        self.agents.insert(
            key.to_string(),
            AgentProcess {
                backend,
                cli_session_id: cli_session_id.clone(),
            },
        );

        let _ = self.hub_tx.send(HubEvent::OnAgentSpawned {
            key: key.to_string(),
            kind: kind.to_string(),
        });

        Ok(cli_session_id)
    }

    pub async fn kill_agent(&self, key: &str) {
        if let Some((_, mut process)) = self.agents.remove(key) {
            process.backend.shutdown().await;
            let _ = self.hub_tx.send(HubEvent::OnAgentKilled {
                key: key.to_string(),
            });
            eprintln!("[AgentManager] killed agent: {}", key);
        }
    }

    pub async fn kill_chat_agents(&self, channel_kind: &str, chat_id: &str) {
        let prefix = format!("{}:{}:", channel_kind, chat_id);
        let keys: Vec<String> = self
            .agents
            .iter()
            .filter(|e| e.key().starts_with(&prefix))
            .map(|e| e.key().clone())
            .collect();
        for key in keys {
            self.kill_agent(&key).await;
        }
    }

    async fn get_session_cli_kind(&self, channel_kind: &str, chat_id: &str) -> Option<String> {
        self.session_hub().get_session_cli_kind(channel_kind, chat_id).await
    }

    async fn get_session_profile(&self, channel_kind: &str, chat_id: &str) -> Option<String> {
        self.session_hub().get_session_profile(channel_kind, chat_id).await
    }
}

fn load_agent_profile(profile: &str) -> Option<String> {
    let profile_dir = config::data_dir().join("agents").join(profile).join("profile");
    if !profile_dir.exists() {
        return None;
    }

    let prompt_files = &["identity.md", "capabilities.md", "workflow.md", "memory.md", "rules.md"];

    let mut parts = Vec::new();
    for file in prompt_files {
        let path = profile_dir.join(file);
        if let Ok(content) = std::fs::read_to_string(&path) {
            let trimmed = content.trim();
            if !trimmed.is_empty() {
                parts.push(trimmed.to_string());
            }
        }
    }

    if parts.is_empty() {
        None
    } else {
        Some(parts.join("\n\n---\n\n"))
    }
}

fn truncate(s: &str, max: usize) -> &str {
    if s.len() <= max { s } else { &s[..max] }
}
