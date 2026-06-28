use va_client::runtime::{
    AgentRuntime, ChannelRuntime, ChannelStatus, TunnelRuntime, TunnelStatus,
};
use va_client::sessions::{PtyRunState, SessionListItem};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DetailContent {
    pub(crate) title: String,
    pub(crate) lines: Vec<String>,
}

pub(crate) fn channel_status_label(status: ChannelStatus) -> &'static str {
    match status {
        ChannelStatus::NotStarted => "not-started",
        ChannelStatus::Spawning => "spawning",
        ChannelStatus::Running => "running",
        ChannelStatus::Crashed => "crashed",
        ChannelStatus::Stopped => "stopped",
    }
}

pub(crate) fn tunnel_status_label(status: &TunnelStatus) -> &'static str {
    match status {
        TunnelStatus::Running => "running",
        TunnelStatus::Stopped { .. } => "stopped",
        TunnelStatus::Failed { .. } => "failed",
    }
}

pub(crate) fn session_status_label(status: &PtyRunState) -> &'static str {
    match status {
        PtyRunState::Running { .. } => "running",
        PtyRunState::Exited { .. } => "exited",
    }
}

pub(crate) fn channel_detail(channel: &ChannelRuntime) -> DetailContent {
    DetailContent {
        title: format!("channel {}", channel.kind),
        lines: vec![
            format!("kind: {}", channel.kind),
            format!("status: {}", channel_status_label(channel.status)),
            format!("version: {}", channel.version.as_deref().unwrap_or("-")),
            format!(
                "plugin_dir: {}",
                channel.plugin_dir.as_deref().unwrap_or("-")
            ),
            format!("reason: {}", channel.reason.as_deref().unwrap_or("-")),
        ],
    }
}

pub(crate) fn tunnel_detail(tunnel: &TunnelRuntime) -> DetailContent {
    DetailContent {
        title: format!("tunnel {}", tunnel.provider),
        lines: vec![
            format!("provider: {}", tunnel.provider),
            format!("status: {}", tunnel_status_label(&tunnel.status)),
            format!("url: {}", tunnel.url.as_deref().unwrap_or("-")),
            format!("uptime_secs: {}", tunnel.uptime_secs),
        ],
    }
}

pub(crate) fn agent_detail(agent: &AgentRuntime) -> DetailContent {
    DetailContent {
        title: format!("agent {}", agent.route_key),
        lines: vec![
            format!("route_key: {}", agent.route_key),
            format!("channel_kind: {}", agent.channel_kind),
            format!("chat_id: {}", agent.chat_id),
            format!("cli_kind: {}", agent.cli_kind.as_deref().unwrap_or("-")),
            format!("profile: {}", agent.profile.as_deref().unwrap_or("-")),
            format!("session_id: {}", agent.session_id.as_deref().unwrap_or("-")),
            format!("workspace: {}", agent.workspace.as_deref().unwrap_or("-")),
            format!("busy: {}", agent.busy),
            format!("failed: {}", agent.failed.as_deref().unwrap_or("-")),
        ],
    }
}

pub(crate) fn session_detail(session: &SessionListItem) -> DetailContent {
    DetailContent {
        title: format!("session {}", short_id(&session.session_id)),
        lines: vec![
            format!("session_id: {}", session.session_id),
            format!("tool: {:?}", session.tool),
            format!("status: {}", session_status_label(&session.status)),
            format!(
                "project_path: {}",
                session.project_path.as_deref().unwrap_or("-")
            ),
            format!(
                "profile_id: {}",
                session.profile_id.as_deref().unwrap_or("-")
            ),
            format!(
                "profile_label: {}",
                session.profile_label.as_deref().unwrap_or("-")
            ),
            format!(
                "launch_target: {}",
                session.launch_target.as_deref().unwrap_or("-")
            ),
            format!(
                "tmux_session: {}",
                session.tmux_session.as_deref().unwrap_or("-")
            ),
        ],
    }
}

fn short_id(value: &str) -> String {
    if value.chars().count() <= 8 {
        value.to_string()
    } else {
        value.chars().take(8).collect()
    }
}
