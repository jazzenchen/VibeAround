use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use common::state::StateSource;
use common::workspace::threads::store::WorkspaceThreadId;
use common::{agent_state, config};

use crate::web_server::AppState;

/// GET /api/agents -- list enabled agents and default agent for frontend agent selector.
pub async fn list_agents_handler(
) -> Result<Json<crate::api_types::AgentsConfig>, (StatusCode, String)> {
    super::run_blocking_io(|| {
        let (cfg, agent_prefs) = agent_state::read_config_and_prefs();
        Ok(Json(crate::api_types::AgentsConfig {
            agents: crate::api_types::AgentInfo::for_ids(&cfg.enabled_agents),
            default_agent: agent_state::resolve_default_agent(&agent_prefs, &cfg),
        }))
    })
    .await
}

/// GET /api/channels -- live list of channel plugins from `ChannelMonitor`.
pub async fn list_channels_handler(
    State(state): State<AppState>,
) -> Json<Vec<crate::api_types::ChannelRuntime>> {
    let monitor = state.channel_hub.monitor();
    let entries = monitor.list().await;
    Json(
        entries
            .into_iter()
            .map(|s| crate::api_types::ChannelRuntime {
                instance_id: s.instance_id,
                kind: s.kind,
                version: s.version,
                plugin_dir: s.plugin_dir.map(|path| path.to_string_lossy().into_owned()),
                status: s.status.as_str(),
                reason: if s.reason.is_empty() {
                    None
                } else {
                    Some(s.reason)
                },
            })
            .collect(),
    )
}

/// POST /api/channels/sync -- reload settings.json and reconcile IM channel
/// plugins without restarting the whole daemon.
pub async fn sync_channels_handler(State(state): State<AppState>) -> impl IntoResponse {
    Json(state.channel_hub.sync_configured_plugins().await)
}

/// POST /api/settings/reload -- reload settings.json in the daemon process
/// without restarting tunnels, channels, or active agent sessions.
pub async fn reload_settings_handler() -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    super::run_blocking_io(|| {
        config::reload();
        Ok(Json(serde_json::json!({ "ok": true })))
    })
    .await
}

/// GET /api/tunnels -- live list of tunnels from `TunnelManager`.
pub async fn list_tunnels_handler(
    State(state): State<AppState>,
) -> Json<Vec<crate::api_types::TunnelRuntime>> {
    let entries = state.tunnels.list().await;
    Json(
        entries
            .into_iter()
            .map(|t| crate::api_types::TunnelRuntime {
                provider: t.provider.as_str(),
                url: t.url,
                status: t.status,
                uptime_secs: t.uptime_secs,
            })
            .collect(),
    )
}

/// GET /api/agents/runtime -- live list of workspace thread host runtimes.
pub async fn list_agents_runtime_handler(
    State(state): State<AppState>,
) -> Json<Vec<crate::api_types::AgentRuntime>> {
    let entries = state.channel_hub.workspace_thread_manager().list().await;
    Json(entries.into_iter().map(Into::into).collect())
}

/// POST /api/channels/:instance_id/stop -- user-initiated stop of a channel
/// plugin (no auto-respawn).
pub async fn stop_channel_handler(
    State(state): State<AppState>,
    Path(instance_id): Path<String>,
) -> impl IntoResponse {
    match state.channel_hub.monitor().force_stop(&instance_id).await {
        Ok(()) => (StatusCode::OK, format!("Stopped {}", instance_id)),
        Err(e) => (StatusCode::NOT_FOUND, e),
    }
}

/// POST /api/channels/:instance_id/restart -- user-initiated restart (kill +
/// immediate respawn).
pub async fn restart_channel_handler(
    State(state): State<AppState>,
    Path(instance_id): Path<String>,
) -> impl IntoResponse {
    match state
        .channel_hub
        .monitor()
        .force_restart(&instance_id)
        .await
    {
        Ok(()) => (StatusCode::OK, format!("Restarting {}", instance_id)),
        Err(e) => (StatusCode::NOT_FOUND, e),
    }
}

/// POST /api/channels/:instance_id/start -- transition a Stopped channel
/// back to Crashed(restart_at=now) so the next monitor tick respawns it.
pub async fn start_channel_handler(
    State(state): State<AppState>,
    Path(instance_id): Path<String>,
) -> impl IntoResponse {
    match state.channel_hub.monitor().force_start(&instance_id).await {
        Ok(()) => (StatusCode::OK, format!("Starting {}", instance_id)),
        Err(e) => (StatusCode::NOT_FOUND, e),
    }
}

/// DELETE /api/tunnels/:provider -- kill a running tunnel.
pub async fn kill_tunnel_handler(
    State(state): State<AppState>,
    Path(provider): Path<String>,
) -> impl IntoResponse {
    if state.tunnels.kill(&provider) {
        (StatusCode::OK, format!("Killed tunnel {}", provider))
    } else {
        (
            StatusCode::NOT_FOUND,
            format!("Tunnel {} not found", provider),
        )
    }
}

/// POST /api/workspace-threads/:thread_id/shutdown-host -- stop the ACP host
/// process of a live workspace thread. The thread, its session and its route
/// attachments survive; the next message starts the host again and resumes.
pub async fn shutdown_thread_host_handler(
    State(state): State<AppState>,
    Path(thread_id): Path<String>,
) -> impl IntoResponse {
    let workspace_threads = state.channel_hub.workspace_thread_manager();
    let resolved_thread_id = WorkspaceThreadId::from(thread_id.as_str());
    if !workspace_threads
        .list()
        .await
        .iter()
        .any(|entry| entry.state.thread_id == resolved_thread_id)
    {
        return (
            StatusCode::NOT_FOUND,
            format!("No live host for thread {}", thread_id),
        );
    }

    match workspace_threads
        .shutdown_thread_host(&resolved_thread_id)
        .await
    {
        Ok(()) => (
            StatusCode::OK,
            format!("Host stopped for thread {}", thread_id),
        ),
        Err(error) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to stop host for thread {}: {}", thread_id, error),
        ),
    }
}
