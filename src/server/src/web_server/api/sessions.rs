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
    resume_session_id: Option<String>,
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
    let sessions = state
        .channel_hub
        .workspace_thread_manager()
        .list_resumable_agent_sessions(
            &agent_id,
            &workspace,
            limit,
            query.include_archived.unwrap_or(false),
        )
        .await
        .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?
        .into_iter()
        .collect::<Vec<_>>();
    let sessions = launch_session_infos(&state, sessions).await?;

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
    let workspace_threads = state.channel_hub.workspace_thread_manager();
    for agent_id in &agent_ids {
        for workspace in &workspaces {
            sessions.extend(
                workspace_threads
                    .list_resumable_agent_sessions(agent_id, workspace, limit, include_archived)
                    .await
                    .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?
                    .into_iter(),
            );
        }
    }
    let mut sessions = launch_session_infos(&state, sessions).await?;
    sessions.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));

    Ok(Json(sessions))
}

async fn launch_session_infos(
    state: &AppState,
    sessions: Vec<common::launch_sessions::LaunchSession>,
) -> Result<Vec<crate::api_types::LaunchSessionInfo>, (StatusCode, String)> {
    let mut infos = Vec::with_capacity(sessions.len());
    for session in sessions {
        infos.push(launch_session_info(state, session).await?);
    }
    Ok(infos)
}

async fn launch_session_info(
    state: &AppState,
    session: common::launch_sessions::LaunchSession,
) -> Result<crate::api_types::LaunchSessionInfo, (StatusCode, String)> {
    let active = state
        .web_channel
        .session_is_active(&session.agent_id, &session.session_id)
        .await;
    let thread_host = state
        .channel_hub
        .workspace_thread_manager()
        .thread_host_for_agent_session(
            &session.agent_id,
            std::path::Path::new(&session.workspace),
            &session.session_id,
        )
        .await
        .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?;
    let (thread_id, host_agent_id, host_profile_id) =
        if let Some((thread_id, host_binding)) = thread_host {
            (
                Some(thread_id.to_string()),
                host_binding.agent_id,
                host_binding.profile_id,
            )
        } else {
            (None, session.agent_id.clone(), session.profile_id.clone())
        };
    let host_profile_label = crate::api_types::agent_profile_label(host_profile_id.as_deref());
    let (host_provider, host_provider_label) = profile_provider_label(host_profile_id.as_deref());
    Ok(crate::api_types::LaunchSessionInfo {
        short_id: common::launch_sessions::short_id(&session.session_id),
        agent_id: session.agent_id,
        host_agent_id,
        host_profile_id,
        host_profile_label,
        host_provider,
        host_provider_label,
        session_id: session.session_id,
        title: session.title,
        workspace: session.workspace,
        updated_at: session.updated_at,
        archived: session.archived,
        active,
        thread_id,
    })
}

fn profile_provider_label(profile_id: Option<&str>) -> (Option<String>, Option<String>) {
    let Some(profile_id) = profile_id else {
        return (None, None);
    };
    let Some(profile) = common::profiles::load_profile(profile_id) else {
        return (None, None);
    };
    let provider_id = profile.provider;
    let provider_label = common::profiles::catalog::get(&provider_id)
        .map(|provider| provider.label.clone())
        .unwrap_or_else(|| provider_id.clone());
    (Some(provider_id), Some(provider_label))
}

fn trimmed(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

pub async fn init_workspace_thread_handler(
    State(state): State<AppState>,
    Json(body): Json<crate::api_types::WorkspaceThreadInitRequest>,
) -> Result<Json<crate::api_types::WorkspaceThreadInitResponse>, (StatusCode, String)> {
    let manager = state.channel_hub.workspace_thread_manager();
    let bad_request = |error: anyhow::Error| (StatusCode::BAD_REQUEST, error.to_string());

    if let Some(thread_id) = trimmed(body.thread_id.as_deref()) {
        let runtime = manager
            .attach_web_route_to_thread(&thread_id.into())
            .await
            .map_err(bad_request)?;
        return Ok(Json(web_thread_response(&runtime).await));
    }

    let agent_id =
        common::resources::resolve_agent_id(body.agent_id.as_deref().unwrap_or_default())
            .map_err(|error| (StatusCode::BAD_REQUEST, error))?;
    let workspace = body
        .workspace_path
        .as_deref()
        .map(std::path::PathBuf::from)
        .map(common::workspace::normalize_workspace_cwd)
        .unwrap_or_else(|| common::config::ensure_loaded().resolve_workspace(&agent_id));

    if let Some(session_id) = trimmed(body.session_id.as_deref()) {
        let runtime = manager
            .attach_external_session_to_web_thread(agent_id, body.profile_id, session_id, workspace)
            .await
            .map_err(bad_request)?;
        return Ok(Json(web_thread_response(&runtime).await));
    }

    // Nothing exists yet, so nothing is created yet.
    let (thread, workspace) = manager
        .draft_web_thread(agent_id, body.profile_id, workspace)
        .await
        .map_err(bad_request)?;
    Ok(Json(crate::api_types::WorkspaceThreadInitResponse {
        chat_id: common::workspace::manager::web_chat_id_for_thread(&thread.id),
        thread_id: thread.id.to_string(),
        agent_id: thread.host_binding.agent_id,
        profile_id: thread.host_binding.profile_id,
        session_id: None,
        workspace: workspace.to_string_lossy().to_string(),
    }))
}

async fn web_thread_response(
    runtime: &common::workspace::threads::runtime::ThreadRuntime,
) -> crate::api_types::WorkspaceThreadInitResponse {
    let state = runtime.state().await;
    crate::api_types::WorkspaceThreadInitResponse {
        chat_id: common::workspace::manager::web_chat_id_for_thread(&state.thread_id),
        thread_id: state.thread_id.to_string(),
        agent_id: state.host_binding.agent_id,
        profile_id: state.host_binding.profile_id,
        session_id: state.session_id,
        workspace: state.workspace.to_string_lossy().to_string(),
    }
}

#[derive(serde::Deserialize)]
pub(crate) struct LaunchSessionArchiveBody {
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

/// POST /api/agents/:agent_id/launch-sessions/:session_id/unarchive.
pub async fn unarchive_launch_session_handler(
    Path((agent_id, session_id)): Path<(String, String)>,
    Json(body): Json<LaunchSessionArchiveBody>,
) -> Result<StatusCode, (StatusCode, String)> {
    set_launch_session_archived(agent_id, session_id, body.workspace_path, false).await
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

    if body.resume_session_id.is_some() {
        return create_resume_session(state, body, initial_size).map(Json);
    }

    let created = match (body.profile_id.as_deref(), body.launch_target.as_deref()) {
        (Some(_), Some(_)) => {
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
            let plan = super::launcher::build_launch_plan(super::launcher::LaunchPlanBody {
                agent_id: None,
                profile_id: body.profile_id.clone(),
                launch_target: body.launch_target.clone(),
                session_id: None,
                terminal_id: Some(super::launcher::WEB_PTY_TERMINAL_ID.to_string()),
            })?;
            create_session_from_launch_plan(
                &state,
                plan,
                body.project_path.clone(),
                body.theme.clone(),
                initial_size,
            )?
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
            if tool == PtyTool::Generic || body.tmux_session.is_some() {
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
            } else {
                let agent_id = tool.agent_id().ok_or_else(|| {
                    (
                        StatusCode::BAD_REQUEST,
                        "direct agent sessions require a coding-agent tool".to_string(),
                    )
                })?;
                let plan = super::launcher::build_launch_plan(super::launcher::LaunchPlanBody {
                    agent_id: Some(agent_id.to_string()),
                    profile_id: None,
                    launch_target: None,
                    session_id: None,
                    terminal_id: Some(super::launcher::WEB_PTY_TERMINAL_ID.to_string()),
                })?;
                create_session_from_launch_plan(
                    &state,
                    plan,
                    body.project_path.clone(),
                    body.theme.clone(),
                    initial_size,
                )?
            }
        }
        _ => {
            return Err((
                StatusCode::BAD_REQUEST,
                "profile_id and launch_target must be provided together".to_string(),
            ));
        }
    };

    Ok(Json(create_session_response(created)))
}

fn create_resume_session(
    state: AppState,
    body: CreateSessionBody,
    initial_size: Option<(u16, u16)>,
) -> Result<crate::api_types::CreateSessionResponse, (StatusCode, String)> {
    if body.tmux_session.is_some() {
        return Err((
            StatusCode::BAD_REQUEST,
            "resume sessions cannot attach tmux".to_string(),
        ));
    }
    let resume_session_id = body.resume_session_id.ok_or_else(|| {
        (
            StatusCode::BAD_REQUEST,
            "resume_session_id is required".to_string(),
        )
    })?;
    let agent_id = match (body.profile_id.as_deref(), body.launch_target.as_deref()) {
        (Some(_), Some(_)) => {
            if body.tool.is_some() {
                return Err((
                    StatusCode::BAD_REQUEST,
                    "profile resume sessions cannot also specify tool".to_string(),
                ));
            }
            None
        }
        (None, None) => {
            let tool = body.tool.ok_or_else(|| {
                (
                    StatusCode::BAD_REQUEST,
                    "missing tool for direct resume session".to_string(),
                )
            })?;
            let agent_id = tool.agent_id().ok_or_else(|| {
                (
                    StatusCode::BAD_REQUEST,
                    "resume sessions require a coding-agent tool".to_string(),
                )
            })?;
            Some(agent_id.to_string())
        }
        _ => {
            return Err((
                StatusCode::BAD_REQUEST,
                "profile_id and launch_target must be provided together".to_string(),
            ));
        }
    };

    let plan = super::launcher::build_launch_plan(super::launcher::LaunchPlanBody {
        agent_id,
        profile_id: body.profile_id.clone(),
        launch_target: body.launch_target.clone(),
        session_id: Some(resume_session_id),
        terminal_id: Some(super::launcher::WEB_PTY_TERMINAL_ID.to_string()),
    })?;
    let created = create_session_from_launch_plan(
        &state,
        plan,
        body.project_path.clone(),
        body.theme.clone(),
        initial_size,
    )?;
    Ok(create_session_response(created))
}

fn create_session_from_launch_plan(
    state: &AppState,
    plan: crate::api_types::LaunchPlanResponse,
    project_path: Option<String>,
    theme: Option<String>,
    initial_size: Option<(u16, u16)>,
) -> Result<common::pty::PtySessionCreated, (StatusCode, String)> {
    let pty_tool = PtyTool::from_agent_id(&plan.agent_id).ok_or_else(|| {
        (
            StatusCode::BAD_REQUEST,
            format!("agent '{}' cannot be launched in a PTY", plan.agent_id),
        )
    })?;
    let command = command_with_args(&plan.command, &plan.args);
    let env = plan
        .env
        .into_iter()
        .map(|item| (item.key, item.value))
        .collect::<Vec<_>>();
    let project_path = project_path.or(Some(plan.cwd));
    let created = state
        .pty_manager
        .create_command_session(
            pty_tool,
            command,
            env,
            plan.profile_id.clone(),
            Some(plan.display.title),
            Some(plan.launch_target.clone()),
            project_path,
            theme,
            initial_size,
        )
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    Ok(created)
}

fn create_session_response(
    created: common::pty::PtySessionCreated,
) -> crate::api_types::CreateSessionResponse {
    crate::api_types::CreateSessionResponse {
        session_id: created.session_id,
        tool: created.tool,
        created_at: created.created_at,
        project_path: created.project_path,
        profile_id: created.profile_id,
        profile_label: created.profile_label,
        launch_target: created.launch_target,
    }
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
    #[cfg(windows)]
    {
        format!("'{}'", value.replace('\'', "''"))
    }
    #[cfg(not(windows))]
    {
        format!("'{}'", value.replace('\'', "'\"'\"'"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(windows)]
    #[test]
    fn command_with_args_uses_powershell_quoting_on_windows() {
        assert_eq!(
            command_with_args(
                "claude",
                &["team's session".to_string(), "path with spaces".to_string()]
            ),
            "claude 'team''s session' 'path with spaces'"
        );
    }

    #[cfg(not(windows))]
    #[test]
    fn command_with_args_uses_shell_quoting_on_unix() {
        assert_eq!(
            command_with_args(
                "claude",
                &["team's session".to_string(), "path with spaces".to_string()]
            ),
            "claude 'team'\"'\"'s session' 'path with spaces'"
        );
    }
}
