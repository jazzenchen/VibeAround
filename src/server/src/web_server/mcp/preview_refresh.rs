//! MCP request to refresh the owner Preview currently driving an agent turn.

use axum::Json;
use serde_json::Value;

use common::workspace::manager::preview_slug_from_web_route;
use common::workspace::threads::runtime::ThreadRuntime;
use common::workspace::threads::WorkspaceThreadId;

use crate::web_server::AppState;

use super::jsonrpc::{jsonrpc_err, mcp_error_text, mcp_text};
use super::session_identity::argument_string;

pub(super) async fn mcp_refresh_preview(
    id: Option<Value>,
    arguments: &Value,
    state: &AppState,
) -> Json<Value> {
    let Some(thread_id) = argument_string(arguments, "thread_id") else {
        return jsonrpc_err(id, -32602, "Missing required argument: thread_id");
    };
    let thread_id = WorkspaceThreadId::from(thread_id.as_str());
    let runtime = match state
        .channel_hub
        .workspace_thread_manager()
        .runtime_for_thread_id(&thread_id)
        .await
    {
        Ok(runtime) => runtime,
        Err(error) => {
            return mcp_error_text(
                id,
                &format!("Failed to load thread runtime {}: {error:#}", thread_id),
            );
        }
    };
    let slug = match active_preview_slug(&thread_id, &runtime) {
        Ok(slug) => slug,
        Err(message) => return mcp_error_text(id, message),
    };

    crate::web_server::preview::request_owner_refresh(&slug);
    mcp_text(id, "Refresh requested")
}

fn active_preview_slug(
    thread_id: &WorkspaceThreadId,
    runtime: &ThreadRuntime,
) -> Result<String, &'static str> {
    let target = runtime
        .active_turn_target()
        .current()
        .ok_or("refresh_preview can only be called during an active VibeAround Preview turn.")?;
    let slug = preview_slug_from_web_route(&target.route)
        .ok_or("refresh_preview can only be called during an active VibeAround Preview turn.")?;
    if common::previews::owner_conversation_thread_id(slug).as_ref() != Some(thread_id) {
        return Err("This task is no longer the active child for its owner Preview.");
    }
    Ok(slug.to_string())
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use common::routing::{ChannelTarget, RouteKey};
    use common::workspace::manager::{preview_web_route_for_slug, WorkspaceThreadManager};
    use uuid::Uuid;

    use super::*;

    fn temp_paths() -> (PathBuf, PathBuf, PathBuf) {
        let root = std::env::temp_dir().join(format!("va-preview-refresh-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        (
            root.join("workspaces.jsonl"),
            root.join("threads.jsonl"),
            root.join("attachments.jsonl"),
        )
    }

    #[tokio::test]
    async fn refresh_requires_the_active_latest_preview_child_turn() {
        let (workspaces, threads, attachments) = temp_paths();
        let manager = WorkspaceThreadManager::with_paths(workspaces, threads, attachments);
        let workspace =
            std::env::temp_dir().join(format!("va-preview-refresh-ws-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&workspace).unwrap();
        let file = workspace.join("README.md");
        std::fs::write(&file, "# Preview").unwrap();
        let parent = manager
            .create_web_thread_for_cwd_with_host(
                "codex".to_string(),
                Some("direct".to_string()),
                workspace.clone(),
            )
            .await
            .unwrap();
        let parent_id = parent.state().await.thread_id;
        let (slug, _) = common::previews::ensure_file(file, workspace, "Preview".to_string());
        let runtime = manager
            .ensure_preview_child_web_thread(&parent_id, &slug)
            .await
            .unwrap();
        let thread_id = runtime.state().await.thread_id;

        assert!(active_preview_slug(&thread_id, &runtime).is_err());
        {
            let active = runtime.active_turn_target();
            let _guard = active.install(ChannelTarget::for_route(RouteKey::new("web", "other")));
            assert!(active_preview_slug(&thread_id, &runtime).is_err());
        }
        {
            let active = runtime.active_turn_target();
            let _guard =
                active.install(ChannelTarget::for_route(preview_web_route_for_slug(&slug)));
            assert_eq!(active_preview_slug(&thread_id, &runtime).unwrap(), slug);

            common::previews::replace_owner_conversation(
                &slug,
                WorkspaceThreadId::from("wt_replacement"),
            )
            .unwrap();
            assert!(active_preview_slug(&thread_id, &runtime).is_err());
        }
    }
}
