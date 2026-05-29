use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};

use common::pty::{list_tmux_sessions, tmux_available, PtyTool, SessionId};

use crate::web_server::AppState;

/// GET /api/tmux/sessions -- list active tmux sessions and whether tmux is available.
pub async fn list_tmux_sessions_handler() -> Json<crate::api_types::TmuxSessionsResponse> {
    let available = tmux_available();
    let sessions = if available {
        list_tmux_sessions()
    } else {
        vec![]
    };
    Json(crate::api_types::TmuxSessionsResponse {
        available,
        sessions,
    })
}

/// Request body for POST /api/sessions.
#[derive(serde::Deserialize)]
pub(crate) struct CreateSessionBody {
    tool: Option<PtyTool>,
    profile_id: Option<String>,
    launch_target: Option<String>,
    project_path: Option<String>,
    tmux_session: Option<String>,
    theme: Option<String>,
    cols: Option<u16>,
    rows: Option<u16>,
}

/// GET /api/sessions -- list all active sessions.
pub async fn list_sessions_handler(
    State(state): State<AppState>,
) -> Json<Vec<crate::api_types::SessionListItem>> {
    let items = state
        .pty_manager
        .list_sessions()
        .into_iter()
        .map(|item| crate::api_types::SessionListItem {
            session_id: item.session_id,
            tool: item.tool,
            status: item.status,
            created_at: item.created_at,
            project_path: item.project_path,
            profile_id: item.profile_id,
            profile_label: item.profile_label,
            launch_target: item.launch_target,
            tmux_session: item.tmux_session,
        })
        .collect();
    Json(items)
}

#[derive(serde::Deserialize)]
pub(crate) struct LaunchSessionsQuery {
    workspace_path: Option<String>,
    include_archived: Option<bool>,
    limit: Option<usize>,
}

/// GET /api/agents/:agent_id/launch-sessions -- list CLI sessions this agent can resume.
pub async fn list_launch_sessions_handler(
    State(state): State<AppState>,
    Path(agent_id): Path<String>,
    Query(query): Query<LaunchSessionsQuery>,
) -> Result<Json<Vec<crate::api_types::LaunchSessionInfo>>, (StatusCode, String)> {
    let agent_id = common::resources::resolve_agent_id(&agent_id)
        .map_err(|error| (StatusCode::BAD_REQUEST, error))?;
    let workspace = query
        .workspace_path
        .map(std::path::PathBuf::from)
        .map(common::workspace::normalize_workspace_cwd)
        .unwrap_or_else(|| common::config::ensure_loaded().resolve_workspace(&agent_id));
    let limit = query.limit.unwrap_or(25).clamp(1, 100);
    let sessions = common::launch_sessions::list_for_agent_workspace_with_archived_async(
        &agent_id,
        &workspace,
        limit,
        query.include_archived.unwrap_or(false),
    )
    .await
    .into_iter()
    .map(|session| launch_session_info(&state, session))
    .collect();

    Ok(Json(sessions))
}

#[derive(serde::Deserialize)]
pub(crate) struct LaunchSessionsBatchBody {
    agent_ids: Vec<String>,
    workspace_paths: Vec<String>,
    include_archived: Option<bool>,
    limit: Option<usize>,
}

/// POST /api/launch-sessions -- list resumable CLI sessions across agents/workspaces.
pub async fn list_launch_sessions_batch_handler(
    State(state): State<AppState>,
    Json(body): Json<LaunchSessionsBatchBody>,
) -> Result<Json<Vec<crate::api_types::LaunchSessionInfo>>, (StatusCode, String)> {
    let mut agent_ids = Vec::new();
    for agent_id in body.agent_ids {
        let resolved = common::resources::resolve_agent_id(&agent_id)
            .map_err(|error| (StatusCode::BAD_REQUEST, error))?;
        if !agent_ids.contains(&resolved) {
            agent_ids.push(resolved);
        }
    }
    if agent_ids.is_empty() {
        return Ok(Json(Vec::new()));
    }

    let mut workspaces = Vec::new();
    for workspace_path in body.workspace_paths {
        let workspace =
            common::workspace::normalize_workspace_cwd(std::path::PathBuf::from(workspace_path));
        if !workspaces.contains(&workspace) {
            workspaces.push(workspace);
        }
    }
    if workspaces.is_empty() {
        let config = common::config::ensure_loaded();
        for agent_id in &agent_ids {
            let workspace = config.resolve_workspace(agent_id);
            if !workspaces.contains(&workspace) {
                workspaces.push(workspace);
            }
        }
    }

    let limit = body.limit.unwrap_or(25).clamp(1, 100);
    let include_archived = body.include_archived.unwrap_or(false);
    let mut sessions = Vec::new();
    for agent_id in &agent_ids {
        sessions.extend(
            common::launch_sessions::list_for_agent_workspaces_with_archived_async(
                agent_id,
                &workspaces,
                limit,
                include_archived,
            )
            .await
            .into_iter()
            .map(|session| launch_session_info(&state, session)),
        );
    }
    sessions.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));

    Ok(Json(sessions))
}

fn launch_session_info(
    state: &AppState,
    session: common::launch_sessions::LaunchSession,
) -> crate::api_types::LaunchSessionInfo {
    let active = state
        .web_channel
        .session_is_active(&session.agent_id, &session.session_id);
    crate::api_types::LaunchSessionInfo {
        short_id: common::launch_sessions::short_id(&session.session_id),
        agent_id: session.agent_id,
        session_id: session.session_id,
        title: session.title,
        workspace: session.workspace,
        updated_at: session.updated_at,
        archived: session.archived,
        active,
    }
}

#[derive(serde::Deserialize)]
pub(crate) struct LaunchSessionArchiveBody {
    workspace_path: Option<String>,
}

#[derive(serde::Deserialize)]
pub(crate) struct LaunchSessionArchiveQuery {
    workspace_path: Option<String>,
}

/// POST /api/agents/:agent_id/launch-sessions/:session_id/archive -- hide a
/// CLI-owned session in VibeAround without modifying the agent's session store.
pub async fn archive_launch_session_handler(
    Path((agent_id, session_id)): Path<(String, String)>,
    Json(body): Json<LaunchSessionArchiveBody>,
) -> Result<StatusCode, (StatusCode, String)> {
    set_launch_session_archived(agent_id, session_id, body.workspace_path, true).await
}

/// POST /api/agents/:agent_id/launch-sessions/:session_id/unarchive -- show a
/// previously hidden session again without relying on a DELETE request body.
pub async fn unarchive_launch_session_handler(
    Path((agent_id, session_id)): Path<(String, String)>,
    Json(body): Json<LaunchSessionArchiveBody>,
) -> Result<StatusCode, (StatusCode, String)> {
    set_launch_session_archived(agent_id, session_id, body.workspace_path, false).await
}

/// DELETE /api/agents/:agent_id/launch-sessions/:session_id/archive -- legacy
/// unarchive endpoint. Use query parameters so clients do not need a DELETE
/// request body.
pub async fn unarchive_launch_session_delete_handler(
    Path((agent_id, session_id)): Path<(String, String)>,
    Query(query): Query<LaunchSessionArchiveQuery>,
) -> Result<StatusCode, (StatusCode, String)> {
    set_launch_session_archived(agent_id, session_id, query.workspace_path, false).await
}

async fn set_launch_session_archived(
    agent_id: String,
    session_id: String,
    workspace_path: Option<String>,
    archived: bool,
) -> Result<StatusCode, (StatusCode, String)> {
    let agent_id = common::resources::resolve_agent_id(&agent_id)
        .map_err(|error| (StatusCode::BAD_REQUEST, error))?;
    let workspace = workspace_path
        .map(std::path::PathBuf::from)
        .map(common::workspace::normalize_workspace_cwd)
        .unwrap_or_else(|| common::config::ensure_loaded().resolve_workspace(&agent_id));

    let result = if archived {
        common::launch_sessions::archive_session(&agent_id, &workspace, &session_id)
    } else {
        common::launch_sessions::unarchive_session(&agent_id, &workspace, &session_id)
    };
    result.map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error))?;
    Ok(StatusCode::NO_CONTENT)
}

/// POST /api/sessions -- create a new PTY session.
pub async fn create_session_handler(
    State(state): State<AppState>,
    Json(body): Json<CreateSessionBody>,
) -> Result<Json<crate::api_types::CreateSessionResponse>, (StatusCode, String)> {
    let initial_size = match (body.cols, body.rows) {
        (Some(c), Some(r)) => Some((c, r)),
        _ => None,
    };

    let created = match (body.profile_id.as_deref(), body.launch_target.as_deref()) {
        (Some(profile_id), Some(launch_target)) => {
            if body.tool.is_some() {
                return Err((
                    StatusCode::BAD_REQUEST,
                    "profile sessions cannot also specify tool".to_string(),
                ));
            }
            if body.tmux_session.is_some() {
                return Err((
                    StatusCode::BAD_REQUEST,
                    "profile sessions cannot attach tmux".to_string(),
                ));
            }
            let profile = common::profiles::schema::load(profile_id)
                .map(common::profiles::normalize_legacy_profile_and_persist)
                .ok_or_else(|| {
                    (
                        StatusCode::BAD_REQUEST,
                        format!("profile '{}' not found", profile_id),
                    )
                })?;
            let route =
                common::profiles::connections::resolve_profile_agent_route(&profile, launch_target)
                    .ok_or_else(|| {
                        (
                            StatusCode::BAD_REQUEST,
                            format!("profile '{}' cannot launch '{}'", profile.id, launch_target),
                        )
                    })?;
            let launch_id = uuid::Uuid::new_v4().to_string();
            let rendered = common::profiles::runtime::render_for_agent_route(
                &profile,
                launch_target,
                &launch_id,
                &route,
            )
            .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;
            let command_args = rendered.command_args.clone();
            let mut env = common::profiles::runtime::materialize_env(&profile.id, rendered)
                .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
            if route.bridge_target_api_type.is_none() {
                common::profiles::runtime::append_settings_proxy_env(&profile, &mut env)
                    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
            }
            env.push(("VIBEAROUND_LAUNCH_ID".to_string(), launch_id));
            env.push(("VIBEAROUND_PROFILE_ID".to_string(), profile.id.clone()));
            env.push((
                "VIBEAROUND_LAUNCH_TARGET".to_string(),
                launch_target.to_string(),
            ));
            let agent_id = common::profiles::runtime::agent_id_for(launch_target)
                .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;
            let agent = common::resources::agent_by_id(agent_id).ok_or_else(|| {
                (
                    StatusCode::BAD_REQUEST,
                    format!("agent '{}' not found", agent_id),
                )
            })?;
            let pty_tool = PtyTool::from_agent_id(agent_id).ok_or_else(|| {
                (
                    StatusCode::BAD_REQUEST,
                    format!("agent '{}' cannot be launched in a PTY", agent_id),
                )
            })?;
            state
                .pty_manager
                .create_profile_session(
                    pty_tool,
                    command_with_args(&agent.pty.command, &command_args),
                    env,
                    profile.id.clone(),
                    profile.label.clone(),
                    launch_target.to_string(),
                    body.project_path.clone(),
                    body.theme.clone(),
                    initial_size,
                )
                .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        }
        (None, None) => {
            let tool = body.tool.ok_or_else(|| {
                (
                    StatusCode::BAD_REQUEST,
                    "missing tool for direct session".to_string(),
                )
            })?;
            if body.tmux_session.is_some() && tool != PtyTool::Generic {
                return Err((
                    StatusCode::BAD_REQUEST,
                    "tmux sessions must use the generic tool".to_string(),
                ));
            }
            state
                .pty_manager
                .create_session(
                    tool,
                    body.project_path.clone(),
                    body.tmux_session.clone(),
                    body.theme.clone(),
                    initial_size,
                )
                .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        }
        _ => {
            return Err((
                StatusCode::BAD_REQUEST,
                "profile_id and launch_target must be provided together".to_string(),
            ));
        }
    };

    Ok(Json(crate::api_types::CreateSessionResponse {
        session_id: created.session_id,
        tool: created.tool,
        created_at: created.created_at,
        project_path: created.project_path,
        profile_id: created.profile_id,
        profile_label: created.profile_label,
        launch_target: created.launch_target,
    }))
}

/// DELETE /api/sessions/:session_id -- kill and remove a session.
pub async fn delete_session_handler(
    State(state): State<AppState>,
    Path(session_id): Path<String>,
) -> impl IntoResponse {
    let uuid = match uuid::Uuid::parse_str(&session_id) {
        Ok(u) => u,
        Err(_) => return (StatusCode::BAD_REQUEST, "Invalid session_id".to_string()),
    };
    let sid = SessionId(uuid);
    if state.pty_manager.delete_session(sid) {
        (StatusCode::OK, format!("Session {} deleted", session_id))
    } else {
        (
            StatusCode::NOT_FOUND,
            format!("Session {} not found", session_id),
        )
    }
}

fn command_with_args(command: &str, args: &[String]) -> String {
    if args.is_empty() {
        return command.to_string();
    }
    let mut out = command.to_string();
    for arg in args {
        out.push(' ');
        out.push_str(&shell_quote(arg));
    }
    out
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\"'\"'"))
}
