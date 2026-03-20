//! AgentHub: agent process lifecycle, profile loading, CLI spawn/kill.
//!
//! Responsibilities:
//! - Spawn agent CLI processes (claude, gemini, etc.)
//! - Maintain its own agent process table (keyed by channel:chat:profile:cli)
//! - Load agent profiles from ~/.vibearound/agents/<profile>/profile/
//! - Forward messages to agents and stream replies back to SessionHub
//! - Kill agents on session reset
//!
//! Agent key format: "{channel_kind}:{chat_id}:{profile}:{cli_kind}"
//! e.g. "feishu:oc_0001:default:claude"
//!
//! All agents share the same workspace: ~/.vibearound/workspaces/

use std::sync::Arc;

use dashmap::DashMap;
use tokio::sync::{broadcast, OnceCell};

use crate::agent::{self, AgentBackend, AgentEvent, AgentKind};
use crate::config::{self, ImVerboseConfig};
use crate::hub::session_hub::SessionHub;
use crate::hub::types::*;

// ---------------------------------------------------------------------------
// Agent process entry
// ---------------------------------------------------------------------------

/// A running agent process managed by AgentHub.
struct AgentProcess {
    backend: Box<dyn AgentBackend>,
    cli_session_id: Option<String>,
}

/// Build the agent key: "{channel}:{chat_id}:{profile}:{cli_kind}".
fn agent_key(channel_kind: &str, chat_id: &str, profile: &str, cli_kind: &str) -> String {
    format!("{}:{}:{}:{}", channel_kind, chat_id, profile, cli_kind)
}

// ---------------------------------------------------------------------------
// AgentHub
// ---------------------------------------------------------------------------

pub struct AgentHub {
    /// Agent process table: key → running process.
    agents: DashMap<String, AgentProcess>,
    /// Back-reference to SessionHub (set after init).
    session_hub: OnceCell<Arc<SessionHub>>,
    /// Hub event broadcaster (subscribed by ServerDaemon).
    hub_tx: broadcast::Sender<HubEvent>,
}

impl AgentHub {
    pub fn new() -> Self {
        let (hub_tx, _) = broadcast::channel(64);
        Self {
            agents: DashMap::new(),
            session_hub: OnceCell::new(),
            hub_tx,
        }
    }

    /// Subscribe to hub lifecycle events.
    pub fn subscribe(&self) -> broadcast::Receiver<HubEvent> {
        self.hub_tx.subscribe()
    }

    /// Set the SessionHub reference (two-phase init).
    pub fn set_session_hub(&self, hub: Arc<SessionHub>) {
        let _ = self.session_hub.set(hub);
    }

    fn session_hub(&self) -> &Arc<SessionHub> {
        self.session_hub.get().expect("SessionHub not initialized")
    }

    // -----------------------------------------------------------------------
    // Dispatch: receive message from SessionHub, send to agent
    // -----------------------------------------------------------------------

    /// Called by SessionHub to dispatch a message to an agent.
    /// Spawns a tokio task that streams agent events back to SessionHub.
    pub fn dispatch(self: &Arc<Self>, msg: InboundMessage, verbose: ImVerboseConfig) {
        let this = Arc::clone(self);
        tokio::spawn(async move {
            this.dispatch_inner(msg, verbose).await;
        });
    }

    /// Inner dispatch logic (runs inside a spawned task).
    async fn dispatch_inner(&self, msg: InboundMessage, verbose: ImVerboseConfig) {
        let cfg = config::ensure_loaded();
        let cli_kind_str = &cfg.default_agent;
        let kind = AgentKind::from_str_loose(cli_kind_str).unwrap_or(AgentKind::Claude);
        let profile = "default";
        let key = agent_key(&msg.channel_kind, &msg.chat_id, profile, cli_kind_str);
        let pfx = format!("[AgentHub][{}]", key);

        // Ensure agent is running
        if let Err(e) = self.ensure_agent(&key, kind, profile).await {
            eprintln!("{} failed to ensure agent: {}", pfx, e);
            self.session_hub().on_reply(AgentReply {
                channel_kind: msg.channel_kind,
                chat_id: msg.chat_id,
                message_id: msg.message_id,
                session_id: String::new(),
                event: AgentReplyEvent::Error { error: format!("Failed to start agent: {}", e) },
            }).await;
            return;
        }

        eprintln!("{} → text={}", pfx, truncate(&msg.text, 80));

        // Subscribe to event stream and fire message
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
                self.session_hub().on_reply(AgentReply {
                    channel_kind: msg.channel_kind,
                    chat_id: msg.chat_id,
                    message_id: msg.message_id,
                    session_id: String::new(),
                    event: AgentReplyEvent::Error { error: e },
                }).await;
                return;
            }
            rx
        }; // DashMap Ref dropped here

        // Send agent_start
        let channel_kind = msg.channel_kind.clone();
        let chat_id = msg.chat_id.clone();
        let message_id = msg.message_id.clone();

        self.session_hub().on_reply(AgentReply {
            channel_kind: channel_kind.clone(),
            chat_id: chat_id.clone(),
            message_id: message_id.clone(),
            session_id: String::new(),
            event: AgentReplyEvent::Start,
        }).await;

        // Forward agent events
        let cli_kind_owned = cli_kind_str.to_string();
        let profile_owned = profile.to_string();
        let key_clone = key.clone();

        loop {
            match rx.recv().await {
                Ok(event) => {
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
                        AgentEvent::TurnComplete { session_id, .. } => {
                            // Update cli_session_id in our process table
                            if let Some(sid) = session_id {
                                if let Some(mut entry) = self.agents.get_mut(&key_clone) {
                                    if entry.cli_session_id.as_ref() != Some(sid) {
                                        eprintln!("{} cli_session_id updated: {:?} → {}", pfx, entry.cli_session_id, sid);
                                        entry.cli_session_id = Some(sid.clone());
                                    }
                                }
                            }
                            Some(AgentReplyEvent::Complete {
                                cli_session_id: session_id.clone(),
                                cli_kind: cli_kind_owned.clone(),
                                profile: profile_owned.clone(),
                            })
                        }
                        AgentEvent::Error(e) => Some(AgentReplyEvent::Error { error: e.clone() }),
                        _ => None,
                    };

                    if let Some(re) = reply_event {
                        let is_complete = matches!(re, AgentReplyEvent::Complete { .. });
                        self.session_hub().on_reply(AgentReply {
                            channel_kind: channel_kind.clone(),
                            chat_id: chat_id.clone(),
                            message_id: message_id.clone(),
                            session_id: String::new(),
                            event: re,
                        }).await;
                        if is_complete {
                            break;
                        }
                    }
                }
                Err(broadcast::error::RecvError::Lagged(n)) => {
                    eprintln!("{} event stream lagged by {} events", pfx, n);
                }
                Err(broadcast::error::RecvError::Closed) => {
                    eprintln!("{} event stream closed", pfx);
                    self.session_hub().on_reply(AgentReply {
                        channel_kind: channel_kind.clone(),
                        chat_id: chat_id.clone(),
                        message_id: message_id.clone(),
                        session_id: String::new(),
                        event: AgentReplyEvent::Complete {
                            cli_session_id: None,
                            cli_kind: cli_kind_owned.clone(),
                            profile: profile_owned.clone(),
                        },
                    }).await;
                    break;
                }
            }
        }

        eprintln!("{} agent turn complete", pfx);
    }

    // -----------------------------------------------------------------------
    // Agent lifecycle
    // -----------------------------------------------------------------------

    /// Ensure an agent process is running for the given key.
    async fn ensure_agent(
        &self,
        key: &str,
        kind: AgentKind,
        profile: &str,
    ) -> Result<(), String> {
        // Already running?
        if self.agents.contains_key(key) {
            return Ok(());
        }

        // Shared workspace for all agents
        let workspace = config::data_dir().join("workspaces");
        if !workspace.exists() {
            std::fs::create_dir_all(&workspace)
                .map_err(|e| format!("Failed to create workspace {:?}: {}", workspace, e))?;
        }

        // Write MCP config
        let port = config::DEFAULT_PORT;
        crate::agent::manager_prompt::ensure_mcp_config(kind, &workspace, port);

        // Load system prompt from profile
        let system_prompt = load_agent_profile(profile)
            .or_else(|| Some(crate::agent::manager_prompt::load_manager_prompt()));

        // Create and start backend
        let mut backend = agent::create_backend(kind);
        backend.start(&workspace, system_prompt.as_deref()).await?;

        eprintln!("[AgentHub] spawned agent: {}", key);

        self.agents.insert(key.to_string(), AgentProcess {
            backend,
            cli_session_id: None,
        });

        let _ = self.hub_tx.send(HubEvent::OnAgentSpawned {
            key: key.to_string(),
            kind: kind.to_string(),
        });

        Ok(())
    }

    /// Kill an agent by its key.
    pub async fn kill_agent(&self, key: &str) {
        if let Some((_, mut process)) = self.agents.remove(key) {
            process.backend.shutdown().await;
            let _ = self.hub_tx.send(HubEvent::OnAgentKilled { key: key.to_string() });
            eprintln!("[AgentHub] killed agent: {}", key);
        }
    }

    /// Kill all agents for a given channel + chat_id.
    pub async fn kill_chat_agents(&self, channel_kind: &str, chat_id: &str) {
        let prefix = format!("{}:{}:", channel_kind, chat_id);
        let keys: Vec<String> = self.agents.iter()
            .filter(|e| e.key().starts_with(&prefix))
            .map(|e| e.key().clone())
            .collect();
        for key in keys {
            self.kill_agent(&key).await;
        }
    }
}

// ---------------------------------------------------------------------------
// Profile loading
// ---------------------------------------------------------------------------

/// Load agent profile from ~/.vibearound/agents/<profile>/profile/*.md
fn load_agent_profile(profile: &str) -> Option<String> {
    let profile_dir = config::data_dir().join("agents").join(profile).join("profile");
    if !profile_dir.exists() {
        return None;
    }

    let prompt_files = &[
        "identity.md",
        "capabilities.md",
        "workflow.md",
        "memory.md",
        "rules.md",
    ];

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
