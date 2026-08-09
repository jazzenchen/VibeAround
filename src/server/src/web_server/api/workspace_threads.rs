use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};

use common::routing::ChannelTarget;
use common::workspace::manager::ExternalSessionAttachMode;
use common::workspace::threads::{ThreadRuntimeState, WorkspaceThreadId};

use crate::web_server::AppState;

pub async fn init_workspace_thread_handler(
    State(state): State<AppState>,
    Json(body): Json<crate::api_types::WorkspaceThreadInitRequest>,
) -> Result<Json<crate::api_types::WorkspaceThreadInitResponse>, (StatusCode, String)> {
    let agent_id = common::resources::resolve_agent_id(&body.agent_id)
        .map_err(|error| (StatusCode::BAD_REQUEST, error))?;
    let workspace = body
        .workspace_path
        .as_deref()
        .map(std::path::PathBuf::from)
        .map(common::workspace::normalize_workspace_cwd)
        .unwrap_or_else(|| common::config::ensure_loaded().resolve_workspace(&agent_id));
    let manager = state.channel_hub.workspace_thread_manager();
    let trimmed_session_id = body
        .session_id
        .as_deref()
        .map(str::trim)
        .filter(|session_id| !session_id.is_empty())
        .map(ToOwned::to_owned);
    let runtime = if let Some(session_id) = trimmed_session_id {
        manager
            .attach_external_session_to_web_thread(
                agent_id,
                body.profile_id,
                session_id,
                workspace,
                ExternalSessionAttachMode::ReuseOpenThread,
            )
            .await
    } else {
        manager
            .create_web_thread_for_cwd_with_host(agent_id, body.profile_id, workspace)
            .await
    }
    .map_err(|error| (StatusCode::BAD_REQUEST, error.to_string()))?;
    Ok(Json(workspace_thread_init_response(runtime.state().await)))
}

pub async fn fork_workspace_thread_handler(
    State(state): State<AppState>,
    Path(source_thread_id): Path<String>,
) -> Result<Json<crate::api_types::WorkspaceThreadInitResponse>, (StatusCode, String)> {
    let manager = state.channel_hub.workspace_thread_manager();
    let runtime = manager
        .fork_thread_to_web(&WorkspaceThreadId::from(source_thread_id))
        .await
        .map_err(|error| (StatusCode::BAD_REQUEST, error.to_string()))?;
    let before = runtime.state().await;
    let route = common::workspace::manager::web_route_for_thread(&before.thread_id);
    let expected_session_id = match before.session_id.clone() {
        Some(session_id) => session_id,
        None => {
            cleanup_failed_fork(&manager, &before.thread_id, &route).await;
            return Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                "forked task has no session".to_string(),
            ));
        }
    };
    let target = ChannelTarget::for_route(route.clone());
    let plugin_host = state.channel_hub.plugin_host();
    match common::channels::prompt::start_runtime_and_notify(
        &manager,
        &runtime,
        &plugin_host,
        &target,
        true,
    )
    .await
    {
        Ok(true) => {}
        Ok(false) => {
            cleanup_failed_fork(&manager, &before.thread_id, &route).await;
            return Err((
                StatusCode::BAD_REQUEST,
                "agent cannot start an ACP session".to_string(),
            ));
        }
        Err(error) => {
            cleanup_failed_fork(&manager, &before.thread_id, &route).await;
            return Err((StatusCode::BAD_REQUEST, error.message.to_string()));
        }
    }

    let after = runtime.state().await;
    if after.session_id.as_deref() != Some(expected_session_id.as_str()) {
        cleanup_failed_fork(&manager, &after.thread_id, &route).await;
        return Err((
            StatusCode::BAD_REQUEST,
            "forked session did not attach".to_string(),
        ));
    }
    Ok(Json(workspace_thread_init_response(after)))
}

async fn cleanup_failed_fork(
    manager: &std::sync::Arc<common::workspace::WorkspaceThreadManager>,
    thread_id: &WorkspaceThreadId,
    route: &common::routing::RouteKey,
) {
    let _ = manager
        .close_thread(
            thread_id,
            Some("forked session failed to attach".to_string()),
        )
        .await;
    let _ = manager.detach_route(route).await;
}

fn workspace_thread_init_response(
    state: ThreadRuntimeState,
) -> crate::api_types::WorkspaceThreadInitResponse {
    let thread_id = state.thread_id.to_string();
    let chat_id = common::workspace::manager::web_chat_id_for_thread(&state.thread_id);
    crate::api_types::WorkspaceThreadInitResponse {
        thread_id,
        chat_id,
        agent_id: state.host_binding.agent_id,
        profile_id: state.host_binding.profile_id,
        session_id: state.session_id,
        workspace: state.workspace.to_string_lossy().to_string(),
    }
}
