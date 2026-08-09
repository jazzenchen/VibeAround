//! MCP `tools/call` implementations.
//!
//! Each tool takes the JSON-RPC id + arguments, validates inputs, touches the
//! relevant workspace config / preview store / session files, and returns a
//! JSON-RPC response.
//!
//! These handlers do not touch agent processes directly. Multi-agent runtime
//! behavior lives in the sibling `subagents` module.

use std::path::{Path, PathBuf};

use agent_client_protocol::schema::v1 as acp;
use anyhow::{anyhow, Context};
use axum::Json;
use serde_json::Value;

use crate::web_server::AppState;

use super::jsonrpc::{jsonrpc_err, mcp_error_text, mcp_text};
use super::session_identity::{argument_string, codex_session_id_from_mcp_metadata};
use super::sessions::find_latest_session;

// ---------------------------------------------------------------------------
// get_session_id — resolve the current ACP session ID from route info
// ---------------------------------------------------------------------------

pub(super) async fn mcp_get_session_id(
    id: Option<serde_json::Value>,
    arguments: &serde_json::Value,
    metadata: Option<&serde_json::Value>,
    state: &AppState,
) -> Json<serde_json::Value> {
    let agent_kind = argument_string(arguments, "agent_kind")
        .or_else(|| argument_string(arguments, "agent_type"));

    if let Some(session_id) = argument_string(arguments, "session_id") {
        record_mcp_session_observation(arguments, agent_kind.as_deref(), &session_id, "argument");
        return mcp_text(id, &session_id);
    }

    let channel_kind = argument_string(arguments, "channel_kind");
    let chat_id = argument_string(arguments, "chat_id");
    if let (Some(channel_kind), Some(chat_id)) = (channel_kind.as_deref(), chat_id.as_deref()) {
        if let Some(session_id) = session_id_from_route(channel_kind, chat_id, state).await {
            record_mcp_session_observation(arguments, agent_kind.as_deref(), &session_id, "route");
            return mcp_text(id, &session_id);
        }
        return mcp_error_text(
            id,
            "No active session found for this route. The agent session may not have started yet.",
        );
    }

    if agent_kind.as_deref() == Some("codex") {
        if let Some(session_id) = codex_session_id_from_mcp_metadata(metadata) {
            record_mcp_session_observation(
                arguments,
                agent_kind.as_deref(),
                &session_id,
                "codex-mcp-metadata",
            );
            return mcp_text(id, &session_id);
        }
        return mcp_error_text(
            id,
            "Codex did not provide a session ID in MCP metadata. Retry from Codex, or pass session_id explicitly.",
        );
    }

    if let (Some(agent_kind), Some(cwd)) =
        (agent_kind.as_deref(), argument_string(arguments, "cwd"))
    {
        let cwd = common::workspace::normalize_workspace_cwd(PathBuf::from(cwd));
        if let Some(session) = find_latest_session(agent_kind, &cwd) {
            record_mcp_session_observation(arguments, Some(agent_kind), &session, "auto-discovery");
            return mcp_text(id, &session);
        }
    }

    match agent_kind.as_deref() {
        Some(other) => mcp_error_text(
            id,
            &format!(
                "Could not resolve session ID for agent_kind '{}'. Pass session_id explicitly or provide channel_kind/chat_id for a VibeAround-managed session.",
                other
            ),
        ),
        None => jsonrpc_err(
            id,
            -32602,
            "Missing required arguments: provide session_id, channel_kind/chat_id, or agent_kind",
        ),
    }
}

async fn session_id_from_route(
    channel_kind: &str,
    chat_id: &str,
    state: &AppState,
) -> Option<String> {
    let route = common::routing::RouteKey::new(channel_kind, chat_id);
    let state_opt = state
        .channel_hub
        .workspace_thread_manager()
        .resolve_route_runtime(&route)
        .await
        .ok()
        .map(|runtime| async move { runtime.state().await });
    let state_opt = match state_opt {
        Some(state) => Some(state.await),
        None => None,
    };
    match state_opt {
        Some(snapshot) => snapshot.session_id,
        None => None,
    }
}

fn record_mcp_session_observation(
    arguments: &Value,
    agent_kind: Option<&str>,
    session_id: &str,
    source: &str,
) {
    let fallback_agent_kind = argument_string(arguments, "launch_target")
        .or_else(|| argument_string(arguments, "launchTarget"));
    let agent_kind = agent_kind
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .or(fallback_agent_kind.as_deref());
    let Some(agent_kind) = agent_kind else {
        return;
    };
    let launch_id =
        argument_string(arguments, "launch_id").or_else(|| argument_string(arguments, "launchId"));
    if launch_id.is_none() {
        return;
    }
    let profile_id = argument_string(arguments, "profile_id")
        .or_else(|| argument_string(arguments, "profileId"));
    let cwd = argument_string(arguments, "cwd")
        .map(PathBuf::from)
        .map(common::workspace::normalize_workspace_cwd);
    if let Err(error) = common::launch_sessions::record_observed_launch_session(
        launch_id.as_deref(),
        agent_kind,
        profile_id.as_deref(),
        cwd.as_deref(),
        session_id,
        source,
    ) {
        tracing::warn!(
            error = %error,
            launch_id = ?launch_id,
            agent_kind = %agent_kind,
            "failed to record MCP-observed launch session"
        );
    }
}

// ---------------------------------------------------------------------------
// send_file — deliver one workspace file to the current turn target
// ---------------------------------------------------------------------------

pub(super) async fn mcp_send_file(
    id: Option<serde_json::Value>,
    arguments: &serde_json::Value,
    state: &AppState,
) -> Json<serde_json::Value> {
    let Some(thread_id) = argument_string(arguments, "thread_id") else {
        return jsonrpc_err(id, -32602, "Missing required argument: thread_id");
    };
    let thread_id = common::workspace::threads::WorkspaceThreadId::from(thread_id.as_str());
    let Some(file) = argument_string(arguments, "file") else {
        return jsonrpc_err(id, -32602, "Missing required argument: file");
    };

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
                &format!("Failed to load thread runtime {}: {:#}", thread_id, error),
            );
        }
    };
    let Some(target) = runtime.active_turn_target().current() else {
        return mcp_error_text(
            id,
            "send_file can only be called during an active VibeAround turn.",
        );
    };
    let snapshot = runtime.state().await;
    let Some(session_id) = snapshot.session_id else {
        return mcp_error_text(id, "The active thread does not have an agent session yet.");
    };
    let file_path = match resolve_workspace_file(&snapshot.workspace, &file) {
        Ok(path) => path,
        Err(error) => return mcp_error_text(id, &error.to_string()),
    };
    let file_name = file_path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "attachment".to_string());
    let file_url = match url::Url::from_file_path(&file_path) {
        Ok(url) => url.to_string(),
        Err(()) => {
            return mcp_error_text(
                id,
                &format!("Failed to create a file URI for {}", file_path.display()),
            );
        }
    };
    let size = std::fs::metadata(&file_path)
        .ok()
        .and_then(|metadata| i64::try_from(metadata.len()).ok());
    let mut link = acp::ResourceLink::new(file_name.clone(), file_url);
    link.size = size;
    let notification = acp::SessionNotification::new(
        session_id.clone(),
        acp::SessionUpdate::AgentMessageChunk(acp::ContentChunk::new(
            acp::ContentBlock::ResourceLink(link),
        )),
    );
    let notification = match serde_json::to_value(notification) {
        Ok(notification) => notification,
        Err(error) => {
            return mcp_error_text(
                id,
                &format!("Failed to encode the file notification: {}", error),
            );
        }
    };

    state
        .channel_hub
        .send_output(common::channels::ChannelOutput::ThreadReply {
            route: target.route,
            reply_to: target.reply_to,
            reply: common::channels::types::ThreadReply {
                workspace_id: snapshot.workspace_id.to_string(),
                thread_id: snapshot.thread_id.to_string(),
                agent: common::channels::types::ThreadReplyAgent {
                    id: snapshot.host_binding.agent_id,
                    profile: snapshot.host_binding.profile_id,
                    session_id,
                },
                payload: common::channels::types::ThreadReplyPayload::AcpSessionNotification {
                    notification,
                },
            },
        });

    mcp_text(
        id,
        &format!(
            "Queued `{}` for delivery to the current VibeAround conversation.",
            file_name
        ),
    )
}

fn resolve_workspace_file(workspace: &Path, file: &str) -> anyhow::Result<PathBuf> {
    let workspace = workspace
        .canonicalize()
        .with_context(|| format!("Failed to resolve workspace {}", workspace.display()))?;
    let requested = PathBuf::from(file);
    let requested = if requested.is_relative() {
        workspace.join(requested)
    } else {
        requested
    };
    let resolved = requested
        .canonicalize()
        .with_context(|| format!("File not found: {}", requested.display()))?;
    if !resolved.starts_with(&workspace) {
        return Err(anyhow!("File must be inside the active workspace."));
    }
    if !resolved.is_file() {
        return Err(anyhow!("Path is not a file: {}", resolved.display()));
    }
    Ok(resolved)
}

// ---------------------------------------------------------------------------
// prepare_handover — issue a short-lived code consumed by /pickup
// ---------------------------------------------------------------------------

pub(super) async fn mcp_prepare_handover(
    id: Option<serde_json::Value>,
    arguments: &serde_json::Value,
) -> Json<serde_json::Value> {
    let cwd = match arguments.get("cwd").and_then(|v| v.as_str()) {
        Some(c) => c,
        None => return jsonrpc_err(id, -32602, "Missing required argument: cwd"),
    };
    let session_id_arg = arguments
        .get("session_id")
        .and_then(|v| v.as_str())
        .map(String::from);
    let agent_kind = match arguments.get("agent_kind").and_then(|v| v.as_str()) {
        Some(k) => k,
        None => return jsonrpc_err(id, -32602, "Missing required argument: agent_kind"),
    };
    let agent_kind_str = agent_kind;
    let profile_id = common::agent::launch::normalize_launch_profile_id(
        arguments.get("profile_id").and_then(|v| v.as_str()),
    );

    if common::agent::launch::profile_uses_vibearound_credentials(&profile_id) {
        let agent_id = match common::resources::resolve_agent_id(agent_kind_str) {
            Ok(agent_id) => agent_id,
            Err(error) => return mcp_error_text(id, &error),
        };
        let Some(profile) = common::profiles::load_profile(&profile_id) else {
            return mcp_error_text(id, &format!("Profile '{}' was not found.", profile_id));
        };
        if common::profiles::connections::resolve_profile_agent_route(&profile, &agent_id).is_none()
        {
            return mcp_error_text(
                id,
                &format!(
                    "Profile '{}' cannot launch agent '{}'.",
                    profile_id, agent_id
                ),
            );
        }
    }

    // Validate cwd is a known workspace. Paths under the configured default
    // workspace are accepted so generated IM/web workspaces can hand over.
    let config = common::config::ensure_loaded();
    let cwd_path = common::workspace::normalize_workspace_cwd(std::path::PathBuf::from(cwd));
    let default_dir =
        common::workspace::normalize_workspace_cwd(config.resolve_workspace(agent_kind_str));
    let builtin_dir =
        common::workspace::normalize_workspace_cwd(common::config::builtin_workspaces_dir());
    let is_default = cwd_path.starts_with(&default_dir);
    let is_builtin = cwd_path.starts_with(&builtin_dir);
    let is_registered = config
        .all_workspaces()
        .iter()
        .any(|ws| common::workspace::normalize_workspace_cwd(ws) == cwd_path);

    if !is_default && !is_builtin && !is_registered {
        return mcp_error_text(
            id,
            &format!(
                "Workspace {} is not registered in VibeAround.\n\
             Use the `register_workspace` tool to add it first, then retry.",
                cwd_path.to_string_lossy()
            ),
        );
    }

    // Resolve session ID: use provided value, or auto-discover from session files
    let session_id = match session_id_arg {
        Some(sid) if !sid.is_empty() => sid,
        _ => match find_latest_session(agent_kind_str, &cwd_path) {
            Some(sid) => sid,
            None => {
                let hint = match agent_kind_str {
                    "claude" => "In Claude Code, you can find it by running /status.",
                    "gemini" => "In Gemini CLI, run /resume to browse recent sessions.",
                    "codex" => "In Codex CLI, run `codex resume` to see recent sessions.",
                    _ => "Check your agent's session history.",
                };
                return mcp_error_text(
                    id,
                    &format!(
                        "Could not auto-discover session ID. Please provide your session_id explicitly.\n{}",
                        hint
                    ),
                );
            }
        },
    };

    let code = common::workspace::handover::store(common::workspace::handover::HandoverPayload {
        agent_kind: agent_kind_str.to_string(),
        profile_id: Some(profile_id),
        session_id,
        cwd: cwd_path.to_string_lossy().to_string(),
    })
    .await;
    let pickup_cmd = format!("/pickup {}", code);
    mcp_text(
        id,
        &format!(
            "Handover prepared.\n\n\
         Tell the user to send this command in any IM chat connected to VibeAround:\n\
         {}\n\n\
         The code expires in 2 minutes. After sending the command, the user's next message will resume this session.",
            pickup_cmd
        ),
    )
}

// ---------------------------------------------------------------------------
// register_workspace — writes to VibeAround settings.json
// ---------------------------------------------------------------------------

pub(super) async fn mcp_register_workspace(
    id: Option<serde_json::Value>,
    arguments: &serde_json::Value,
) -> Json<serde_json::Value> {
    let cwd = match arguments.get("cwd").and_then(|v| v.as_str()) {
        Some(c) => c,
        None => return jsonrpc_err(id, -32602, "Missing required argument: cwd"),
    };

    enum RegistrationOutcome {
        MissingDirectory,
        AlreadyRegistered,
        Registered,
    }

    let cwd_path = std::path::PathBuf::from(cwd);
    let cwd_display = cwd.to_string();
    let result = tokio::task::spawn_blocking(move || {
        if !cwd_path.is_dir() {
            return Ok(RegistrationOutcome::MissingDirectory);
        }
        let settings = common::config::read_settings_json()?;
        if workspace_is_registered(&settings, &cwd_path) {
            return Ok(RegistrationOutcome::AlreadyRegistered);
        }
        common::config::register_workspace_path(&cwd_path)?;
        Ok::<_, String>(RegistrationOutcome::Registered)
    })
    .await;

    match result {
        Ok(Ok(RegistrationOutcome::MissingDirectory)) => {
            mcp_error_text(id, &format!("Directory does not exist: {}", cwd_display))
        }
        Ok(Ok(RegistrationOutcome::AlreadyRegistered)) => mcp_text(
            id,
            &format!("Workspace {} is already registered.", cwd_display),
        ),
        Ok(Ok(RegistrationOutcome::Registered)) => mcp_text(
            id,
            &format!("Workspace {} registered successfully.", cwd_display),
        ),
        Ok(Err(error)) => mcp_error_text(id, &format!("Failed to update settings: {}", error)),
        Err(error) => mcp_error_text(id, &format!("Failed to update settings: {}", error)),
    }
}

fn workspace_is_registered(settings: &serde_json::Value, path: &std::path::Path) -> bool {
    common::config::config_from_settings_json(settings)
        .all_workspaces()
        .iter()
        .any(|workspace| common::config::workspace_paths_equal(path, workspace))
}

// ---------------------------------------------------------------------------
// Shared helpers
// ---------------------------------------------------------------------------

/// Validate that cwd is a registered workspace. Returns Err with a JSON-RPC
/// error response on failure.
pub(super) fn validate_workspace(
    cwd_path: &std::path::Path,
    id: Option<serde_json::Value>,
) -> Result<(), Json<serde_json::Value>> {
    let config = common::config::ensure_loaded();
    let builtin_dir = common::config::builtin_workspaces_dir();
    let is_builtin = cwd_path.starts_with(&builtin_dir);
    let is_registered = config.all_workspaces().iter().any(|ws| ws == cwd_path);

    if !is_builtin && !is_registered {
        return Err(mcp_error_text(
            id,
            &format!(
                "Workspace {} is not registered in VibeAround.\n\
             Use the `register_workspace` tool to add it first, then retry.",
                cwd_path.display()
            ),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{resolve_workspace_file, workspace_is_registered};

    #[test]
    fn handover_profile_id_defaults_external_sessions_to_direct() {
        assert_eq!(
            common::agent::launch::normalize_launch_profile_id(None),
            common::agent::launch::DIRECT_PROFILE_ID
        );
        assert_eq!(
            common::agent::launch::normalize_launch_profile_id(Some("")),
            common::agent::launch::DIRECT_PROFILE_ID
        );
        assert_eq!(
            common::agent::launch::normalize_launch_profile_id(Some(" direct ")),
            common::agent::launch::DIRECT_PROFILE_ID
        );
        assert_eq!(
            common::agent::launch::normalize_launch_profile_id(Some("DEFAULT")),
            common::agent::launch::DIRECT_PROFILE_ID
        );
        assert_eq!(
            common::agent::launch::normalize_launch_profile_id(Some("claude-deepseek")),
            "claude-deepseek".to_string()
        );
    }

    #[test]
    fn workspace_registration_check_uses_latest_settings_shape() {
        let settings = json!({
            "default_workspace": "/tmp/default",
            "workspaces": ["/tmp/registered"]
        });

        assert!(workspace_is_registered(
            &settings,
            std::path::Path::new("/tmp/default")
        ));
        assert!(workspace_is_registered(
            &settings,
            std::path::Path::new("/tmp/registered")
        ));
        assert!(workspace_is_registered(
            &settings,
            &common::config::builtin_workspaces_dir()
        ));
        assert!(!workspace_is_registered(
            &settings,
            std::path::Path::new("/tmp/missing")
        ));
    }

    #[test]
    fn outbound_file_must_exist_inside_the_active_workspace() {
        let nonce = uuid::Uuid::new_v4().simple().to_string();
        let root = std::env::temp_dir().join(format!("vibearound-send-file-{nonce}"));
        let workspace = root.join("workspace");
        let outside = root.join("outside.txt");
        std::fs::create_dir_all(&workspace).unwrap();
        std::fs::write(workspace.join("report.txt"), "report").unwrap();
        std::fs::write(&outside, "outside").unwrap();

        let resolved = resolve_workspace_file(&workspace, "report.txt").unwrap();
        assert_eq!(
            resolved,
            workspace.join("report.txt").canonicalize().unwrap()
        );
        assert!(
            resolve_workspace_file(&workspace, outside.to_str().unwrap())
                .unwrap_err()
                .to_string()
                .contains("inside the active workspace")
        );

        std::fs::remove_dir_all(root).unwrap();
    }
}
