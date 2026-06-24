use ratatui::style::{Color, Modifier, Style};
use ratatui::text::Span;
use va_client::profiles::ModelProfileSummary;
use va_client::runtime::{
    AgentInfo, AgentRuntime, ChannelRuntime, ChannelStatus, TunnelRuntime, TunnelStatus,
};
use va_client::sessions::{LaunchSessionInfo, PtyRunState, SessionListItem};
use va_client::workspaces::WorkspaceItem;

use crate::detail::{channel_status_label, session_status_label, tunnel_status_label};
use crate::theme::{muted_style, ACTION, ERROR, NEUTRAL, OK, WARN};

pub(super) fn channel_row(channel: &ChannelRuntime) -> Vec<Span<'static>> {
    let mut spans = vec![
        Span::styled(
            fixed(&channel.kind, 14),
            Style::default().add_modifier(Modifier::BOLD),
        ),
        status_span(
            channel_status_label(channel.status),
            channel_status_color(channel.status),
            12,
        ),
        Span::styled(
            channel.version.as_deref().unwrap_or("-").to_string(),
            muted_style(),
        ),
    ];
    if let Some(reason) = channel.reason.as_ref().filter(|reason| !reason.is_empty()) {
        spans.push(Span::raw("  "));
        spans.push(Span::styled(reason.clone(), Style::default().fg(ERROR)));
    }
    spans
}

pub(super) fn tunnel_row(tunnel: &TunnelRuntime) -> Vec<Span<'static>> {
    vec![
        Span::styled(
            fixed(&tunnel.provider, 14),
            Style::default().add_modifier(Modifier::BOLD),
        ),
        status_span(
            tunnel_status_label(&tunnel.status),
            tunnel_status_color(&tunnel.status),
            10,
        ),
        Span::styled(
            tunnel.url.as_deref().unwrap_or("-").to_string(),
            muted_style(),
        ),
    ]
}

pub(super) fn agent_row(agent: &AgentRuntime) -> Vec<Span<'static>> {
    let name = agent
        .agent_title
        .as_deref()
        .or(agent.agent_name.as_deref())
        .or(agent.cli_kind.as_deref())
        .unwrap_or("-");
    vec![
        Span::styled(
            fixed(&agent.route_key, 18),
            Style::default().add_modifier(Modifier::BOLD),
        ),
        status_span(
            if agent.busy { "busy" } else { "idle" },
            if agent.busy { WARN } else { OK },
            8,
        ),
        Span::styled(name.to_string(), muted_style()),
    ]
}

pub(super) fn session_row(session: &SessionListItem) -> Vec<Span<'static>> {
    let status = session_status_label(&session.status);
    let tool = format!("{:?}", session.tool).to_ascii_lowercase();
    vec![
        Span::styled(
            fixed(&short_id(&session.session_id), 14),
            Style::default().add_modifier(Modifier::BOLD),
        ),
        status_span(status, session_status_color(&session.status), 10),
        Span::styled(fixed(&tool, 12), muted_style()),
        Span::styled(
            session.project_path.as_deref().unwrap_or("-").to_string(),
            muted_style(),
        ),
    ]
}

pub(super) fn launch_session_row(session: &LaunchSessionInfo) -> Vec<Span<'static>> {
    vec![
        Span::styled(
            fixed(&session.short_id, 10),
            Style::default().add_modifier(Modifier::BOLD),
        ),
        status_span(
            if session.active {
                "active"
            } else if session.archived {
                "archived"
            } else {
                "saved"
            },
            if session.active { ACTION } else { NEUTRAL },
            10,
        ),
        Span::styled(fixed(&session.agent_id, 12), muted_style()),
        Span::styled(session.title.clone(), Style::default()),
        Span::styled(format!("  {}", session.workspace), muted_style()),
    ]
}

pub(super) fn agent_info_row(agent: &AgentInfo) -> Vec<Span<'static>> {
    vec![
        Span::styled(
            fixed(&agent.id, 14),
            Style::default().add_modifier(Modifier::BOLD),
        ),
        Span::styled(agent.name.clone(), muted_style()),
    ]
}

pub(super) fn profile_row(profile: &ModelProfileSummary) -> Vec<Span<'static>> {
    vec![
        Span::styled(
            fixed(&profile.label, 18),
            Style::default().add_modifier(Modifier::BOLD),
        ),
        Span::styled(profile.provider_label.clone(), muted_style()),
    ]
}

pub(super) fn workspace_row(workspace: &WorkspaceItem) -> Vec<Span<'static>> {
    let marker = if workspace.is_default { "* " } else { "  " };
    vec![
        Span::styled(marker, Style::default().fg(ACTION)),
        Span::styled(workspace.path.clone(), Style::default()),
    ]
}

fn fixed(value: &str, width: usize) -> String {
    format!("{value:<width$}")
}

fn status_span(label: &'static str, color: Color, width: usize) -> Span<'static> {
    Span::styled(
        fixed(label, width),
        Style::default().fg(color).add_modifier(Modifier::BOLD),
    )
}

fn short_id(value: &str) -> String {
    value.chars().take(12).collect()
}

fn channel_status_color(status: ChannelStatus) -> Color {
    match status {
        ChannelStatus::Running => OK,
        ChannelStatus::Spawning => WARN,
        ChannelStatus::Crashed => ERROR,
        ChannelStatus::Stopped | ChannelStatus::NotStarted => NEUTRAL,
    }
}

fn tunnel_status_color(status: &TunnelStatus) -> Color {
    match status {
        TunnelStatus::Running => OK,
        TunnelStatus::Stopped { .. } => NEUTRAL,
        TunnelStatus::Failed { .. } => ERROR,
    }
}

fn session_status_color(status: &PtyRunState) -> Color {
    match status {
        PtyRunState::Running { .. } => OK,
        PtyRunState::Exited { .. } => NEUTRAL,
    }
}

#[cfg(test)]
fn row_text(row: Vec<Span<'static>>) -> String {
    row.into_iter()
        .map(|span| span.content.into_owned())
        .collect::<String>()
        .trim_end()
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn channel(kind: &str) -> ChannelRuntime {
        ChannelRuntime {
            kind: kind.into(),
            version: Some("0.1.0".into()),
            plugin_dir: None,
            status: ChannelStatus::Running,
            reason: None,
        }
    }

    #[test]
    fn formats_runtime_lines() {
        let channel = channel("feishu");
        assert_eq!(
            row_text(channel_row(&channel)),
            "feishu        running     0.1.0"
        );

        let tunnel = TunnelRuntime {
            provider: "cloudflare".into(),
            url: Some("https://example.test".into()),
            status: TunnelStatus::Running,
            uptime_secs: 10,
        };
        assert_eq!(
            row_text(tunnel_row(&tunnel)),
            "cloudflare    running   https://example.test"
        );

        let session = SessionListItem {
            session_id: "abcdef1234567890".into(),
            tool: va_client::sessions::PtyTool::Codex,
            status: PtyRunState::Running {
                tool: va_client::sessions::PtyTool::Codex,
            },
            created_at: 1,
            project_path: Some("/tmp/project".into()),
            profile_id: None,
            profile_label: None,
            launch_target: None,
            tmux_session: None,
        };
        assert_eq!(
            row_text(session_row(&session)),
            "abcdef123456  running   codex       /tmp/project"
        );

        let launch_session = LaunchSessionInfo {
            agent_id: "codex".into(),
            session_id: "abcdef1234567890".into(),
            title: "Fix tests".into(),
            workspace: "/tmp/project".into(),
            updated_at: 10,
            short_id: "abcdef12".into(),
            archived: false,
            active: true,
        };
        assert_eq!(
            row_text(launch_session_row(&launch_session)),
            "abcdef12  active    codex       Fix tests  /tmp/project"
        );
    }
}
