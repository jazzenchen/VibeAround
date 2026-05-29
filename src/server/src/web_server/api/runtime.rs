use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};

use common::pty::SessionId;
use common::state::StateSource;
use common::{agent_state, config};

use crate::web_server::AppState;

/// GET /api/agents -- list enabled agents and default agent for frontend agent selector.
pub async fn list_agents_handler() -> Json<crate::api_types::AgentsConfig> {
    let cfg = config::ensure_loaded();
    let agent_prefs = agent_state::read_prefs();
    Json(crate::api_types::AgentsConfig {
        agents: crate::api_types::AgentInfo::for_ids(&cfg.enabled_agents),
        default_agent: agent_state::resolve_default_agent(&agent_prefs, &cfg),
    })
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
                kind: s.kind,
                status: s.status.as_str(),
                reason: if s.reason.is_empty() {
                    None
                } else {
                    Some(s.reason)
                },
                crash_count: s.crash_count,
                last_seen_age_secs: s.last_seen_age_secs,
                restart_in_secs: s.restart_in_secs,
                started_at: s.started_at,
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
pub async fn reload_settings_handler() -> impl IntoResponse {
    config::reload();
    Json(serde_json::json!({ "ok": true }))
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
    let mut out = Vec::with_capacity(entries.len());
    for entry in entries {
        let st = entry.state;
        let (agent_name, agent_title, agent_version) = st
            .initialize
            .as_ref()
            .and_then(|i| i.agent_info.as_ref())
            .map(|info| {
                (
                    Some(info.name.clone()),
                    info.title.clone(),
                    Some(info.version.clone()),
                )
            })
            .unwrap_or((None, None, None));
        let (route_key, channel_kind, chat_id) = match entry.route {
            Some(route) => (
                route.as_key(),
                route.channel_kind.clone(),
                route.chat_id.clone(),
            ),
            None => (
                st.thread_id.to_string(),
                "workspace".to_string(),
                st.thread_id.to_string(),
            ),
        };
        out.push(crate::api_types::AgentRuntime {
            route_key,
            channel_kind,
            chat_id,
            cli_kind: Some(st.host_binding.agent_id.clone()),
            profile: st.host_binding.profile_id.clone(),
            session_id: st.session_id,
            workspace: Some(st.workspace.to_string_lossy().to_string()),
            busy: st.busy,
            failed: st.failed,
            started_at: 0,
            agent_name,
            agent_title,
            agent_version,
            multi_agent_turns: st.multi_agent_turns,
            subagents: st.agents,
        });
    }
    Json(out)
}

/// POST /api/channels/:kind/stop -- user-initiated stop of a channel
/// plugin (no auto-respawn).
pub async fn stop_channel_handler(
    State(state): State<AppState>,
    Path(kind): Path<String>,
) -> impl IntoResponse {
    match state.channel_hub.monitor().force_stop(&kind).await {
        Ok(()) => (StatusCode::OK, format!("Stopped {}", kind)),
        Err(e) => (StatusCode::NOT_FOUND, e),
    }
}

/// POST /api/channels/:kind/restart -- user-initiated restart (kill +
/// immediate respawn, no retry backoff).
pub async fn restart_channel_handler(
    State(state): State<AppState>,
    Path(kind): Path<String>,
) -> impl IntoResponse {
    match state.channel_hub.monitor().force_restart(&kind).await {
        Ok(()) => (StatusCode::OK, format!("Restarting {}", kind)),
        Err(e) => (StatusCode::NOT_FOUND, e),
    }
}

/// POST /api/channels/:kind/start -- transition a Stopped channel
/// back to Crashed(restart_at=now) so the next monitor tick respawns it.
pub async fn start_channel_handler(
    State(state): State<AppState>,
    Path(kind): Path<String>,
) -> impl IntoResponse {
    match state.channel_hub.monitor().force_start(&kind) {
        Ok(()) => (StatusCode::OK, format!("Starting {}", kind)),
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

/// DELETE /api/agents/:route_key -- stop a live workspace thread host.
///
/// `route_key` is the colon-joined form from `RouteKey::as_key()`, e.g.
/// `telegram:chat_42`, or a workspace thread id.
pub async fn kill_agent_handler(
    State(state): State<AppState>,
    Path(route_key): Path<String>,
) -> impl IntoResponse {
    if let Some(route) = common::routing::RouteKey::from_key(&route_key) {
        let _ = state
            .channel_hub
            .workspace_thread_manager()
            .shutdown_route_host(&route)
            .await;
        return (StatusCode::OK, format!("Stopped agent {}", route_key));
    }
    let thread_id = common::workspace::threads::store::WorkspaceThreadId::from(route_key.as_str());
    let _ = state
        .channel_hub
        .workspace_thread_manager()
        .shutdown_thread_host(&thread_id)
        .await;
    (StatusCode::OK, format!("Stopped agent {}", route_key))
}

/// DELETE /api/pty/:session_id -- kill a PTY session.
///
/// Goes through `PtySessionManager::delete_session` so the child
/// process gets SIGKILL'd, not just the registry entry removed.
pub async fn kill_pty_handler(
    State(state): State<AppState>,
    Path(session_id): Path<String>,
) -> impl IntoResponse {
    let Ok(uuid) = uuid::Uuid::parse_str(&session_id) else {
        return (
            StatusCode::BAD_REQUEST,
            format!("Invalid session id: {}", session_id),
        );
    };
    if state.pty_manager.delete_session(SessionId(uuid)) {
        (StatusCode::OK, format!("Killed pty {}", session_id))
    } else {
        (
            StatusCode::NOT_FOUND,
            format!("PTY session {} not found", session_id),
        )
    }
}
