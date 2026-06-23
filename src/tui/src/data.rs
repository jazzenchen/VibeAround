use va_client::launcher::LauncherPreferencesResponse;
use va_client::ops;
use va_client::profiles::ModelProfileSummary;
use va_client::runtime::{AgentInfo, AgentRuntime, AgentsConfig, ChannelRuntime, TunnelRuntime};
use va_client::service::ServiceInfoResponse;
use va_client::sessions::SessionListItem;
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
    pub(crate) sessions: Vec<SessionListItem>,
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

pub(crate) async fn fetch_agent_picker(
    transport: &HttpTransport,
) -> Result<AgentPickerSnapshot, TuiError> {
    let preferences = transport.execute(ops::launcher_preferences()).await?;
    let agents: AgentsConfig = transport.execute(ops::runtime_agents()).await?;
    Ok(AgentPickerSnapshot {
        agents: agents.agents,
        profiles: transport.execute(ops::model_profiles()).await?,
        workspaces: transport.execute(ops::workspaces()).await?.workspaces,
        sessions: transport.execute(ops::sessions()).await?,
        preferences: Some(preferences),
    })
}
