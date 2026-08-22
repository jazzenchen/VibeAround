use uuid::Uuid;

use crate::workspace::threads::{
    MultiAgentTurnId, MultiAgentTurnMode, ThreadAgentId, ThreadAgentStatus,
};

use super::*;

#[cfg(windows)]
#[test]
fn normalize_platform_cwd_strips_windows_verbatim_prefixes() {
    assert_eq!(
        normalize_platform_cwd(PathBuf::from(r"\\?\D:\_P\26\test_VibeAround")),
        PathBuf::from(r"D:\_P\26\test_VibeAround")
    );
    assert_eq!(
        normalize_platform_cwd(PathBuf::from(r"\\?\UNC\server\share\test_VibeAround")),
        PathBuf::from(r"\\server\share\test_VibeAround")
    );
}

fn temp_paths() -> (PathBuf, PathBuf, PathBuf) {
    let root = std::env::temp_dir().join(format!("vibearound-wtm-{}", Uuid::new_v4()));
    (
        root.join("workspaces.jsonl"),
        root.join("threads.jsonl"),
        root.join("attachments.jsonl"),
    )
}

#[test]
fn route_defaults_derive_host_and_workspace_from_one_settings_snapshot() {
    let default_workspace = std::env::temp_dir().join("vibearound-snapshot-workspace");
    let settings = serde_json::json!({
        "default_workspace": default_workspace,
        "enabled_agents": ["claude", "codex"],
        "launcher": {
            "default_agent": "codex",
            "default_profile_id": "snapshot-profile"
        },
        "remote": {
            "channels": {
                "telegram": {
                    "agent_id": "claude",
                    "profile_id": "channel-profile"
                }
            }
        }
    });
    let cfg = crate::config::config_from_settings_json(&settings);
    let prefs = agent_state::prefs_from_settings_json(&settings);

    let (web_host, web_workspace) = default_route_binding_and_workspace_from_settings(
        &RouteKey::new("web", "chat-a"),
        &cfg,
        &prefs,
    );
    assert_eq!(web_host.agent_id, "codex");
    assert_eq!(web_host.profile_id.as_deref(), Some("snapshot-profile"));
    assert_eq!(web_workspace, default_workspace);

    let (telegram_host, telegram_workspace) = default_route_binding_and_workspace_from_settings(
        &RouteKey::new("telegram", "chat-a"),
        &cfg,
        &prefs,
    );
    assert_eq!(telegram_host.agent_id, "claude");
    assert_eq!(telegram_host.profile_id.as_deref(), Some("channel-profile"));
    assert_eq!(
        telegram_workspace,
        default_workspace.join("im").join("telegram")
    );
}

#[test]
fn switch_defaults_take_the_channel_profile_then_the_agent_default() {
    let settings = serde_json::json!({
        "default_workspace": std::env::temp_dir().join("vibearound-switch-workspace"),
        "enabled_agents": ["claude", "codex"],
        "launcher": {
            "default_agent": "codex",
            "default_profile_id": "launch-profile",
            "agents": {
                "claude": { "profile_id": "claude-profile" }
            }
        },
        "remote": {
            "channels": {
                "telegram": { "profile_id": "channel-profile" }
            }
        }
    });
    let cfg = crate::config::config_from_settings_json(&settings);
    let prefs = agent_state::prefs_from_settings_json(&settings);

    // A channel that pins a profile wins for whichever agent is switched to.
    assert_eq!(
        default_profile_for_agent_from_settings("telegram", "claude", &cfg, &prefs).as_deref(),
        Some("channel-profile")
    );
    // Otherwise the agent keeps its own launch-screen profile.
    assert_eq!(
        default_profile_for_agent_from_settings("feishu", "claude", &cfg, &prefs).as_deref(),
        Some("claude-profile")
    );
    // The launcher default agent inherits the launcher profile.
    assert_eq!(
        default_profile_for_agent_from_settings("feishu", "codex", &cfg, &prefs).as_deref(),
        Some("launch-profile")
    );
}

async fn seed_session_thread(
    manager: &WorkspaceThreadManager,
    root: PathBuf,
    agent_id: &str,
    profile_id: Option<&str>,
    session_id: &str,
    closed: bool,
) -> WorkspaceThreadId {
    let workspace = manager.ensure_workspace_for_cwd(root).await.unwrap();
    let profile_id = profile_id.map(ToOwned::to_owned);
    let host_binding = HostBinding::new(agent_id.to_string(), profile_id.clone());
    let thread = manager.new_thread_record_with_host(workspace.id.clone(), None, host_binding);
    manager.ensure_thread_persisted(&thread).await.unwrap();
    manager
        .thread_store
        .append(&ThreadEvent::agent_session_observed(
            thread.id.clone(),
            agent_id.to_string(),
            profile_id,
            session_id.to_string(),
        ))
        .await
        .unwrap();
    if closed {
        manager
            .thread_store
            .append(&ThreadEvent::closed(
                thread.id.clone(),
                Some("closed for test".to_string()),
            ))
            .await
            .unwrap();
    }
    thread.id
}

#[tokio::test]
async fn route_resolves_to_stable_thread_attachment() {
    let (workspaces, threads, attachments) = temp_paths();
    let manager = WorkspaceThreadManager::with_paths(workspaces, threads, attachments);
    let route = RouteKey::new("web", "chat-a");

    let first = manager.resolve_route_runtime(&route).await.unwrap();
    let second = manager.resolve_route_runtime(&route).await.unwrap();

    assert_eq!(
        first.state().await.thread_id,
        second.state().await.thread_id
    );
    assert_eq!(
        manager
            .current_attachment(&route)
            .await
            .unwrap()
            .unwrap()
            .workspace_id,
        WorkspaceId::general()
    );
}

#[tokio::test]
async fn explicit_new_reloads_im_channel_host_defaults() {
    let (workspaces, threads, attachments) = temp_paths();
    let manager = WorkspaceThreadManager::with_paths(workspaces, threads, attachments);
    let route = RouteKey::new("telegram", "chat-a");
    let workspace = manager
        .ensure_workspace_for_cwd(std::env::temp_dir())
        .await
        .unwrap();
    let old_host = HostBinding::new("stale-agent", Some("stale-profile".to_string()));
    manager
        .create_thread_for_route_with_host(&route, workspace.id.clone(), old_host)
        .await
        .unwrap();

    let expected_host = default_route_binding_and_workspace(&route).0;
    let runtime = manager
        .close_route_and_create_thread(&route, Some("test new".to_string()))
        .await
        .unwrap();
    let state = runtime.state().await;

    assert_eq!(state.workspace_id, workspace.id);
    assert_eq!(state.host_binding, expected_host);
    assert_ne!(state.host_binding.agent_id, "stale-agent");
}

#[tokio::test]
async fn explicit_new_preserves_web_selected_host() {
    let (workspaces, threads, attachments) = temp_paths();
    let manager = WorkspaceThreadManager::with_paths(workspaces, threads, attachments);
    let route = RouteKey::new("web", "chat-a");
    let workspace = manager
        .ensure_workspace_for_cwd(std::env::temp_dir())
        .await
        .unwrap();
    let selected_host = HostBinding::new("cursor", Some("direct".to_string()));
    manager
        .create_thread_for_route_with_host(&route, workspace.id.clone(), selected_host.clone())
        .await
        .unwrap();

    let runtime = manager
        .close_route_and_create_thread(&route, Some("test new".to_string()))
        .await
        .unwrap();
    let state = runtime.state().await;

    assert_eq!(state.workspace_id, workspace.id);
    assert_eq!(state.host_binding, selected_host);
}

async fn seed_preview_child(
    manager: &WorkspaceThreadManager,
    label: &str,
) -> (String, WorkspaceThreadId, WorkspaceThreadId, RouteKey) {
    let root = std::env::temp_dir().join(format!(
        "vibearound-preview-child-{label}-{}",
        Uuid::new_v4()
    ));
    std::fs::create_dir_all(&root).unwrap();
    let file = root.join("README.md");
    std::fs::write(&file, "# Preview").unwrap();
    let parent = manager
        .create_web_thread_for_cwd_with_host(
            "codex".to_string(),
            Some("direct".to_string()),
            root.clone(),
        )
        .await
        .unwrap();
    let parent_id = parent.state().await.thread_id;
    let (slug, _) = crate::previews::ensure_file(file, root, label.to_string());
    let child = manager
        .ensure_preview_web_thread(Some(&parent_id), &slug)
        .await
        .unwrap();
    let child_id = child.state().await.thread_id;
    let route = preview_web_route_for_slug(&slug);
    (slug, parent_id, child_id, route)
}

async fn seed_standalone_preview(
    manager: &WorkspaceThreadManager,
    label: &str,
) -> (String, WorkspaceThreadId, RouteKey, PathBuf) {
    let root = std::env::temp_dir().join(format!(
        "vibearound-preview-standalone-{label}-{}",
        Uuid::new_v4()
    ));
    std::fs::create_dir_all(&root).unwrap();
    let file = root.join("README.md");
    std::fs::write(&file, "# Standalone Preview").unwrap();
    let (slug, _) = crate::previews::ensure_file(file, root.clone(), label.to_string());
    let runtime = manager
        .ensure_preview_web_thread(None, &slug)
        .await
        .unwrap();
    let thread_id = runtime.state().await.thread_id;
    let route = preview_web_route_for_slug(&slug);
    (slug, thread_id, route, root)
}

#[tokio::test]
async fn preview_without_parent_creates_and_reuses_a_standalone_task() {
    let (workspaces, threads, attachments) = temp_paths();
    let manager = WorkspaceThreadManager::with_paths(workspaces, threads, attachments);
    let (slug, thread_id, route, root) = seed_standalone_preview(&manager, "root").await;

    let thread = manager.thread(&thread_id).await.unwrap().unwrap();
    assert_eq!(thread.parent_thread_id, None);
    assert_eq!(thread.preview_slug.as_deref(), Some(slug.as_str()));
    assert_eq!(
        manager
            .current_attachment(&route)
            .await
            .unwrap()
            .unwrap()
            .thread_id,
        thread_id
    );
    assert_eq!(
        manager
            .runtime_for_thread_id(&thread_id)
            .await
            .unwrap()
            .state()
            .await
            .workspace,
        normalize_workspace_cwd(root)
    );

    let reused = manager
        .ensure_preview_web_thread(None, &slug)
        .await
        .unwrap();
    assert_eq!(reused.state().await.thread_id, thread_id);
}

#[tokio::test]
async fn standalone_preview_new_close_and_next_message_keep_the_slug() {
    let (workspaces, threads, attachments) = temp_paths();
    let manager = WorkspaceThreadManager::with_paths(workspaces, threads, attachments);
    let (slug, first_id, route, _) = seed_standalone_preview(&manager, "commands").await;

    let second = manager
        .close_route_and_create_thread(&route, Some("test /new".to_string()))
        .await
        .unwrap();
    let second_id = second.state().await.thread_id;
    let second_thread = manager.thread(&second_id).await.unwrap().unwrap();
    assert_ne!(second_id, first_id);
    assert_eq!(second_thread.parent_thread_id, None);
    assert_eq!(second_thread.preview_slug.as_deref(), Some(slug.as_str()));

    manager
        .close_route(&route, Some("test /close".to_string()))
        .await
        .unwrap();
    let third = manager.resolve_route_runtime(&route).await.unwrap();
    let third_id = third.state().await.thread_id;
    let third_thread = manager.thread(&third_id).await.unwrap().unwrap();
    assert_ne!(third_id, second_id);
    assert_eq!(third_thread.parent_thread_id, None);
    assert_eq!(third_thread.preview_slug.as_deref(), Some(slug.as_str()));
}

#[tokio::test]
async fn preview_child_reuses_global_slug_across_parents_and_reload() {
    let (workspaces, threads, attachments) = temp_paths();
    let manager = WorkspaceThreadManager::with_paths(
        workspaces.clone(),
        threads.clone(),
        attachments.clone(),
    );
    let (slug, parent_id, child_id, route) = seed_preview_child(&manager, "reload").await;
    let child = manager.thread(&child_id).await.unwrap().unwrap();
    assert_eq!(child.parent_thread_id.as_ref(), Some(&parent_id));
    assert_eq!(child.preview_slug.as_deref(), Some(slug.as_str()));
    assert_eq!(
        manager
            .current_attachment(&route)
            .await
            .unwrap()
            .unwrap()
            .thread_id,
        child_id
    );

    let preview = crate::previews::lookup_owner(&slug).unwrap();
    let other_parent = manager
        .create_web_thread_for_cwd_with_host(
            "claude".to_string(),
            Some("direct".to_string()),
            preview.workspace.clone(),
        )
        .await
        .unwrap();
    let other_parent_id = other_parent.state().await.thread_id;
    let reused = manager
        .ensure_preview_web_thread(Some(&other_parent_id), &slug)
        .await
        .unwrap();
    assert_eq!(reused.state().await.thread_id, child_id);
    assert_eq!(
        manager
            .thread(&child_id)
            .await
            .unwrap()
            .unwrap()
            .parent_thread_id,
        Some(parent_id.clone())
    );

    assert!(crate::previews::delete_session(&slug));
    let (recreated_slug, _) =
        crate::previews::ensure_file(preview.id, preview.workspace, preview.title);
    assert_eq!(recreated_slug, slug);
    drop(manager);
    let reloaded = WorkspaceThreadManager::with_paths(workspaces, threads, attachments);
    let runtime = reloaded
        .ensure_preview_web_thread(Some(&other_parent_id), &slug)
        .await
        .unwrap();

    assert_eq!(runtime.state().await.thread_id, child_id);
    assert_eq!(
        reloaded
            .thread(&child_id)
            .await
            .unwrap()
            .unwrap()
            .parent_thread_id,
        Some(parent_id)
    );
}

#[tokio::test]
async fn concurrent_preview_ensure_creates_one_standalone_task_for_the_slug() {
    let (workspaces, threads, attachments) = temp_paths();
    let manager = WorkspaceThreadManager::with_paths(workspaces, threads, attachments);
    let root = std::env::temp_dir().join(format!(
        "vibearound-preview-child-concurrent-{}",
        Uuid::new_v4()
    ));
    std::fs::create_dir_all(&root).unwrap();
    let file = root.join("README.md");
    std::fs::write(&file, "# Preview").unwrap();
    let (slug, _) = crate::previews::ensure_file(file, root, "concurrent".to_string());

    let (first, second) = tokio::join!(
        manager.ensure_preview_web_thread(None, &slug),
        manager.ensure_preview_web_thread(None, &slug),
    );
    let first_id = first.unwrap().state().await.thread_id;
    let second_id = second.unwrap().state().await.thread_id;

    assert_eq!(first_id, second_id);
    assert_eq!(
        manager
            .thread_projection()
            .await
            .unwrap()
            .all()
            .filter(|thread| thread.preview_slug.as_deref() == Some(slug.as_str()))
            .count(),
        1
    );
}

#[tokio::test]
async fn preview_new_close_and_next_message_keep_parent_and_slug() {
    let (workspaces, threads, attachments) = temp_paths();
    let manager = WorkspaceThreadManager::with_paths(workspaces, threads, attachments);
    let (slug, parent_id, first_id, route) = seed_preview_child(&manager, "commands").await;

    let second = manager
        .close_route_and_create_thread(&route, Some("test /new".to_string()))
        .await
        .unwrap();
    let second_id = second.state().await.thread_id;
    let second_thread = manager.thread(&second_id).await.unwrap().unwrap();
    assert_ne!(second_id, first_id);
    assert_eq!(second_thread.parent_thread_id.as_ref(), Some(&parent_id));
    assert_eq!(second_thread.preview_slug.as_deref(), Some(slug.as_str()));
    manager
        .close_route(&route, Some("test /close".to_string()))
        .await
        .unwrap();
    assert!(manager.current_attachment(&route).await.unwrap().is_none());

    let third = manager.resolve_route_runtime(&route).await.unwrap();
    let third_id = third.state().await.thread_id;
    let third_thread = manager.thread(&third_id).await.unwrap().unwrap();
    assert_ne!(third_id, second_id);
    assert_eq!(third_thread.parent_thread_id.as_ref(), Some(&parent_id));
    assert_eq!(third_thread.preview_slug.as_deref(), Some(slug.as_str()));
}

#[tokio::test]
async fn preview_host_switch_keeps_the_same_child_and_route() {
    let (workspaces, threads, attachments) = temp_paths();
    let manager = WorkspaceThreadManager::with_paths(
        workspaces.clone(),
        threads.clone(),
        attachments.clone(),
    );
    let (slug, parent_id, child_id, route) = seed_preview_child(&manager, "switch").await;
    let runtime = manager.resolve_route_runtime(&route).await.unwrap();

    runtime
        .switch_host_replacing_session(HostBinding::new("claude", Some("direct".to_string())))
        .await
        .unwrap();

    let state = runtime.state().await;
    assert_eq!(state.thread_id, child_id);
    assert_eq!(state.host_binding.agent_id, "claude");
    let child = manager.thread(&child_id).await.unwrap().unwrap();
    assert_eq!(child.parent_thread_id.as_ref(), Some(&parent_id));
    assert_eq!(child.preview_slug.as_deref(), Some(slug.as_str()));
    assert_eq!(
        manager
            .current_attachment(&route)
            .await
            .unwrap()
            .unwrap()
            .thread_id,
        child_id
    );
    drop(runtime);
    drop(manager);
    let reloaded = WorkspaceThreadManager::with_paths(workspaces, threads, attachments);
    let reloaded_runtime = reloaded.resolve_route_runtime(&route).await.unwrap();
    let reloaded_state = reloaded_runtime.state().await;
    assert_eq!(reloaded_state.thread_id, child_id);
    assert_eq!(reloaded_state.host_binding.agent_id, "claude");
    assert_eq!(reloaded_state.session_id, None);
}

#[tokio::test]
async fn active_runtime_resolve_does_not_reload_thread_store() {
    let (workspaces, threads, attachments) = temp_paths();
    let manager = WorkspaceThreadManager::with_paths(workspaces, threads, attachments);
    let route = RouteKey::new("feishu", "chat-a");

    let first = manager.resolve_route_runtime(&route).await.unwrap();
    let first_thread_id = first.state().await.thread_id;
    tokio::fs::write(manager.thread_store.path(), b"not valid jsonl\n")
        .await
        .unwrap();

    let second = manager.resolve_route_runtime(&route).await.unwrap();

    assert!(Arc::ptr_eq(&first, &second));
    assert_eq!(second.state().await.thread_id, first_thread_id);
}

#[tokio::test]
async fn cancel_unattached_route_is_noop() {
    let (workspaces, threads, attachments) = temp_paths();
    let manager = WorkspaceThreadManager::with_paths(workspaces, threads, attachments);
    let route = RouteKey::new("web", "chat-a");

    let cancelled = manager.cancel_route(&route).await.unwrap();

    assert!(!cancelled);
    assert!(manager.current_attachment(&route).await.unwrap().is_none());
    assert!(manager
        .workspace_store
        .read_events()
        .await
        .unwrap()
        .is_empty());
    assert!(manager.thread_store.read_events().await.unwrap().is_empty());
    assert!(manager
        .attachment_store
        .read_events()
        .await
        .unwrap()
        .is_empty());
}

#[tokio::test]
async fn channel_routes_get_route_private_threads() {
    let (workspaces, threads, attachments) = temp_paths();
    let manager = WorkspaceThreadManager::with_paths(workspaces, threads, attachments);
    let first_route = RouteKey::new("feishu", "chat-a");
    let second_route = RouteKey::new("feishu", "chat-b");

    let first = manager.resolve_route_runtime(&first_route).await.unwrap();
    let second = manager.resolve_route_runtime(&second_route).await.unwrap();

    assert_ne!(
        first.state().await.thread_id,
        second.state().await.thread_id
    );
    assert_eq!(
        manager
            .current_attachment(&first_route)
            .await
            .unwrap()
            .unwrap()
            .thread_id,
        first.state().await.thread_id
    );
}

#[tokio::test]
async fn base_and_topic_routes_get_different_threads() {
    let (workspaces, threads, attachments) = temp_paths();
    let manager = WorkspaceThreadManager::with_paths(workspaces, threads, attachments);
    let base_route = RouteKey::new("slack", "chat-a");
    let base_runtime = manager.resolve_route_runtime(&base_route).await.unwrap();
    let base_thread_id = base_runtime.state().await.thread_id;
    let topic_route = RouteKey::with_actor(
        "slack",
        "U_REAL_BOT",
        "chat-a",
        "slack",
        Some("thread-1".to_string()),
    );

    let topic_runtime = manager.resolve_route_runtime(&topic_route).await.unwrap();

    assert_ne!(topic_runtime.state().await.thread_id, base_thread_id);
    assert_eq!(
        manager
            .current_attachment(&base_route)
            .await
            .unwrap()
            .unwrap()
            .thread_id,
        base_thread_id
    );
}

#[tokio::test]
async fn different_channels_get_different_default_threads() {
    let (workspaces, threads, attachments) = temp_paths();
    let manager = WorkspaceThreadManager::with_paths(workspaces, threads, attachments);
    let feishu = RouteKey::new("feishu", "chat-a");
    let slack = RouteKey::new("slack", "chat-a");

    let feishu_runtime = manager.resolve_route_runtime(&feishu).await.unwrap();
    let slack_runtime = manager.resolve_route_runtime(&slack).await.unwrap();

    assert_ne!(
        feishu_runtime.state().await.thread_id,
        slack_runtime.state().await.thread_id
    );
}

#[tokio::test]
async fn im_route_attachment_retains_runtime_after_host_shutdown() {
    route_attachment_retains_runtime_after_host_shutdown("feishu").await;
}

#[tokio::test]
async fn web_route_attachment_retains_runtime_after_host_shutdown() {
    route_attachment_retains_runtime_after_host_shutdown("web").await;
}

#[tokio::test]
async fn tui_route_attachment_retains_runtime_after_host_shutdown() {
    route_attachment_retains_runtime_after_host_shutdown("tui").await;
}

async fn route_attachment_retains_runtime_after_host_shutdown(channel_kind: &str) {
    let (workspaces, threads, attachments) = temp_paths();
    let manager = WorkspaceThreadManager::with_paths(workspaces, threads, attachments);
    let route = RouteKey::new(channel_kind, "chat-a");
    let first = manager.resolve_route_runtime(&route).await.unwrap();
    let first_thread_id = first.state().await.thread_id;

    manager.shutdown_route_host(&route).await.unwrap();
    assert!(manager.runtimes.get(&first_thread_id).await.is_some());
    assert_eq!(
        manager
            .current_attachment(&route)
            .await
            .unwrap()
            .unwrap()
            .thread_id,
        first_thread_id
    );

    let second = manager.resolve_route_runtime(&route).await.unwrap();

    assert_eq!(second.state().await.thread_id, first_thread_id);
    assert!(Arc::ptr_eq(&first, &second));
    assert!(manager.runtimes.get(&first_thread_id).await.is_some());
}

#[tokio::test]
async fn detach_route_keeps_thread_open() {
    let (workspaces, threads, attachments) = temp_paths();
    let manager = WorkspaceThreadManager::with_paths(workspaces, threads, attachments);
    let route = RouteKey::new("web", "chat-a");

    let runtime = manager.resolve_route_runtime(&route).await.unwrap();
    let thread_id = runtime.state().await.thread_id;
    manager.detach_route(&route).await.unwrap();

    assert!(manager.current_attachment(&route).await.unwrap().is_none());
    assert_eq!(
        manager.thread(&thread_id).await.unwrap().unwrap().status,
        crate::workspace::threads::store::ThreadStatus::Open
    );
}

#[tokio::test]
async fn host_start_reset_keeps_other_routes_on_shared_thread() {
    let (workspaces, threads, attachments) = temp_paths();
    let manager = WorkspaceThreadManager::with_paths(workspaces, threads, attachments);
    let stale_route = RouteKey::new("qqbot", "chat-old");
    let current_route = RouteKey::new("feishu", "chat-new");

    let runtime = manager.resolve_route_runtime(&stale_route).await.unwrap();
    let thread_id = runtime.state().await.thread_id;
    manager
        .attach_thread(&current_route, &thread_id)
        .await
        .unwrap();

    manager
        .reset_thread_attachments_for_host_start(&thread_id, Some(&current_route))
        .await
        .unwrap();

    let mut routes = manager
        .attached_routes_for_thread(&thread_id)
        .await
        .unwrap();
    routes.sort_by_key(|route| route.display_key());

    assert_eq!(routes, vec![current_route, stale_route]);
}

#[tokio::test]
async fn runtime_entries_do_not_materialize_unstarted_threads() {
    let (workspaces, threads, attachments) = temp_paths();
    let manager = WorkspaceThreadManager::with_paths(workspaces, threads, attachments);
    let route = RouteKey::new("web", "chat-a");

    let runtime = manager.resolve_route_runtime(&route).await.unwrap();
    let thread_id = runtime.state().await.thread_id;

    assert!(manager.runtime_entries().await.unwrap().is_empty());
    assert_eq!(
        manager.thread(&thread_id).await.unwrap().unwrap().status,
        crate::workspace::threads::store::ThreadStatus::Open
    );
}

#[tokio::test]
async fn close_route_detaches_closed_thread() {
    let (workspaces, threads, attachments) = temp_paths();
    let manager = WorkspaceThreadManager::with_paths(workspaces, threads, attachments);
    let route = RouteKey::new("slack", "chat-a");

    let runtime = manager.resolve_route_runtime(&route).await.unwrap();
    let thread_id = runtime.state().await.thread_id;

    manager
        .close_route(&route, Some("user closed".to_string()))
        .await
        .unwrap();

    assert!(manager.current_attachment(&route).await.unwrap().is_none());
    assert_eq!(
        manager.thread(&thread_id).await.unwrap().unwrap().status,
        crate::workspace::threads::store::ThreadStatus::Closed
    );
}

#[tokio::test]
async fn switch_workspace_registers_existing_directory_path() {
    let (workspaces, threads, attachments) = temp_paths();
    let manager = WorkspaceThreadManager::with_paths(workspaces, threads, attachments);
    let root = std::env::temp_dir().join(format!("vibearound-ws-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&root).unwrap();
    let route = RouteKey::new("slack", "chat-a");

    let runtime = manager
        .switch_workspace(&route, root.to_str().unwrap())
        .await
        .unwrap();

    assert_eq!(
        runtime.state().await.workspace,
        normalize_workspace_cwd(&root)
    );
    assert!(manager
        .workspace_projection()
        .await
        .unwrap()
        .get_by_cwd(&normalize_workspace_cwd(&root))
        .is_some());
}

#[tokio::test]
async fn attach_external_session_normalizes_workspace_cwd() {
    let (workspaces, threads, attachments) = temp_paths();
    let manager = WorkspaceThreadManager::with_paths(workspaces, threads, attachments);
    let root = std::env::temp_dir().join(format!("vibearound-ws-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&root).unwrap();
    let route = RouteKey::new("web", "chat-a");
    let thread_id = seed_session_thread(
        &manager,
        root.clone(),
        "codex",
        Some("direct"),
        "session-1",
        false,
    )
    .await;

    let runtime = manager
        .attach_external_session(
            &route,
            "codex".to_string(),
            Some("direct".to_string()),
            "session-1".to_string(),
            root.join("."),
            ExternalSessionAttachMode::ReuseOpenThread,
        )
        .await
        .unwrap();

    assert_eq!(
        runtime.state().await.workspace,
        normalize_workspace_cwd(&root)
    );
    assert_eq!(runtime.state().await.thread_id, thread_id);
    assert!(manager
        .workspace_projection()
        .await
        .unwrap()
        .get_by_cwd(&normalize_workspace_cwd(&root))
        .is_some());
}

#[tokio::test]
async fn attach_external_session_keeps_missing_profile_unknown() {
    let (workspaces, threads, attachments) = temp_paths();
    let manager = WorkspaceThreadManager::with_paths(workspaces, threads, attachments);
    let root = std::env::temp_dir().join(format!("vibearound-ws-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&root).unwrap();
    let route = RouteKey::new("feishu", "chat-a");
    let agent_id = format!("test-agent-{}", Uuid::new_v4());
    seed_session_thread(
        &manager,
        root.clone(),
        &agent_id,
        None,
        "external-session",
        false,
    )
    .await;

    let runtime = manager
        .attach_external_session(
            &route,
            agent_id.clone(),
            None,
            "external-session".to_string(),
            root,
            ExternalSessionAttachMode::ReuseOpenThread,
        )
        .await
        .unwrap();

    let state = runtime.state().await;
    assert_eq!(state.host_binding, HostBinding::new(agent_id, None));
    assert_eq!(state.session_id.as_deref(), Some("external-session"));
}

#[tokio::test]
async fn attach_external_session_preserves_known_profile_when_missing() {
    let (workspaces, threads, attachments) = temp_paths();
    let manager = WorkspaceThreadManager::with_paths(workspaces, threads, attachments);
    let root = std::env::temp_dir().join(format!("vibearound-ws-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&root).unwrap();
    let route = RouteKey::new("web", "chat-a");
    let thread_id = seed_session_thread(
        &manager,
        root.clone(),
        "claude",
        Some("deepseek-profile"),
        "external-session",
        false,
    )
    .await;

    let runtime = manager
        .attach_external_session(
            &route,
            "claude".to_string(),
            None,
            "external-session".to_string(),
            root,
            ExternalSessionAttachMode::ReuseOpenThread,
        )
        .await
        .unwrap();

    let state = runtime.state().await;
    assert_eq!(state.thread_id, thread_id);
    assert_eq!(
        state.host_binding,
        HostBinding::new("claude", Some("deepseek-profile".to_string()))
    );
}

#[tokio::test]
async fn attach_external_session_rejects_unknown_session() {
    let (workspaces, threads, attachments) = temp_paths();
    let manager = WorkspaceThreadManager::with_paths(workspaces, threads, attachments);
    let root = std::env::temp_dir().join(format!("vibearound-ws-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&root).unwrap();
    let route = RouteKey::new("feishu", "chat-a");

    let error = match manager
        .attach_external_session(
            &route,
            "codex".to_string(),
            Some("direct".to_string()),
            "missing-session".to_string(),
            root,
            ExternalSessionAttachMode::NewThread,
        )
        .await
    {
        Ok(_) => panic!("unknown session should be rejected"),
        Err(error) => error,
    };

    assert!(error.to_string().contains("missing-session"));
    assert!(manager.current_attachment(&route).await.unwrap().is_none());
}

#[tokio::test]
async fn attach_external_session_reuses_existing_open_thread() {
    let (workspaces, threads, attachments) = temp_paths();
    let manager = WorkspaceThreadManager::with_paths(workspaces, threads, attachments);
    let root = std::env::temp_dir().join(format!("vibearound-ws-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&root).unwrap();
    let web_route = RouteKey::new("web", "chat-a");
    let im_route = RouteKey::new("feishu", "chat-a");
    let thread_id = seed_session_thread(
        &manager,
        root.clone(),
        "codex",
        Some("direct"),
        "session-picked-up",
        false,
    )
    .await;
    manager.attach_thread(&web_route, &thread_id).await.unwrap();

    let runtime = manager
        .attach_external_session(
            &im_route,
            "codex".to_string(),
            Some("direct".to_string()),
            "session-picked-up".to_string(),
            root,
            ExternalSessionAttachMode::ReuseOpenThread,
        )
        .await
        .unwrap();

    assert_eq!(runtime.state().await.thread_id, thread_id);
    let mut routes = manager
        .attached_routes_for_thread(&thread_id)
        .await
        .unwrap();
    routes.sort_by_key(|route| route.display_key());
    assert_eq!(routes, vec![im_route, web_route]);
}

#[tokio::test]
async fn attach_external_session_resolves_workspace_thread_id_to_host_session() {
    let (workspaces, threads, attachments) = temp_paths();
    let manager = WorkspaceThreadManager::with_paths(workspaces, threads, attachments);
    let root = std::env::temp_dir().join(format!("vibearound-ws-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&root).unwrap();
    let route = RouteKey::new("web", "chat-a");
    let thread_id = seed_session_thread(
        &manager,
        root.clone(),
        "codex",
        Some("direct"),
        "native-session",
        false,
    )
    .await;

    let runtime = manager
        .attach_external_session(
            &route,
            "codex".to_string(),
            Some("direct".to_string()),
            thread_id.to_string(),
            root,
            ExternalSessionAttachMode::ReuseOpenThread,
        )
        .await
        .unwrap();

    let state = runtime.state().await;
    assert_eq!(state.thread_id, thread_id);
    assert_eq!(state.session_id.as_deref(), Some("native-session"));
}

#[tokio::test]
async fn subagent_session_ids_for_agent_workspace_reads_thread_agents() {
    let (workspaces, threads, attachments) = temp_paths();
    let manager = WorkspaceThreadManager::with_paths(workspaces, threads, attachments);
    let root = std::env::temp_dir().join(format!("vibearound-ws-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&root).unwrap();
    let workspace = manager
        .ensure_workspace_for_cwd(root.clone())
        .await
        .unwrap();
    let host_binding = HostBinding::new("codex", Some("direct".to_string()));
    let thread = manager.new_thread_record_with_host(workspace.id.clone(), None, host_binding);
    manager.ensure_thread_persisted(&thread).await.unwrap();

    let turn_id = MultiAgentTurnId::from("mat_a");
    let agent_id = ThreadAgentId::from("agent_a");
    let turn = MultiAgentTurn::new(
        turn_id.clone(),
        MultiAgentTurnMode::Parallel,
        vec![agent_id.clone()],
    );
    let agent = ThreadAgent::ready(
        agent_id.clone(),
        turn_id,
        "Builder",
        "codex",
        Some("direct".to_string()),
        "va/subagents/mat_a/builder",
        root.join("builder").to_string_lossy().to_string(),
        Some("build".to_string()),
    );
    manager
        .thread_store
        .append(&ThreadEvent::multi_agent_turn_initialized(
            thread.id.clone(),
            turn,
            vec![agent],
        ))
        .await
        .unwrap();
    manager
        .thread_store
        .append(&ThreadEvent::thread_agent_status_changed_with_session(
            thread.id,
            agent_id,
            ThreadAgentStatus::Running,
            Some("subagent-native-session".to_string()),
            None,
            None,
        ))
        .await
        .unwrap();

    let session_ids = manager
        .subagent_session_ids_for_agent_workspace("codex", &root)
        .await
        .unwrap();

    assert!(session_ids.contains("subagent-native-session"));
}

#[tokio::test]
async fn attach_external_session_new_thread_mode_does_not_reuse_open_thread() {
    let (workspaces, threads, attachments) = temp_paths();
    let manager = WorkspaceThreadManager::with_paths(workspaces, threads, attachments);
    let root = std::env::temp_dir().join(format!("vibearound-ws-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&root).unwrap();
    let existing_route = RouteKey::new("web", "chat-a");
    let switch_route = RouteKey::new("feishu", "chat-a");
    let existing_thread_id = seed_session_thread(
        &manager,
        root.clone(),
        "codex",
        Some("direct"),
        "session-switch",
        false,
    )
    .await;
    manager
        .attach_thread(&existing_route, &existing_thread_id)
        .await
        .unwrap();

    let runtime = manager
        .attach_external_session(
            &switch_route,
            "codex".to_string(),
            Some("direct".to_string()),
            "session-switch".to_string(),
            root,
            ExternalSessionAttachMode::NewThread,
        )
        .await
        .unwrap();

    assert_ne!(runtime.state().await.thread_id, existing_thread_id);
    assert_eq!(
        manager
            .current_attachment(&existing_route)
            .await
            .unwrap()
            .unwrap()
            .thread_id,
        existing_thread_id
    );
    assert_eq!(
        manager
            .current_attachment(&switch_route)
            .await
            .unwrap()
            .unwrap()
            .thread_id,
        runtime.state().await.thread_id
    );
}

#[tokio::test]
async fn attach_external_session_creates_open_thread_when_matching_thread_is_closed() {
    let (workspaces, threads, attachments) = temp_paths();
    let manager = WorkspaceThreadManager::with_paths(workspaces, threads, attachments);
    let root = std::env::temp_dir().join(format!("vibearound-ws-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&root).unwrap();
    let second_route = RouteKey::new("web", "chat-b");
    let thread_id = seed_session_thread(
        &manager,
        root.clone(),
        "codex",
        Some("direct"),
        "session-closed",
        true,
    )
    .await;

    let second = manager
        .attach_external_session(
            &second_route,
            "codex".to_string(),
            Some("direct".to_string()),
            "session-closed".to_string(),
            root,
            ExternalSessionAttachMode::ReuseOpenThread,
        )
        .await
        .unwrap();

    let second_thread_id = second.state().await.thread_id;
    assert_ne!(second_thread_id, thread_id);
    assert_eq!(
        manager.thread(&thread_id).await.unwrap().unwrap().status,
        ThreadStatus::Closed
    );
    let second_thread = manager
        .thread(&second_thread_id)
        .await
        .unwrap()
        .expect("second thread should exist");
    assert_eq!(second_thread.status, ThreadStatus::Open);
    assert!(second_thread.has_agent_session(
        &HostBinding::new("codex", Some("direct".to_string())),
        "session-closed"
    ));
    assert_eq!(
        second.state().await.session_id.as_deref(),
        Some("session-closed")
    );
    assert_eq!(
        manager
            .current_attachment(&second_route)
            .await
            .unwrap()
            .unwrap()
            .thread_id,
        second_thread_id
    );
}

#[tokio::test]
async fn create_thread_for_cwd_starts_new_thread_even_when_workspace_has_threads() {
    let (workspaces, threads, attachments) = temp_paths();
    let manager = WorkspaceThreadManager::with_paths(workspaces, threads, attachments);
    let root = std::env::temp_dir().join(format!("vibearound-ws-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&root).unwrap();
    let first_route = RouteKey::new("web", "chat-a");
    let second_route = RouteKey::new("web", "chat-b");

    let first = manager
        .create_thread_for_cwd(&first_route, root.clone())
        .await
        .unwrap();
    let second = manager
        .create_thread_for_cwd(&second_route, root.clone())
        .await
        .unwrap();

    assert_ne!(
        first.state().await.thread_id,
        second.state().await.thread_id
    );
    assert_eq!(
        first.state().await.workspace_id,
        second.state().await.workspace_id
    );
    assert_eq!(
        second.state().await.workspace,
        normalize_workspace_cwd(&root)
    );
    assert_eq!(
        manager
            .current_attachment(&second_route)
            .await
            .unwrap()
            .unwrap()
            .thread_id,
        second.state().await.thread_id
    );
}

#[tokio::test]
async fn switch_workspace_starts_new_thread_when_workspace_has_threads() {
    let (workspaces, threads, attachments) = temp_paths();
    let manager = WorkspaceThreadManager::with_paths(workspaces, threads, attachments);
    let root = std::env::temp_dir().join(format!("vibearound-ws-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&root).unwrap();
    let route = RouteKey::new("feishu", "chat-a");

    let first_runtime = manager
        .switch_workspace(&route, root.to_str().unwrap())
        .await
        .unwrap();
    let second_runtime = manager
        .create_thread_in_current_workspace(&route)
        .await
        .unwrap();

    let third_runtime = manager
        .switch_workspace(&route, root.to_str().unwrap())
        .await
        .unwrap();

    assert_ne!(
        first_runtime.state().await.thread_id,
        third_runtime.state().await.thread_id
    );
    assert_ne!(
        second_runtime.state().await.thread_id,
        third_runtime.state().await.thread_id
    );
    assert_eq!(
        manager
            .current_attachment(&route)
            .await
            .unwrap()
            .unwrap()
            .thread_id,
        third_runtime.state().await.thread_id
    );
}
