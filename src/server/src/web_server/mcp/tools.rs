//! MCP `tools/call` implementations.
//!
//! Each tool takes the JSON-RPC id + arguments, validates inputs, touches the
//! relevant workspace config / preview store / session files, and returns a
//! JSON-RPC response.
//!
//! Tools mostly do not touch agent processes directly. `initialize_subagents`
//! is the collaboration exception: it creates git worktrees, records thread
//! state, and asks the thread runtime to start isolated subagent sessions.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{anyhow, Context};
use axum::Json;
use serde::Deserialize;

use crate::web_server::AppState;

use super::jsonrpc::{jsonrpc_err, mcp_error_text, mcp_text};
use super::ports::is_denied_port;
use super::sessions::find_latest_session;

// ---------------------------------------------------------------------------
// get_session_id — resolve the current ACP session ID from route info
// ---------------------------------------------------------------------------

pub(super) async fn mcp_get_session_id(
    id: Option<serde_json::Value>,
    arguments: &serde_json::Value,
    state: &AppState,
) -> Json<serde_json::Value> {
    let channel_kind = match arguments.get("channel_kind").and_then(|v| v.as_str()) {
        Some(c) => c,
        None => return jsonrpc_err(id, -32602, "Missing required argument: channel_kind"),
    };
    let chat_id = match arguments.get("chat_id").and_then(|v| v.as_str()) {
        Some(c) => c,
        None => return jsonrpc_err(id, -32602, "Missing required argument: chat_id"),
    };

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
        Some(snapshot) if snapshot.session_id.is_some() => {
            let sid = snapshot.session_id.unwrap();
            mcp_text(id, &sid)
        }
        _ => mcp_error_text(
            id,
            "No active session found for this route. The agent session may not have started yet.",
        ),
    }
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
        let Some(profile) = common::profiles::schema::load(&profile_id)
            .map(common::profiles::normalize_legacy_profile_and_persist)
        else {
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

    // Validate cwd is a known workspace.
    // Built-in workspaces under ~/.vibearound/workspaces/ are always accepted.
    let config = common::config::ensure_loaded();
    let cwd_path = common::workspace::normalize_workspace_cwd(std::path::PathBuf::from(cwd));
    let builtin_dir =
        common::workspace::normalize_workspace_cwd(common::config::builtin_workspaces_dir());
    let is_builtin = cwd_path.starts_with(&builtin_dir);
    let is_registered = config
        .all_workspaces()
        .iter()
        .any(|ws| common::workspace::normalize_workspace_cwd(ws) == cwd_path);

    if !is_builtin && !is_registered {
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

    let code = common::workspace::handoff::store(common::workspace::handoff::HandoffPayload {
        agent_kind: agent_kind_str.to_string(),
        profile_id: Some(profile_id),
        session_id,
        cwd: cwd_path.to_string_lossy().to_string(),
    });
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

    let cwd_path = std::path::PathBuf::from(cwd);
    if !cwd_path.is_dir() {
        return mcp_error_text(id, &format!("Directory does not exist: {}", cwd));
    }

    // Check if already registered
    let config = common::config::ensure_loaded();
    let already_registered = config.all_workspaces().iter().any(|ws| ws == &cwd_path);

    if already_registered {
        return mcp_text(id, &format!("Workspace {} is already registered.", cwd));
    }

    // Add to settings.json
    let cwd_owned = cwd.to_string();
    if let Err(e) = common::config::update_settings_json(move |settings| {
        if let Some(obj) = settings.as_object_mut() {
            let workspaces = obj
                .entry("workspaces")
                .or_insert_with(|| serde_json::json!([]));
            if let Some(arr) = workspaces.as_array_mut() {
                arr.push(serde_json::Value::String(cwd_owned));
            }
        }
    }) {
        return mcp_error_text(id, &format!("Failed to update settings: {}", e));
    }

    mcp_text(id, &format!("Workspace {} registered successfully.", cwd))
}

// ---------------------------------------------------------------------------
// initialize_subagents — create a parallel multi-agent turn with git worktrees
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct InitializeSubagentsArgs {
    thread_id: String,
    cwd: String,
    mode: String,
    agents: Vec<InitializeSubagentSpec>,
    #[serde(default)]
    branch_prefix: Option<String>,
}

#[derive(Debug, Deserialize)]
struct InitializeSubagentSpec {
    name: String,
    #[serde(alias = "kind")]
    agent_kind: String,
    #[serde(default, alias = "profile")]
    profile_id: Option<String>,
    #[serde(default)]
    task: Option<String>,
}

#[derive(Debug, Deserialize)]
struct WaitForSubagentsArgs {
    thread_id: String,
    #[serde(default)]
    turn_id: Option<String>,
    #[serde(default)]
    timeout_ms: Option<u64>,
}

struct InitializedSubagents {
    turn: common::workspace::threads::MultiAgentTurn,
    agents: Vec<common::workspace::threads::ThreadAgent>,
}

pub(super) async fn mcp_initialize_subagents(
    id: Option<serde_json::Value>,
    arguments: &serde_json::Value,
    state: &AppState,
) -> Json<serde_json::Value> {
    let args = match serde_json::from_value::<InitializeSubagentsArgs>(arguments.clone()) {
        Ok(args) => args,
        Err(error) => return jsonrpc_err(id, -32602, &format!("Invalid arguments: {}", error)),
    };

    let mode = match parse_multi_agent_mode(&args.mode) {
        Ok(mode) => mode,
        Err(error) => return mcp_error_text(id, &error),
    };
    if mode != common::workspace::threads::MultiAgentTurnMode::Parallel {
        return mcp_error_text(
            id,
            "Only `parallel` subagent turns are supported in this first implementation.",
        );
    }
    if args.agents.is_empty() {
        return jsonrpc_err(id, -32602, "Missing required argument: agents");
    }
    if args.agents.len() > 8 {
        return mcp_error_text(id, "At most 8 subagents can be initialized at once.");
    }

    let thread_id = common::workspace::threads::WorkspaceThreadId::from(args.thread_id.trim());
    if thread_id.as_str().is_empty() {
        return jsonrpc_err(id, -32602, "Missing required argument: thread_id");
    }

    let cwd_path = PathBuf::from(args.cwd.trim());
    if !cwd_path.is_dir() {
        return mcp_error_text(
            id,
            &format!("Directory does not exist: {}", cwd_path.display()),
        );
    }
    let cwd_path = common::workspace::normalize_workspace_cwd(cwd_path);
    if let Err(resp) = validate_workspace(&cwd_path, id.clone()) {
        return resp;
    }

    let initialized = match initialize_subagent_worktrees(&cwd_path, &args, mode) {
        Ok(initialized) => initialized,
        Err(error) => return mcp_error_text(id, &format!("{:#}", error)),
    };

    let manager = state.channel_hub.workspace_thread_manager();
    if let Err(error) = manager
        .initialize_multi_agent_turn(
            &thread_id,
            initialized.turn.clone(),
            initialized.agents.clone(),
        )
        .await
    {
        cleanup_created_worktrees(&cwd_path, &initialized.agents);
        return mcp_error_text(
            id,
            &format!(
                "Failed to record multi-agent turn on thread {}: {:#}",
                thread_id, error
            ),
        );
    }

    notify_web_multi_agent_turn(state, &thread_id, &initialized.turn, &initialized.agents).await;
    let start_errors = start_initialized_subagents(state, &thread_id, &initialized.agents).await;

    let body = serde_json::json!({
        "protocol": "va-agent-protocol",
        "kind": "multi_agent_turn",
        "turn": initialized.turn,
        "agents": initialized.agents,
        "started": start_errors.is_empty(),
        "start_errors": start_errors,
        "notes": [
            "Subagents are initialized in isolated git worktrees.",
            "Subagents have been assigned their initial tasks.",
            "Call wait_for_subagents before producing the host final answer.",
            "The host agent remains responsible for review, merge, and cleanup."
        ]
    });
    mcp_text(
        id,
        &serde_json::to_string_pretty(&body).unwrap_or_else(|_| body.to_string()),
    )
}

pub(super) async fn mcp_wait_for_subagents(
    id: Option<serde_json::Value>,
    arguments: &serde_json::Value,
    state: &AppState,
) -> Json<serde_json::Value> {
    let args = match serde_json::from_value::<WaitForSubagentsArgs>(arguments.clone()) {
        Ok(args) => args,
        Err(error) => return jsonrpc_err(id, -32602, &format!("Invalid arguments: {}", error)),
    };
    let thread_id = common::workspace::threads::WorkspaceThreadId::from(args.thread_id.trim());
    if thread_id.as_str().is_empty() {
        return jsonrpc_err(id, -32602, "Missing required argument: thread_id");
    }
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
            )
        }
    };

    let timeout = Duration::from_millis(args.timeout_ms.unwrap_or(600_000).clamp(1_000, 3_600_000));
    let started = Instant::now();
    loop {
        let snapshot = runtime.state().await;
        let turn_id = args
            .turn_id
            .clone()
            .or_else(|| latest_turn_id(&snapshot.multi_agent_turns));
        let agents: Vec<_> = match turn_id.as_deref() {
            Some(turn_id) => snapshot
                .agents
                .into_iter()
                .filter(|agent| agent.turn_id.as_str() == turn_id)
                .collect(),
            None => snapshot.agents,
        };
        let pending = agents.iter().any(|agent| {
            matches!(
                agent.status,
                common::workspace::threads::ThreadAgentStatus::Ready
                    | common::workspace::threads::ThreadAgentStatus::Running
            )
        });
        let timed_out = pending && started.elapsed() >= timeout;
        if !pending || timed_out {
            let completed = !pending && !agents.is_empty();
            let body = serde_json::json!({
                "protocol": "va-agent-protocol",
                "kind": "subagent_reports",
                "thread_id": thread_id.to_string(),
                "turn_id": turn_id,
                "completed": completed,
                "timed_out": timed_out,
                "agents": agents,
            });
            return mcp_text(
                id,
                &serde_json::to_string_pretty(&body).unwrap_or_else(|_| body.to_string()),
            );
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
}

fn parse_multi_agent_mode(
    mode: &str,
) -> Result<common::workspace::threads::MultiAgentTurnMode, String> {
    match mode.trim().to_ascii_lowercase().as_str() {
        "parallel" => Ok(common::workspace::threads::MultiAgentTurnMode::Parallel),
        "collaboration" => Ok(common::workspace::threads::MultiAgentTurnMode::Collaboration),
        "brainstorming" => Ok(common::workspace::threads::MultiAgentTurnMode::Brainstorming),
        other => Err(format!(
            "Unknown subagent mode `{}`. Valid modes: parallel, collaboration, brainstorming.",
            other
        )),
    }
}

fn latest_turn_id(turns: &[common::workspace::threads::MultiAgentTurn]) -> Option<String> {
    turns
        .iter()
        .max_by(|a, b| a.created_at.cmp(&b.created_at))
        .map(|turn| turn.id.to_string())
}

fn initialize_subagent_worktrees(
    cwd: &Path,
    args: &InitializeSubagentsArgs,
    mode: common::workspace::threads::MultiAgentTurnMode,
) -> anyhow::Result<InitializedSubagents> {
    ensure_git_available()?;
    let repo_root = ensure_git_repository(cwd)?;
    let repo_root = common::workspace::normalize_workspace_cwd(repo_root);
    let head = ensure_git_head(&repo_root)?;
    let dirty = git_output(
        &repo_root,
        &["status", "--porcelain=v1", "--untracked-files=all"],
    )?;
    if !dirty.trim().is_empty() {
        return Err(anyhow!(
            "Workspace has uncommitted or untracked changes. Commit, stash, or clean the workspace before initializing subagents."
        ));
    }
    let branch_prefix = clean_branch_prefix(args.branch_prefix.as_deref())?;
    let repo_slug = repo_root
        .file_name()
        .and_then(|name| name.to_str())
        .map(slugify)
        .filter(|slug| !slug.is_empty())
        .unwrap_or_else(|| "workspace".to_string());

    let turn_id = common::workspace::threads::MultiAgentTurnId::new();
    let short_turn = short_id(turn_id.as_str());
    let worktree_base = common::config::data_dir()
        .join("worktrees")
        .join(repo_slug)
        .join(turn_id.as_str());
    std::fs::create_dir_all(&worktree_base)
        .with_context(|| format!("create worktree base {}", worktree_base.display()))?;

    let mut agent_ids = Vec::with_capacity(args.agents.len());
    let mut agents = Vec::with_capacity(args.agents.len());

    for spec in &args.agents {
        let name = validate_agent_name(&spec.name)?;
        let agent_id = common::resources::resolve_agent_id(&spec.agent_kind)
            .map_err(|error| anyhow!(error))?;
        let subagent_id = common::workspace::threads::ThreadAgentId::new();
        let agent_short_id = short_id(subagent_id.as_str());
        let name_slug = slugify(&name);
        let branch = format!(
            "{}/{}/{}-{}",
            branch_prefix, short_turn, name_slug, agent_short_id
        );
        let worktree = worktree_base.join(format!("{}-{}", name_slug, agent_short_id));

        if let Err(error) = git_worktree_add(&repo_root, &branch, &worktree, &head) {
            cleanup_created_worktrees(&repo_root, &agents);
            return Err(error);
        }

        agent_ids.push(subagent_id.clone());
        agents.push(common::workspace::threads::ThreadAgent::ready(
            subagent_id,
            turn_id.clone(),
            name,
            agent_id,
            spec.profile_id.clone(),
            branch,
            worktree.to_string_lossy().to_string(),
            spec.task.clone().filter(|task| !task.trim().is_empty()),
        ));
    }

    Ok(InitializedSubagents {
        turn: common::workspace::threads::MultiAgentTurn::new(turn_id, mode, agent_ids),
        agents,
    })
}

fn ensure_git_available() -> anyhow::Result<()> {
    if command_success("git", &["--version"]) {
        return Ok(());
    }
    if try_install_git()? && command_success("git", &["--version"]) {
        return Ok(());
    }
    Err(anyhow!(
        "Git is required to initialize subagents, but `git` was not found on PATH."
    ))
}

fn try_install_git() -> anyhow::Result<bool> {
    if cfg!(target_os = "macos") && command_success("brew", &["--version"]) {
        let output = common::process::env::std_command("brew")
            .args(["install", "git"])
            .output()
            .context("install git with Homebrew")?;
        return Ok(output.status.success());
    }
    Ok(false)
}

fn command_success(program: &str, args: &[&str]) -> bool {
    common::process::env::std_command(program)
        .args(args)
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

fn ensure_git_repository(cwd: &Path) -> anyhow::Result<PathBuf> {
    if let Ok(root) = git_output(cwd, &["rev-parse", "--show-toplevel"]) {
        return Ok(PathBuf::from(root));
    }
    let output = common::process::env::std_command("git")
        .arg("-C")
        .arg(cwd)
        .arg("init")
        .output()
        .with_context(|| format!("git init {}", cwd.display()))?;
    if !output.status.success() {
        return Err(anyhow!(
            "git init failed in {}: {}",
            cwd.display(),
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(PathBuf::from(git_output(
        cwd,
        &["rev-parse", "--show-toplevel"],
    )?))
}

fn ensure_git_head(repo_root: &Path) -> anyhow::Result<String> {
    if let Ok(head) = git_output(repo_root, &["rev-parse", "--verify", "HEAD"]) {
        return Ok(head);
    }
    let output = common::process::env::std_command("git")
        .arg("-C")
        .arg(repo_root)
        .args([
            "-c",
            "user.name=VibeAround",
            "-c",
            "user.email=vibearound@example.invalid",
            "commit",
            "--allow-empty",
            "-m",
            "Initialize workspace for VibeAround subagents",
        ])
        .output()
        .with_context(|| format!("create initial git commit in {}", repo_root.display()))?;
    if !output.status.success() {
        return Err(anyhow!(
            "git initial commit failed in {}: {}",
            repo_root.display(),
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    git_output(repo_root, &["rev-parse", "--verify", "HEAD"])
}

async fn notify_web_multi_agent_turn(
    state: &AppState,
    thread_id: &common::workspace::threads::WorkspaceThreadId,
    turn: &common::workspace::threads::MultiAgentTurn,
    agents: &[common::workspace::threads::ThreadAgent],
) {
    for route in web_routes_for_thread(state, thread_id, "multi-agent turn").await {
        state
            .channel_hub
            .send_output(common::channels::ChannelOutput::MultiAgentTurn {
                route,
                turn: turn.clone(),
                agents: agents.to_vec(),
            })
            .await;
    }
}

async fn start_initialized_subagents(
    state: &AppState,
    thread_id: &common::workspace::threads::WorkspaceThreadId,
    agents: &[common::workspace::threads::ThreadAgent],
) -> Vec<String> {
    let manager = state.channel_hub.workspace_thread_manager();
    let runtime = match manager.runtime_for_thread_id(thread_id).await {
        Ok(runtime) => runtime,
        Err(error) => {
            return vec![format!("failed to load thread runtime: {:#}", error)];
        }
    };

    let web_routes = web_routes_for_thread(state, thread_id, "subagent launch").await;
    let launch_route = web_routes
        .first()
        .cloned()
        .unwrap_or_else(|| common::routing::RouteKey::new("web", thread_id.as_str()));
    let (status_tx, mut status_rx) =
        tokio::sync::mpsc::unbounded_channel::<common::workspace::threads::ThreadAgent>();
    let state_for_status = state.clone();
    let thread_for_status = thread_id.clone();
    tokio::spawn(async move {
        while let Some(agent) = status_rx.recv().await {
            notify_web_subagent_status(&state_for_status, &thread_for_status, &agent).await;
        }
    });

    let mut errors = Vec::new();
    for agent in agents {
        let tracker =
            Arc::new(common::channels::subagent_handler::SubagentReportTracker::new(agent.clone()));
        let handler = Arc::new(
            common::channels::subagent_handler::SubagentBridgeHandler::for_thread(
                state.channel_hub.plugin_host(),
                &manager,
                thread_id.clone(),
                agent.clone(),
                Arc::clone(&tracker),
            ),
        );
        let validator: Arc<dyn common::workspace::threads::runtime::SubagentCompletionValidator> =
            tracker;
        if let Err(error) = runtime
            .start_subagent_assignment(
                &launch_route,
                agent.clone(),
                handler,
                status_tx.clone(),
                Some(validator),
            )
            .await
        {
            errors.push(format!("{}: {}", agent.name, error.message));
        }
    }
    errors
}

async fn notify_web_subagent_status(
    state: &AppState,
    thread_id: &common::workspace::threads::WorkspaceThreadId,
    agent: &common::workspace::threads::ThreadAgent,
) {
    for route in web_routes_for_thread(state, thread_id, "subagent status").await {
        state
            .channel_hub
            .send_output(common::channels::ChannelOutput::SubagentStatus {
                route,
                agent: agent.clone(),
            })
            .await;
    }
}

async fn web_routes_for_thread(
    state: &AppState,
    thread_id: &common::workspace::threads::WorkspaceThreadId,
    purpose: &'static str,
) -> Vec<common::routing::RouteKey> {
    match state
        .channel_hub
        .workspace_thread_manager()
        .attached_routes_for_thread(thread_id)
        .await
    {
        Ok(routes) => routes
            .into_iter()
            .filter(|route| route.channel_kind == "web")
            .collect(),
        Err(error) => {
            tracing::warn!(
                thread_id = %thread_id,
                error = %error,
                purpose,
                "failed to resolve web routes for thread"
            );
            Vec::new()
        }
    }
}

fn validate_agent_name(name: &str) -> anyhow::Result<String> {
    let trimmed = name.trim();
    let char_count = trimmed.chars().count();
    if !(2..=64).contains(&char_count) {
        return Err(anyhow!("Subagent name must be 2-64 characters."));
    }
    if trimmed
        .chars()
        .any(|ch| ch.is_control() || matches!(ch, '/' | '\\'))
    {
        return Err(anyhow!(
            "Subagent name `{}` contains unsupported characters.",
            trimmed
        ));
    }
    if !trimmed.chars().any(|ch| ch.is_alphanumeric()) {
        return Err(anyhow!(
            "Subagent name `{}` must contain at least one letter or number.",
            trimmed
        ));
    }
    Ok(trimmed.to_string())
}

fn clean_branch_prefix(prefix: Option<&str>) -> anyhow::Result<String> {
    let prefix = prefix.unwrap_or("va/subagents").trim().trim_matches('/');
    if prefix.is_empty()
        || prefix.contains("..")
        || prefix
            .chars()
            .any(|ch| ch.is_control() || ch.is_whitespace() || matches!(ch, '\\' | ':'))
    {
        return Err(anyhow!("Invalid branch_prefix `{}`.", prefix));
    }
    Ok(prefix.to_string())
}

fn git_output(cwd: &Path, args: &[&str]) -> anyhow::Result<String> {
    let output = common::process::env::std_command("git")
        .arg("-C")
        .arg(cwd)
        .args(args)
        .output()
        .with_context(|| format!("run git {}", args.join(" ")))?;
    if !output.status.success() {
        return Err(anyhow!(
            "git {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn git_worktree_add(cwd: &Path, branch: &str, worktree: &Path, head: &str) -> anyhow::Result<()> {
    let output = common::process::env::std_command("git")
        .arg("-C")
        .arg(cwd)
        .args(["worktree", "add", "-b", branch])
        .arg(worktree)
        .arg(head)
        .output()
        .with_context(|| format!("create git worktree {}", worktree.display()))?;
    if output.status.success() {
        return Ok(());
    }
    Err(anyhow!(
        "git worktree add failed for {}: {}",
        worktree.display(),
        String::from_utf8_lossy(&output.stderr).trim()
    ))
}

fn cleanup_created_worktrees(repo: &Path, agents: &[common::workspace::threads::ThreadAgent]) {
    for agent in agents.iter().rev() {
        let _ = common::process::env::std_command("git")
            .arg("-C")
            .arg(repo)
            .args(["worktree", "remove", "--force", &agent.worktree])
            .output();
        let _ = common::process::env::std_command("git")
            .arg("-C")
            .arg(repo)
            .args(["branch", "-D", &agent.branch])
            .output();
    }
}

fn slugify(value: &str) -> String {
    let mut slug = String::new();
    let mut last_dash = false;
    for ch in value.chars().flat_map(char::to_lowercase) {
        if ch.is_ascii_alphanumeric() {
            slug.push(ch);
            last_dash = false;
        } else if !last_dash {
            slug.push('-');
            last_dash = true;
        }
    }
    let slug = slug.trim_matches('-').to_string();
    if slug.is_empty() {
        "agent".to_string()
    } else {
        slug
    }
}

fn short_id(id: &str) -> String {
    id.chars()
        .filter(|ch| ch.is_ascii_hexdigit())
        .take(8)
        .collect::<String>()
}

// ---------------------------------------------------------------------------
// preview_start — register a live preview for a running local server
// ---------------------------------------------------------------------------

pub(super) async fn mcp_preview_start(
    id: Option<serde_json::Value>,
    arguments: &serde_json::Value,
    state: &AppState,
) -> Json<serde_json::Value> {
    let port = match arguments.get("port").and_then(|v| v.as_u64()) {
        Some(p) if p > 0 && p <= 65535 => p as u16,
        _ => {
            return jsonrpc_err(
                id,
                -32602,
                "Missing or invalid required argument: port (1-65535)",
            )
        }
    };

    if is_denied_port(port) {
        return mcp_error_text(
            id,
            &format!(
                "Port {} is a well-known service port and cannot be previewed for security reasons. \
             Use a typical dev server port (e.g. 3000, 5173, 8080).",
                port
            ),
        );
    }

    let cwd = match arguments.get("cwd").and_then(|v| v.as_str()) {
        Some(c) => c,
        None => return jsonrpc_err(id, -32602, "Missing required argument: cwd"),
    };

    let cwd_path = std::path::PathBuf::from(cwd);
    if let Err(resp) = validate_workspace(&cwd_path, id.clone()) {
        return resp;
    }

    let title = derive_title(arguments, &cwd_path);
    let session_id = arguments
        .get("session_id")
        .and_then(|v| v.as_str())
        .map(String::from);

    let (owner_slug, share_slug) =
        common::previews::ensure_server(port, cwd_path, title, session_id.clone());
    let owner_url = build_preview_url(state, "preview/u", &owner_slug);
    let share_url = build_preview_url(state, "preview/s", &share_slug);

    let session_hint = if session_id.is_none() {
        "\n\n\u{26a0}\u{fe0f} No session_id provided. Use /va-session skill to resolve it and pass session_id for automatic dev-server cleanup."
    } else {
        ""
    };

    mcp_text(
        id,
        &format!(
            "Preview ready.\n\n\
         Owner: `{}`\n\
         Share: `{}`\n\
         Port: {}\n\
         Share expires: 10 minutes{}",
            owner_url, share_url, port, session_hint
        ),
    )
}

// ---------------------------------------------------------------------------
// md_preview — render a markdown file with styled preview
// ---------------------------------------------------------------------------

pub(super) async fn mcp_md_preview(
    id: Option<serde_json::Value>,
    arguments: &serde_json::Value,
    state: &AppState,
) -> Json<serde_json::Value> {
    let file_str = match arguments.get("file").and_then(|v| v.as_str()) {
        Some(f) => f,
        None => return jsonrpc_err(id, -32602, "Missing required argument: file"),
    };
    let cwd = match arguments.get("cwd").and_then(|v| v.as_str()) {
        Some(c) => c,
        None => return jsonrpc_err(id, -32602, "Missing required argument: cwd"),
    };

    let cwd_path = std::path::PathBuf::from(cwd);
    if let Err(resp) = validate_workspace(&cwd_path, id.clone()) {
        return resp;
    }

    // Resolve relative paths against cwd.
    let file_path = {
        let p = std::path::PathBuf::from(file_str);
        if p.is_relative() {
            cwd_path.join(&p)
        } else {
            p
        }
    };
    if !file_path.is_file() {
        return mcp_error_text(id, &format!("File not found: {}", file_path.display()));
    }

    // Security: file must be inside the workspace.
    if let (Ok(canon_file), Ok(canon_ws)) = (file_path.canonicalize(), cwd_path.canonicalize()) {
        if !canon_file.starts_with(&canon_ws) {
            return mcp_error_text(id, "File must be inside the workspace directory.");
        }
    }

    let title = arguments
        .get("title")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .map(String::from)
        .unwrap_or_else(|| {
            file_path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("Preview")
                .to_string()
        });

    let (owner_slug, share_slug) = common::previews::ensure_file(file_path, cwd_path, title);
    let owner_url = build_preview_url(state, "preview/u", &owner_slug);
    let share_url = build_preview_url(state, "preview/s", &share_slug);

    mcp_text(
        id,
        &format!(
            "Markdown preview ready.\n\n\
         Owner: `{}`\n\
         Share: `{}`\n\
         Share expires: 10 minutes",
            owner_url, share_url
        ),
    )
}

// ---------------------------------------------------------------------------
// Shared helpers
// ---------------------------------------------------------------------------

/// Validate that cwd is a registered workspace. Returns Err with a JSON-RPC
/// error response on failure.
fn validate_workspace(
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

/// Derive a title from the MCP arguments or the workspace directory name.
fn derive_title(arguments: &serde_json::Value, cwd_path: &std::path::Path) -> String {
    arguments
        .get("title")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .map(String::from)
        .unwrap_or_else(|| {
            cwd_path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("Preview")
                .to_string()
        })
}

/// Build a full preview URL from the tunnel (or localhost fallback).
/// All preview routes live under `/va/` to avoid conflicts with dev servers.
fn build_preview_url(state: &AppState, route: &str, slug: &str) -> String {
    let base = state
        .tunnels
        .first_url()
        .unwrap_or_else(|| format!("http://127.0.0.1:{}", state.port));
    format!("{}/va/{}/{}", base.trim_end_matches('/'), route, slug)
}

#[cfg(test)]
mod tests {
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
}
