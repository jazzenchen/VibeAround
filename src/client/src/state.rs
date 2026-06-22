use crate::events::ChatEvent;
use crate::previews::PreviewsResponse;
use crate::runtime::{AgentRuntime, AgentsConfig, ChannelRuntime, TunnelRuntime};
use crate::service::ServiceInfoResponse;
use crate::sessions::SessionListItem;
use crate::workspaces::WorkspacesResponse;

/// Client-side runtime view assembled from independent server responses.
///
/// This is intentionally display-oriented, not a source of truth. Hosts can
/// keep one snapshot, feed decoded HTTP/WS results into it, and render CLI/TUI
/// views without duplicating merge logic in every surface.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct RuntimeSnapshot {
    pub service: Option<ServiceInfoResponse>,
    pub agents: Option<AgentsConfig>,
    pub channels: Vec<ChannelRuntime>,
    pub tunnels: Vec<TunnelRuntime>,
    pub agent_runtimes: Vec<AgentRuntime>,
    pub sessions: Vec<SessionListItem>,
    pub workspaces: Option<WorkspacesResponse>,
    pub previews: Option<PreviewsResponse>,
}

impl RuntimeSnapshot {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn apply_service_info(&mut self, service: ServiceInfoResponse) {
        self.service = Some(service);
    }

    pub fn apply_agents(&mut self, agents: AgentsConfig) {
        self.agents = Some(agents);
    }

    pub fn apply_channels(&mut self, channels: Vec<ChannelRuntime>) {
        self.channels = channels;
    }

    pub fn apply_tunnels(&mut self, tunnels: Vec<TunnelRuntime>) {
        self.tunnels = tunnels;
    }

    pub fn apply_agent_runtimes(&mut self, agent_runtimes: Vec<AgentRuntime>) {
        self.agent_runtimes = agent_runtimes;
    }

    pub fn apply_sessions(&mut self, sessions: Vec<SessionListItem>) {
        self.sessions = sessions;
    }

    pub fn apply_workspaces(&mut self, workspaces: WorkspacesResponse) {
        self.workspaces = Some(workspaces);
    }

    pub fn apply_previews(&mut self, previews: PreviewsResponse) {
        self.previews = Some(previews);
    }

    pub fn running_channels(&self) -> usize {
        self.channels
            .iter()
            .filter(|channel| matches!(channel.status, crate::runtime::ChannelStatus::Running))
            .count()
    }

    pub fn failed_channels(&self) -> usize {
        self.channels
            .iter()
            .filter(|channel| matches!(channel.status, crate::runtime::ChannelStatus::Crashed))
            .count()
    }

    pub fn active_agents(&self) -> usize {
        self.agent_runtimes
            .iter()
            .filter(|agent| agent.failed.is_none())
            .count()
    }

    pub fn busy_agents(&self) -> usize {
        self.agent_runtimes
            .iter()
            .filter(|agent| agent.busy)
            .count()
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct ChatState {
    pub channel_id: Option<String>,
    pub agents: Vec<crate::runtime::AgentInfo>,
    pub default_agent: Option<String>,
    pub current_agent: Option<ChatAgentInfo>,
    pub session_id: Option<String>,
    pub turn_active: bool,
    pub pending_permission_request_id: Option<String>,
    pub last_error: Option<String>,
    pub system_messages: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChatAgentInfo {
    pub agent: String,
    pub version: String,
}

impl ChatState {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn apply_event(&mut self, event: ChatEvent) {
        match event {
            ChatEvent::Config {
                channel_id,
                agents,
                default_agent,
            } => {
                self.channel_id = Some(channel_id);
                self.agents = agents;
                self.default_agent = Some(default_agent);
            }
            ChatEvent::AgentReady { agent, version } => {
                self.current_agent = Some(ChatAgentInfo { agent, version });
            }
            ChatEvent::SessionReady { session_id } => {
                self.session_id = Some(session_id);
            }
            ChatEvent::PermissionRequest { request_id, .. } => {
                self.pending_permission_request_id = Some(request_id);
            }
            ChatEvent::PromptDone { .. } => {
                self.turn_active = false;
                self.pending_permission_request_id = None;
            }
            ChatEvent::TurnStatus { active } => {
                self.turn_active = active;
            }
            ChatEvent::SystemText { text } => {
                self.system_messages.push(text);
            }
            ChatEvent::Error { error } => {
                self.last_error = Some(error);
                self.turn_active = false;
            }
            ChatEvent::SessionMode { .. }
            | ChatEvent::CommandMenu { .. }
            | ChatEvent::MultiAgentTurn { .. }
            | ChatEvent::SubagentStatus { .. }
            | ChatEvent::SubagentAcpNotification { .. }
            | ChatEvent::AcpNotification { .. } => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::events::ChatEvent;
    use crate::runtime::{AgentInfo, ChannelRuntime, ChannelStatus};

    use super::*;

    #[test]
    fn runtime_snapshot_counts_channel_state() {
        let mut snapshot = RuntimeSnapshot::new();
        snapshot.apply_channels(vec![
            ChannelRuntime {
                kind: "feishu".into(),
                version: None,
                plugin_dir: None,
                status: ChannelStatus::Running,
                reason: None,
            },
            ChannelRuntime {
                kind: "telegram".into(),
                version: None,
                plugin_dir: None,
                status: ChannelStatus::Crashed,
                reason: Some("missing token".into()),
            },
        ]);

        assert_eq!(snapshot.running_channels(), 1);
        assert_eq!(snapshot.failed_channels(), 1);
    }

    #[test]
    fn chat_state_reduces_lifecycle_events() {
        let mut state = ChatState::new();
        state.apply_event(ChatEvent::Config {
            channel_id: "web:1".into(),
            agents: vec![AgentInfo {
                id: "codex".into(),
                name: "Codex".into(),
                description: "Coding agent".into(),
            }],
            default_agent: "codex".into(),
        });
        state.apply_event(ChatEvent::AgentReady {
            agent: "Codex".into(),
            version: "1.0".into(),
        });
        state.apply_event(ChatEvent::SessionReady {
            session_id: "s1".into(),
        });
        state.apply_event(ChatEvent::TurnStatus { active: true });
        state.apply_event(ChatEvent::PromptDone { message_id: None });

        assert_eq!(state.channel_id.as_deref(), Some("web:1"));
        assert_eq!(state.default_agent.as_deref(), Some("codex"));
        assert_eq!(state.current_agent.as_ref().unwrap().agent, "Codex");
        assert_eq!(state.session_id.as_deref(), Some("s1"));
        assert!(!state.turn_active);
    }
}
