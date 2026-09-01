use crate::previews::PreviewsResponse;
use crate::runtime::{AgentRuntime, AgentsConfig, ChannelRuntime, TunnelRuntime};
use crate::service::ServiceInfoResponse;
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

#[cfg(test)]
mod tests {
    use crate::runtime::{ChannelRuntime, ChannelStatus};

    use super::*;

    #[test]
    fn runtime_snapshot_counts_channel_state() {
        let mut snapshot = RuntimeSnapshot::new();
        snapshot.apply_channels(vec![
            ChannelRuntime {
                instance_id: "feishu".into(),
                kind: "feishu".into(),
                version: None,
                plugin_dir: None,
                status: ChannelStatus::Running,
                reason: None,
            },
            ChannelRuntime {
                instance_id: "telegram".into(),
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
}
