use va_client::launcher::LauncherPreferencesResponse;
use va_client::ops;
use va_client::profiles::ModelProfileSummary;
use va_client::runtime::{AgentInfo, AgentRuntime, AgentsConfig, ChannelRuntime, TunnelRuntime};
use va_client::service::ServiceInfoResponse;
use va_client::sessions::{LaunchSessionInfo, SessionListItem};
use va_client::workspaces::WorkspaceItem;

use crate::transport::{HttpTransport, TuiError};

#[derive(Debug, Default)]
pub(crate) struct DashboardSnapshot {
    pub(crate) service: Option<ServiceInfoResponse>,
    pub(crate) channels: Vec<ChannelRuntime>,
    pub(crate) tunnels: Vec<TunnelRuntime>,
    pub(crate) agents: Vec<AgentRuntime>,
    pub(crate) sessions: Vec<SessionListItem>,
}

#[derive(Debug, Default)]
pub(crate) struct AgentPickerSnapshot {
    pub(crate) agents: Vec<AgentInfo>,
    pub(crate) profiles: Vec<ModelProfileSummary>,
    pub(crate) workspaces: Vec<WorkspaceItem>,
    pub(crate) sessions: Vec<LaunchSessionInfo>,
    pub(crate) preferences: Option<LauncherPreferencesResponse>,
}

pub(crate) async fn fetch_snapshot(
    transport: &HttpTransport,
) -> Result<DashboardSnapshot, TuiError> {
    Ok(DashboardSnapshot {
        service: Some(transport.execute(ops::service_info()).await?),
        channels: transport.execute(ops::runtime_channels()).await?,
        tunnels: transport.execute(ops::runtime_tunnels()).await?,
        agents: transport.execute(ops::runtime_agent_hosts()).await?,
        sessions: transport.execute(ops::sessions()).await?,
    })
}

/// Fetch only the launcher preferences (selected agent/profile/workspace),
/// the lightweight call used to seed the chat context on startup.
pub(crate) async fn fetch_launcher_preferences(
    transport: &HttpTransport,
) -> Result<LauncherPreferencesResponse, TuiError> {
    transport.execute(ops::launcher_preferences()).await
}

pub(crate) async fn fetch_agent_picker(
    transport: &HttpTransport,
) -> Result<AgentPickerSnapshot, TuiError> {
    let preferences = transport.execute(ops::launcher_preferences()).await?;
    let agents: AgentsConfig = transport.execute(ops::runtime_agents()).await?;
    let profiles = transport.execute(ops::model_profiles()).await?;
    let workspaces = transport.execute(ops::workspaces()).await?.workspaces;
    let mut session_agent_ids = preferences.enabled_agents.clone();
    if session_agent_ids.is_empty() {
        session_agent_ids = agents.agents.iter().map(|agent| agent.id.clone()).collect();
    }
    let agent_refs = session_agent_ids
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>();
    let workspace_refs = workspaces
        .iter()
        .map(|workspace| workspace.path.as_str())
        .collect::<Vec<_>>();
    let sessions = if agent_refs.is_empty() {
        Vec::new()
    } else {
        transport
            .execute(ops::launch_sessions_batch(
                &agent_refs,
                &workspace_refs,
                Some(false),
                Some(50),
            )?)
            .await?
    };
    Ok(AgentPickerSnapshot {
        agents: agents.agents,
        profiles,
        workspaces,
        sessions,
        preferences: Some(preferences),
    })
}
