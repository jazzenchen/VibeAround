use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::Json;
use serde::Deserialize;

use crate::web_server::AppState;

use super::{
    jsonrpc::{jsonrpc_err, mcp_error_text, mcp_text},
    subagent_worktrees::{cleanup_created_worktrees, initialize_subagent_worktrees},
    tools::validate_workspace,
};

#[derive(Debug, Deserialize)]
pub(super) struct InitializeSubagentsArgs {
    pub(super) thread_id: String,
    pub(super) cwd: String,
    pub(super) mode: String,
    pub(super) agents: Vec<InitializeSubagentSpec>,
    #[serde(default)]
    pub(super) branch_prefix: Option<String>,
}
#[derive(Debug, Deserialize)]
pub(super) struct InitializeSubagentSpec {
    pub(super) name: String,
    #[serde(alias = "kind")]
    pub(super) agent_kind: String,
    #[serde(default, alias = "profile")]
    pub(super) profile_id: Option<String>,
    #[serde(default)]
    pub(super) task: Option<String>,
}
#[derive(Debug, Deserialize)]
struct WaitForSubagentsArgs {
    thread_id: String,
    #[serde(default)]
    turn_id: Option<String>,
    #[serde(default)]
    timeout_ms: Option<u64>,
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

    notify_presentation_multi_agent_turn(state, &thread_id, &initialized.turn, &initialized.agents)
        .await;
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
            );
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

async fn notify_presentation_multi_agent_turn(
    state: &AppState,
    thread_id: &common::workspace::threads::WorkspaceThreadId,
    turn: &common::workspace::threads::MultiAgentTurn,
    agents: &[common::workspace::threads::ThreadAgent],
) {
    for route in presentation_routes_for_thread(state, thread_id, "multi-agent turn").await {
        state
            .channel_hub
            .send_output(common::channels::ChannelOutput::MultiAgentTurn {
                route,
                turn: turn.clone(),
                agents: agents.to_vec(),
            });
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
        Err(error) => return vec![format!("failed to load thread runtime: {:#}", error)],
    };

    let presentation_routes =
        presentation_routes_for_thread(state, thread_id, "subagent launch").await;
    let launch_route = presentation_routes
        .first()
        .cloned()
        .unwrap_or_else(|| common::routing::RouteKey::new("web", thread_id.as_str()));
    let (status_tx, mut status_rx) =
        tokio::sync::mpsc::unbounded_channel::<common::workspace::threads::ThreadAgent>();
    let state_for_status = state.clone();
    let thread_for_status = thread_id.clone();
    tokio::spawn(async move {
        while let Some(agent) = status_rx.recv().await {
            notify_presentation_subagent_status(&state_for_status, &thread_for_status, &agent)
                .await;
        }
    });

    let mut errors = Vec::new();
    for agent in agents {
        let active_turn_target = common::routing::ActiveTurnTarget::default();
        let tracker =
            Arc::new(common::channels::subagent_handler::SubagentReportTracker::new(agent.clone()));
        let handler = Arc::new(
            common::channels::subagent_handler::SubagentBridgeHandler::for_thread(
                state.channel_hub.plugin_host(),
                &manager,
                thread_id.clone(),
                agent.clone(),
                active_turn_target.clone(),
                Arc::clone(&tracker),
            ),
        );
        let validator: Arc<dyn common::workspace::threads::runtime::SubagentCompletionValidator> =
            tracker;
        if let Err(error) = runtime
            .start_subagent_assignment(
                common::routing::ChannelTarget::for_route(launch_route.clone()),
                agent.clone(),
                handler,
                active_turn_target,
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

async fn notify_presentation_subagent_status(
    state: &AppState,
    thread_id: &common::workspace::threads::WorkspaceThreadId,
    agent: &common::workspace::threads::ThreadAgent,
) {
    for route in presentation_routes_for_thread(state, thread_id, "subagent status").await {
        state
            .channel_hub
            .send_output(common::channels::ChannelOutput::SubagentStatus {
                route,
                agent: agent.clone(),
            });
    }
}

async fn presentation_routes_for_thread(
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
            .filter(|route| common::routing::channel_traits(&route.channel_kind).rich_agent_events)
            .collect(),
        Err(error) => {
            tracing::warn!(
                thread_id = %thread_id,
                error = %error,
                purpose,
                "failed to resolve presentation routes for thread"
            );
            Vec::new()
        }
    }
}
