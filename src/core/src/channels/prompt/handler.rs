//! Workspace-thread prompt handling for channel inputs.

use std::path::PathBuf;
use std::sync::Arc;

use agent_client_protocol::schema::v1 as acp;

use crate::agent::launch::{normalize_launch_profile_id, DIRECT_PROFILE_ID};
use crate::agent::AgentClientHandler;
use crate::channels::bridge_handler::ChannelBridgeHandler;
use crate::channels::plugin_host::PluginHost;
use crate::channels::subagent_handler::{SubagentBridgeHandler, SubagentReportTracker};
use crate::channels::types::{
    ChannelOutput, ChannelSessionAgent, ChannelSessionInfo, ChannelSessionStart,
};
use crate::profiles::{self, connections};
use crate::routing::{ChannelTarget, RouteKey};
use crate::workspace::manager::ExternalSessionAttachMode;
use crate::workspace::threads::runtime::{
    route_allows_startup_replay, ThreadRuntime, ThreadRuntimeState,
};
use crate::workspace::threads::store::HostBinding;
use crate::workspace::WorkspaceThreadManager;

use super::send_system_text_to_target;

const SESSION_LIST_LIMIT: usize = 5;

pub(crate) async fn handle_prompt(
    workspace_threads: &Arc<WorkspaceThreadManager>,
    plugin_host: &Arc<PluginHost>,
    target: ChannelTarget,
    mut content_blocks: Vec<acp::ContentBlock>,
) -> acp::Result<acp::PromptResponse> {
    let text = first_text(&content_blocks).unwrap_or_default();

    let route = &target.route;
    if commands_enabled_for_route(route) {
        if let Some(command) = parse_thread_command(&text) {
            return handle_command(workspace_threads, plugin_host, &target, command).await;
        }
    }

    if content_blocks.is_empty() {
        return Ok(acp::PromptResponse::new(acp::StopReason::EndTurn));
    }

    let runtime = workspace_threads
        .resolve_route_runtime(route)
        .await
        .map_err(internal_error)?;
    if !start_runtime_and_notify(workspace_threads, &runtime, plugin_host, &target, false).await? {
        return Ok(acp::PromptResponse::new(acp::StopReason::EndTurn));
    }
    let state = runtime.state().await;
    let handler = bridge_handler(workspace_threads, plugin_host, &runtime, &state);
    runtime
        .prompt(&target, std::mem::take(&mut content_blocks), handler)
        .await
}

async fn handle_command(
    workspace_threads: &Arc<WorkspaceThreadManager>,
    plugin_host: &Arc<PluginHost>,
    target: &ChannelTarget,
    command: ThreadCommand,
) -> acp::Result<acp::PromptResponse> {
    let route = &target.route;
    match command {
        ThreadCommand::New => {
            let runtime = workspace_threads
                .close_route_and_create_thread(route, Some("user started a new thread".to_string()))
                .await
                .map_err(internal_error)?;
            if !start_runtime_and_notify(workspace_threads, &runtime, plugin_host, target, true)
                .await?
            {
                return Ok(acp::PromptResponse::new(acp::StopReason::EndTurn));
            }
            send_system_text_to_target(
                plugin_host,
                target,
                &format!("Started new thread {}.", runtime.state().await.thread_id),
            )
            .await;
        }
        ThreadCommand::Close => {
            workspace_threads
                .close_route(route, Some("user closed".to_string()))
                .await
                .map_err(internal_error)?;
            send_system_text_to_target(
                plugin_host,
                target,
                "Thread closed. Use /new to start again.",
            )
            .await;
        }
        ThreadCommand::Pair(code) => {
            if crate::auth::pair::validate(&code).is_some() {
                send_system_text_to_target(plugin_host, target, "Session paired.").await;
            } else {
                send_system_text_to_target(
                    plugin_host,
                    target,
                    "Pairing code is invalid or expired.",
                )
                .await;
            }
        }
        ThreadCommand::Pickup(code) => {
            let Some(handover) = crate::workspace::handover::consume(&code) else {
                send_system_text_to_target(
                    plugin_host,
                    target,
                    "Handover code is invalid or expired.",
                )
                .await;
                return Ok(acp::PromptResponse::new(acp::StopReason::EndTurn));
            };
            let agent_id =
                crate::resources::resolve_agent_id(&handover.agent_kind).map_err(invalid_params)?;
            let session_id = handover.session_id.clone();
            let runtime = match workspace_threads
                .attach_external_session(
                    route,
                    agent_id.clone(),
                    handover.profile_id,
                    handover.session_id,
                    std::path::PathBuf::from(handover.cwd),
                    ExternalSessionAttachMode::ReuseOpenThread,
                )
                .await
            {
                Ok(runtime) => runtime,
                Err(error) => {
                    send_system_text_to_target(
                        plugin_host,
                        target,
                        &format!("Could not pickup session: {:#}", error),
                    )
                    .await;
                    return Ok(acp::PromptResponse::new(acp::StopReason::EndTurn));
                }
            };
            if !start_runtime_and_notify(workspace_threads, &runtime, plugin_host, target, true)
                .await?
            {
                return Ok(acp::PromptResponse::new(acp::StopReason::EndTurn));
            }
            send_system_text_to_target(
                plugin_host,
                target,
                &format!("Picked up session {} with {}.", session_id, agent_id),
            )
            .await;
        }
        ThreadCommand::Status => {
            let runtime = workspace_threads
                .resolve_route_runtime(route)
                .await
                .map_err(internal_error)?;
            send_system_text_to_target(plugin_host, target, &format_status(&runtime.state().await))
                .await;
        }
        ThreadCommand::Resource { kind, action } => match (kind, action) {
            (ResourceKind::Workspace, ResourceAction::List) => {
                let runtime = workspace_threads
                    .resolve_route_runtime(route)
                    .await
                    .map_err(internal_error)?;
                let current = runtime.state().await.workspace_id;
                let workspaces = workspace_threads
                    .list_workspaces()
                    .await
                    .map_err(internal_error)?;
                send_system_text_to_target(
                    plugin_host,
                    target,
                    &format_workspace_list(&workspaces, current.as_str()),
                )
                .await;
            }
            (ResourceKind::Workspace, ResourceAction::Switch(id)) => {
                switch_workspace(
                    workspace_threads,
                    plugin_host,
                    target,
                    &id,
                    WorkspaceTokenMode::IdOnly,
                )
                .await?;
            }
            (ResourceKind::Agent, ResourceAction::List) => {
                let runtime = workspace_threads
                    .resolve_route_runtime(route)
                    .await
                    .map_err(internal_error)?;
                let current = runtime.state().await.host_binding.agent_id;
                send_system_text_to_target(plugin_host, target, &format_agent_list(&current)).await;
            }
            (ResourceKind::Agent, ResourceAction::Switch(id)) => {
                switch_host(workspace_threads, plugin_host, target, &id, None).await?;
            }
            (ResourceKind::Profile, ResourceAction::List) => {
                let runtime = workspace_threads
                    .resolve_route_runtime(route)
                    .await
                    .map_err(internal_error)?;
                let state = runtime.state().await;
                send_system_text_to_target(plugin_host, target, &format_profile_list(&state)).await;
            }
            (ResourceKind::Profile, ResourceAction::Switch(id)) => {
                let runtime = workspace_threads
                    .resolve_route_runtime(route)
                    .await
                    .map_err(internal_error)?;
                let agent = runtime.state().await.host_binding.agent_id;
                validate_profile_for_agent(&id, &agent).map_err(invalid_params)?;
                switch_host(workspace_threads, plugin_host, target, &agent, Some(id)).await?;
            }
            (ResourceKind::Session, ResourceAction::List) => {
                let runtime = workspace_threads
                    .resolve_route_runtime(route)
                    .await
                    .map_err(internal_error)?;
                let state = runtime.state().await;
                let sessions = list_sessions_for_state(workspace_threads, &state).await;
                send_system_text_to_target(
                    plugin_host,
                    target,
                    &format_session_list(&state, &sessions),
                )
                .await;
            }
            (ResourceKind::Session, ResourceAction::Switch(id)) => {
                switch_session(workspace_threads, plugin_host, target, &id).await?;
            }
        },
        ThreadCommand::SwitchWorkspace(token) => {
            switch_workspace(
                workspace_threads,
                plugin_host,
                target,
                &token,
                WorkspaceTokenMode::Legacy,
            )
            .await?;
        }
        ThreadCommand::SwitchHost { agent, profile } => {
            switch_host(workspace_threads, plugin_host, target, &agent, profile).await?;
        }
        ThreadCommand::AgentPassThrough(command) => {
            return send_agent_command(workspace_threads, plugin_host, target, &command).await;
        }
        ThreadCommand::Help => {
            send_system_text_to_target(plugin_host, target, command_help_text()).await;
        }
        ThreadCommand::Unknown(command) => {
            send_system_text_to_target(
                plugin_host,
                target,
                &format!("Unknown command: {}", command),
            )
            .await;
        }
    }
    Ok(acp::PromptResponse::new(acp::StopReason::EndTurn))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WorkspaceTokenMode {
    IdOnly,
    Legacy,
}

async fn switch_workspace(
    workspace_threads: &Arc<WorkspaceThreadManager>,
    plugin_host: &Arc<PluginHost>,
    target: &ChannelTarget,
    token: &str,
    mode: WorkspaceTokenMode,
) -> acp::Result<()> {
    let route = &target.route;
    let runtime = match mode {
        WorkspaceTokenMode::IdOnly => workspace_threads.switch_workspace_id(route, token).await,
        WorkspaceTokenMode::Legacy => workspace_threads.switch_workspace(route, token).await,
    }
    .map_err(internal_error)?;

    if !start_runtime_and_notify(workspace_threads, &runtime, plugin_host, target, true).await? {
        return Ok(());
    }
    send_system_text_to_target(
        plugin_host,
        target,
        &format!(
            "Entered workspace and started thread {}.",
            runtime.state().await.thread_id
        ),
    )
    .await;
    Ok(())
}

async fn switch_host(
    workspace_threads: &Arc<WorkspaceThreadManager>,
    plugin_host: &Arc<PluginHost>,
    channel_target: &ChannelTarget,
    agent: &str,
    profile: Option<String>,
) -> acp::Result<()> {
    let route = &channel_target.route;
    let host_binding = resolve_host_binding(agent, profile.as_deref()).map_err(invalid_params)?;
    let active_runtime = workspace_threads
        .active_route_runtime(route)
        .await
        .map_err(internal_error)?;
    if let Some(runtime) = active_runtime {
        if runtime.state().await.host_binding.agent_id == host_binding.agent_id {
            runtime
                .switch_profile_preserving_session(host_binding.clone())
                .await?;
            if !start_runtime_and_notify(
                workspace_threads,
                &runtime,
                plugin_host,
                channel_target,
                true,
            )
            .await?
            {
                return Ok(());
            }
            send_system_text_to_target(
                plugin_host,
                channel_target,
                &format!(
                    "Switched profile to {}.",
                    host_binding
                        .profile_id
                        .as_deref()
                        .unwrap_or(DIRECT_PROFILE_ID)
                ),
            )
            .await;
            return Ok(());
        }
    }

    let runtime = workspace_threads
        .create_thread_in_current_workspace_with_host(route, host_binding.clone())
        .await
        .map_err(internal_error)?;
    if !start_runtime_and_notify(
        workspace_threads,
        &runtime,
        plugin_host,
        channel_target,
        true,
    )
    .await?
    {
        return Ok(());
    }
    send_system_text_to_target(
        plugin_host,
        channel_target,
        &format!(
            "Switched agent to {} in new thread {}.",
            host_binding.agent_id,
            runtime.state().await.thread_id
        ),
    )
    .await;
    Ok(())
}

async fn send_agent_command(
    workspace_threads: &Arc<WorkspaceThreadManager>,
    plugin_host: &Arc<PluginHost>,
    target: &ChannelTarget,
    command: &str,
) -> acp::Result<acp::PromptResponse> {
    let route = &target.route;
    let runtime = workspace_threads
        .resolve_route_runtime(route)
        .await
        .map_err(internal_error)?;
    if !start_runtime_and_notify(workspace_threads, &runtime, plugin_host, target, false).await? {
        return Ok(acp::PromptResponse::new(acp::StopReason::EndTurn));
    }
    let state = runtime.state().await;
    let handler = bridge_handler(workspace_threads, plugin_host, &runtime, &state);
    runtime
        .prompt(
            target,
            vec![acp::ContentBlock::Text(acp::TextContent::new(
                command.to_string(),
            ))],
            handler,
        )
        .await
}

async fn switch_session(
    workspace_threads: &Arc<WorkspaceThreadManager>,
    plugin_host: &Arc<PluginHost>,
    target: &ChannelTarget,
    id: &str,
) -> acp::Result<()> {
    let route = &target.route;
    let runtime = workspace_threads
        .resolve_route_runtime(route)
        .await
        .map_err(internal_error)?;
    let state = runtime.state().await;
    let sessions = list_sessions_for_state(workspace_threads, &state).await;
    let matches: Vec<_> = sessions
        .into_iter()
        .filter(|session| {
            session.session_id == id || crate::launch_sessions::short_id(&session.session_id) == id
        })
        .collect();
    let session = match matches.as_slice() {
        [] => {
            send_system_text_to_target(
                plugin_host,
                target,
                &format!(
                    "Session '{}' was not found for the current agent/workspace.",
                    id
                ),
            )
            .await;
            return Ok(());
        }
        [session] => session.clone(),
        _ => {
            send_system_text_to_target(
                plugin_host,
                target,
                &format!("Session '{}' is ambiguous; use the full session id.", id),
            )
            .await;
            return Ok(());
        }
    };
    let resumed = match workspace_threads
        .attach_external_session(
            route,
            session.agent_id.clone(),
            state.host_binding.profile_id.clone(),
            session.session_id.clone(),
            PathBuf::from(&session.workspace),
            ExternalSessionAttachMode::NewThread,
        )
        .await
    {
        Ok(runtime) => runtime,
        Err(error) => {
            send_system_text_to_target(
                plugin_host,
                target,
                &format!("Could not switch session: {:#}", error),
            )
            .await;
            return Ok(());
        }
    };
    if !start_runtime_and_notify(workspace_threads, &resumed, plugin_host, target, true).await? {
        return Ok(());
    }
    send_system_text_to_target(
        plugin_host,
        target,
        &format!(
            "Resumed session {} in thread {}.",
            session.session_id,
            resumed.state().await.thread_id
        ),
    )
    .await;
    Ok(())
}

pub async fn start_runtime_and_notify(
    workspace_threads: &Arc<WorkspaceThreadManager>,
    runtime: &Arc<ThreadRuntime>,
    plugin_host: &Arc<PluginHost>,
    target: &ChannelTarget,
    force_session_ready: bool,
) -> acp::Result<bool> {
    let route = &target.route;
    let before = runtime.state().await;
    if let Err(message) =
        crate::resources::validate_acp_runtime_agent(&before.host_binding.agent_id)
    {
        tracing::info!(
            agent_id = %before.host_binding.agent_id,
            route = %route,
            "rejected non-ACP runtime agent for channel route"
        );
        send_system_text_to_target(plugin_host, target, &message).await;
        return Ok(false);
    }
    if before.initialize.is_none() {
        workspace_threads
            .reset_thread_attachments_for_host_start(&before.thread_id, Some(route))
            .await
            .map_err(internal_error)?;
    }
    let handler = bridge_handler(workspace_threads, plugin_host, runtime, &before);
    let session_id = runtime.start(route, handler).await?;
    let after = runtime.state().await;
    let session_was_resumed = before.session_id.as_deref() == Some(session_id.as_str());

    let agent_info = after
        .initialize
        .as_ref()
        .and_then(|initialize| initialize.agent_info.as_ref());
    let agent = agent_info
        .map(|info| info.title.clone().unwrap_or_else(|| info.name.clone()))
        .unwrap_or_else(|| after.host_binding.agent_id.clone());
    let version = agent_info
        .map(|info| info.version.clone())
        .unwrap_or_default();
    if before.initialize.is_none() {
        plugin_host
            .send_output(ChannelOutput::AgentReady {
                route: route.clone(),
                agent: agent.clone(),
                version: version.clone(),
            })
            .await;
    }

    let should_send_session_ready = force_session_ready
        || before.initialize.is_none()
        || before.session_id.as_deref() != Some(session_id.as_str());
    if should_send_session_ready {
        plugin_host
            .send_output(ChannelOutput::SessionReady {
                route: route.clone(),
                session_id: session_id.clone(),
            })
            .await;
        plugin_host
            .send_output(ChannelOutput::SessionInfo {
                route: route.clone(),
                info: ChannelSessionInfo {
                    workspace_id: after.workspace_id.to_string(),
                    workspace_path: after.workspace.to_string_lossy().into_owned(),
                    thread_id: after.thread_id.to_string(),
                    agent: ChannelSessionAgent {
                        id: after.host_binding.agent_id.clone(),
                        name: agent,
                        version,
                        profile_id: after.host_binding.profile_id.clone(),
                    },
                    session_id,
                    start: if session_was_resumed {
                        ChannelSessionStart::Resumed
                    } else {
                        ChannelSessionStart::New
                    },
                },
            })
            .await;
    }
    if should_send_session_ready && route_allows_startup_replay(route) {
        send_multi_agent_state_and_replay(workspace_threads, runtime, plugin_host, route, &after)
            .await;
    }
    workspace_threads.schedule_host_idle_shutdown(after.thread_id);
    Ok(true)
}

pub async fn send_runtime_multi_agent_state_and_replay(
    workspace_threads: &Arc<WorkspaceThreadManager>,
    runtime: &Arc<ThreadRuntime>,
    plugin_host: &Arc<PluginHost>,
    route: &RouteKey,
) {
    let state = runtime.state().await;
    send_multi_agent_state_and_replay(workspace_threads, runtime, plugin_host, route, &state).await;
}

async fn send_multi_agent_state_and_replay(
    workspace_threads: &Arc<WorkspaceThreadManager>,
    runtime: &Arc<ThreadRuntime>,
    plugin_host: &Arc<PluginHost>,
    route: &RouteKey,
    state: &ThreadRuntimeState,
) {
    send_multi_agent_state(plugin_host, route, state).await;
    replay_subagent_sessions(workspace_threads, runtime, plugin_host, route, state).await;
}

async fn send_multi_agent_state(
    plugin_host: &Arc<PluginHost>,
    route: &RouteKey,
    state: &ThreadRuntimeState,
) {
    for turn in &state.multi_agent_turns {
        let agents = state
            .agents
            .iter()
            .filter(|agent| turn.agent_ids.contains(&agent.id))
            .cloned()
            .collect();
        plugin_host
            .send_output(ChannelOutput::MultiAgentTurn {
                route: route.clone(),
                turn: turn.clone(),
                agents,
            })
            .await;
    }
}

async fn replay_subagent_sessions(
    workspace_threads: &Arc<WorkspaceThreadManager>,
    runtime: &Arc<ThreadRuntime>,
    plugin_host: &Arc<PluginHost>,
    route: &RouteKey,
    state: &ThreadRuntimeState,
) {
    for agent in &state.agents {
        let Some(session_id) = latest_subagent_session_id(agent) else {
            continue;
        };
        let tracker = Arc::new(SubagentReportTracker::new(agent.clone()));
        let handler = Arc::new(SubagentBridgeHandler::for_thread(
            Arc::clone(plugin_host),
            workspace_threads,
            state.thread_id.clone(),
            agent.clone(),
            tracker,
        ));
        if let Err(error) = runtime
            .replay_subagent_session(route, agent, session_id, handler)
            .await
        {
            tracing::debug!(
                thread_id = %state.thread_id,
                agent_id = %agent.id,
                error = %error.message,
                "failed to replay subagent session"
            );
        }
    }
}

fn latest_subagent_session_id(agent: &crate::workspace::threads::ThreadAgent) -> Option<String> {
    agent.session_id.clone()
}

fn bridge_handler(
    workspace_threads: &Arc<WorkspaceThreadManager>,
    plugin_host: &Arc<PluginHost>,
    runtime: &Arc<ThreadRuntime>,
    state: &ThreadRuntimeState,
) -> Arc<dyn AgentClientHandler> {
    Arc::new(ChannelBridgeHandler::for_thread(
        Arc::clone(plugin_host),
        workspace_threads,
        state.workspace_id.clone(),
        state.thread_id.clone(),
        state.host_binding.clone(),
        runtime.active_turn_target(),
    ))
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ThreadCommand {
    New,
    Close,
    Pair(String),
    Pickup(String),
    Status,
    Resource {
        kind: ResourceKind,
        action: ResourceAction,
    },
    SwitchWorkspace(String),
    SwitchHost {
        agent: String,
        profile: Option<String>,
    },
    AgentPassThrough(String),
    Help,
    Unknown(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ResourceKind {
    Workspace,
    Agent,
    Profile,
    Session,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ResourceAction {
    List,
    Switch(String),
}

fn parse_thread_command(text: &str) -> Option<ThreadCommand> {
    let trimmed = text.trim();
    let normalized = trimmed.split_whitespace().collect::<Vec<_>>().join(" ");
    let normalized = canonical_thread_command(&normalized);
    let normalized = normalized.as_str();
    if !normalized.starts_with('/') {
        return None;
    }
    if normalized == "/new" {
        return Some(ThreadCommand::New);
    }
    if normalized == "/close" {
        return Some(ThreadCommand::Close);
    }
    if let Some(code) = normalized.strip_prefix("/pair ") {
        let code = code.trim();
        if !code.is_empty() {
            return Some(ThreadCommand::Pair(code.to_string()));
        }
    }
    if let Some(code) = normalized.strip_prefix("/pickup ") {
        let code = code.trim();
        if !code.is_empty() {
            return Some(ThreadCommand::Pickup(code.to_string()));
        }
    }
    if normalized == "/status" {
        return Some(ThreadCommand::Status);
    }
    if normalized == "/help" || normalized == "/commands" {
        return Some(ThreadCommand::Help);
    }
    if let Some(command) = parse_resource_command(normalized) {
        return Some(command);
    }
    if let Some(token) = normalized.strip_prefix("/switch workspace ") {
        let token = token.trim();
        if !token.is_empty() {
            return Some(ThreadCommand::SwitchWorkspace(token.to_string()));
        }
    }
    if let Some(rest) = normalized.strip_prefix("/switch host ") {
        if let Some(command) = parse_switch_host(rest) {
            return Some(command);
        }
    }
    if let Some(rest) = normalized.strip_prefix("/switch ") {
        let rest = rest.trim();
        if rest != "host" && rest != "workspace" {
            if let Some(command) = parse_switch_host(rest) {
                return Some(command);
            }
        }
    }
    Some(ThreadCommand::Unknown(normalized.to_string()))
}

fn parse_resource_command(normalized: &str) -> Option<ThreadCommand> {
    for (prefix, kind) in [
        ("/workspace", ResourceKind::Workspace),
        ("/profile", ResourceKind::Profile),
        ("/session", ResourceKind::Session),
    ] {
        if normalized == prefix {
            return Some(ThreadCommand::Resource {
                kind,
                action: ResourceAction::List,
            });
        }
        if let Some(rest) = normalized.strip_prefix(&format!("{prefix} ")) {
            return parse_resource_action(kind, rest.trim())
                .or_else(|| Some(ThreadCommand::Unknown(normalized.to_string())));
        }
    }

    if normalized == "/agent" {
        return Some(ThreadCommand::Resource {
            kind: ResourceKind::Agent,
            action: ResourceAction::List,
        });
    }
    if let Some(rest) = normalized.strip_prefix("/agent ") {
        let rest = rest.trim();
        if rest == "--list" {
            return Some(ThreadCommand::Resource {
                kind: ResourceKind::Agent,
                action: ResourceAction::List,
            });
        }
        if let Some(id) = parse_switch_value(rest) {
            return Some(ThreadCommand::Resource {
                kind: ResourceKind::Agent,
                action: ResourceAction::Switch(id),
            });
        }
        if rest == "--help" || rest == "help" {
            return Some(ThreadCommand::AgentPassThrough("/help".to_string()));
        }
        if !rest.is_empty() {
            return Some(ThreadCommand::AgentPassThrough(agent_command_text(rest)));
        }
    }

    None
}

fn parse_resource_action(kind: ResourceKind, rest: &str) -> Option<ThreadCommand> {
    if rest == "--list" {
        return Some(ThreadCommand::Resource {
            kind,
            action: ResourceAction::List,
        });
    }
    parse_switch_value(rest).map(|id| ThreadCommand::Resource {
        kind,
        action: ResourceAction::Switch(id),
    })
}

fn parse_switch_value(rest: &str) -> Option<String> {
    let value = rest.strip_prefix("--switch ")?.trim();
    (!value.is_empty()).then(|| value.to_string())
}

fn parse_switch_host(rest: &str) -> Option<ThreadCommand> {
    let mut parts = rest.split_whitespace();
    let agent_token = parts.next()?;
    let explicit_profile = parts.next().map(ToOwned::to_owned);
    let (agent, profile) = match agent_token.split_once('+') {
        Some((agent, profile)) => {
            let profile = explicit_profile.or_else(|| {
                let profile = profile.trim();
                (!profile.is_empty()).then(|| profile.to_string())
            });
            (agent.trim(), profile)
        }
        None => (agent_token.trim(), explicit_profile),
    };
    if agent.is_empty() {
        return None;
    }
    Some(ThreadCommand::SwitchHost {
        agent: agent.to_string(),
        profile,
    })
}

fn canonical_thread_command(normalized: &str) -> String {
    for prefix in ["/va", "/vibearound", "va", "vibearound"] {
        if normalized == prefix {
            return "/help".to_string();
        }
        let prefix_with_space = format!("{prefix} ");
        if let Some(rest) = normalized.strip_prefix(prefix_with_space.as_str()) {
            let rest = rest.trim();
            if rest.starts_with('/') {
                return rest.to_string();
            }
            return format!("/{rest}");
        }
    }
    normalized.to_string()
}

fn agent_command_text(rest: &str) -> String {
    let trimmed = rest.trim();
    if trimmed.starts_with('/') {
        trimmed.to_string()
    } else {
        format!("/{trimmed}")
    }
}

fn commands_enabled_for_route(_route: &RouteKey) -> bool {
    true
}

fn first_text(content_blocks: &[acp::ContentBlock]) -> Option<String> {
    content_blocks.iter().find_map(|block| match block {
        acp::ContentBlock::Text(text) => Some(text.text.clone()),
        _ => None,
    })
}

fn command_help_text() -> &'static str {
    "Commands:
/status
/workspace [--list]
/workspace --switch <workspace_id>
/agent [--list]
/agent --switch <agent_id>
/agent <agent-command>
/profile [--list]
/profile --switch <profile_id>
/session [--list]
/session --switch <session_id>
/pair <code>
/pickup <code>
/new
/stop
/close

Bare /workspace, /agent, /profile, and /session default to --list. Prefix with /va, /vibearound, va, or vibearound when a channel cannot send slash commands. Legacy /switch commands still work."
}

fn format_status(state: &ThreadRuntimeState) -> String {
    format!(
        "Status:\nWorkspace: {} · {}\nThread: {}\nAgent: {}\nProfile: {}\nSession: {}",
        state.workspace_id,
        state.workspace.to_string_lossy(),
        state.thread_id,
        state.host_binding.agent_id,
        state
            .host_binding
            .profile_id
            .as_deref()
            .unwrap_or(DIRECT_PROFILE_ID),
        state.session_id.as_deref().unwrap_or("(not started)")
    )
}

fn format_workspace_list(
    workspaces: &[crate::workspace::WorkspaceRecord],
    current_id: &str,
) -> String {
    let mut lines = vec!["Workspaces:".to_string()];
    if workspaces.is_empty() {
        lines.push("(none)".to_string());
        return lines.join("\n");
    }
    for workspace in workspaces {
        lines.push(format!(
            "{} {} · {} · {}",
            current_marker(workspace.id.as_str() == current_id),
            workspace.id,
            workspace.name,
            workspace.cwd.to_string_lossy()
        ));
    }
    lines.push("Switch with /workspace --switch <workspace_id>.".to_string());
    lines.join("\n")
}

fn format_agent_list(current_id: &str) -> String {
    let cfg = crate::config::ensure_loaded();
    let agent_ids = if cfg.enabled_agents.is_empty() {
        crate::resources::agent_ids()
            .into_iter()
            .map(ToOwned::to_owned)
            .collect()
    } else {
        cfg.enabled_agents.clone()
    };
    let mut lines = vec!["Agents:".to_string()];
    for id in agent_ids {
        let label = crate::resources::agent_by_id(&id)
            .map(|agent| agent.display_name.as_str())
            .unwrap_or(id.as_str());
        lines.push(format!(
            "{} {} · {}",
            current_marker(id == current_id),
            id,
            label
        ));
    }
    lines.push("Switch with /agent --switch <agent_id>.".to_string());
    lines.join("\n")
}

fn format_profile_list(state: &ThreadRuntimeState) -> String {
    let current = state
        .host_binding
        .profile_id
        .as_deref()
        .unwrap_or(DIRECT_PROFILE_ID);
    let agent = state.host_binding.agent_id.as_str();
    let mut lines = vec![format!("Profiles for {}:", agent)];
    lines.push(format!(
        "{} {} · Launch without VibeAround profile",
        current_marker(current == DIRECT_PROFILE_ID),
        DIRECT_PROFILE_ID
    ));
    for profile in profiles::ordered_profiles() {
        let compatible = connections::profile_can_launch_agent(&profile, agent);
        let suffix = if compatible { "" } else { " · incompatible" };
        lines.push(format!(
            "{} {} · {} · {}{}",
            current_marker(profile.id == current),
            profile.id,
            profile.label,
            profile.provider,
            suffix
        ));
    }
    lines.push("Switch with /profile --switch <profile_id>.".to_string());
    lines.join("\n")
}

async fn list_sessions_for_state(
    workspace_threads: &Arc<WorkspaceThreadManager>,
    state: &ThreadRuntimeState,
) -> Vec<crate::launch_sessions::LaunchSession> {
    workspace_threads
        .list_resumable_agent_sessions(
            &state.host_binding.agent_id,
            &state.workspace,
            SESSION_LIST_LIMIT,
            false,
        )
        .await
        .unwrap_or_else(|error| {
            tracing::warn!(
                agent_id = %state.host_binding.agent_id,
                workspace = %state.workspace.display(),
                error = %error,
                "failed to list resumable sessions"
            );
            Vec::new()
        })
}

fn format_session_list(
    state: &ThreadRuntimeState,
    sessions: &[crate::launch_sessions::LaunchSession],
) -> String {
    let mut lines = vec![format!(
        "Sessions for {} in {}:",
        state.host_binding.agent_id,
        state.workspace.to_string_lossy()
    )];
    if sessions.is_empty() {
        lines.push("(none)".to_string());
        return lines.join("\n");
    }
    let current = state.session_id.as_deref();
    for session in sessions {
        let short_id = crate::launch_sessions::short_id(&session.session_id);
        lines.push(format!(
            "{} {} · {} · updated {}",
            current_marker(current == Some(session.session_id.as_str())),
            short_id,
            session.title,
            session.updated_at
        ));
    }
    lines.push("Switch with /session --switch <session_id>.".to_string());
    lines.join("\n")
}

fn validate_profile_for_agent(profile_id: &str, agent_id: &str) -> Result<(), String> {
    let profile_id = normalize_launch_profile_id(Some(profile_id));
    if profile_id == DIRECT_PROFILE_ID {
        return Ok(());
    }
    let Some(profile) =
        profiles::schema::load(&profile_id).map(profiles::normalize_legacy_profile_and_persist)
    else {
        return Err(format!("Profile '{}' was not found.", profile_id));
    };
    if !connections::profile_can_launch_agent(&profile, agent_id) {
        return Err(format!(
            "Profile '{}' cannot launch agent '{}'.",
            profile_id, agent_id
        ));
    }
    Ok(())
}

fn current_marker(current: bool) -> &'static str {
    if current {
        "*"
    } else {
        "-"
    }
}

fn internal_error(error: anyhow::Error) -> acp::Error {
    acp::Error::new(-32603, format!("{:#}", error))
}

fn invalid_params(error: String) -> acp::Error {
    acp::Error::new(-32602, error)
}

fn resolve_host_binding(agent: &str, profile: Option<&str>) -> Result<HostBinding, String> {
    let agent_id = crate::resources::resolve_agent_id(agent)?;
    let profile_id = crate::agent::launch::normalize_launch_profile_id(profile);
    Ok(HostBinding::new(agent_id, Some(profile_id)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_new_thread_commands() {
        assert_eq!(parse_thread_command("/new"), Some(ThreadCommand::New));
        assert_eq!(parse_thread_command("/close"), Some(ThreadCommand::Close));
        assert_eq!(parse_thread_command("/va"), Some(ThreadCommand::Help));
        assert_eq!(parse_thread_command("/va new"), Some(ThreadCommand::New));
        assert_eq!(
            parse_thread_command("  /pair   049778  "),
            Some(ThreadCommand::Pair("049778".to_string()))
        );
        assert_eq!(
            parse_thread_command("/vibearound pair 049778"),
            Some(ThreadCommand::Pair("049778".to_string()))
        );
        assert_eq!(
            parse_thread_command("va pair 049778"),
            Some(ThreadCommand::Pair("049778".to_string()))
        );
        assert_eq!(
            parse_thread_command("\n/pickup   B8LX  "),
            Some(ThreadCommand::Pickup("B8LX".to_string()))
        );
        assert_eq!(parse_thread_command("/status"), Some(ThreadCommand::Status));
        assert_eq!(
            parse_thread_command("/workspace"),
            Some(ThreadCommand::Resource {
                kind: ResourceKind::Workspace,
                action: ResourceAction::List
            })
        );
        assert_eq!(
            parse_thread_command("vibearound workspace"),
            Some(ThreadCommand::Resource {
                kind: ResourceKind::Workspace,
                action: ResourceAction::List
            })
        );
        assert_eq!(
            parse_thread_command("/workspace --switch ws_a"),
            Some(ThreadCommand::Resource {
                kind: ResourceKind::Workspace,
                action: ResourceAction::Switch("ws_a".to_string())
            })
        );
        assert_eq!(
            parse_thread_command("/agent"),
            Some(ThreadCommand::Resource {
                kind: ResourceKind::Agent,
                action: ResourceAction::List
            })
        );
        assert_eq!(
            parse_thread_command("/agent --switch codex"),
            Some(ThreadCommand::Resource {
                kind: ResourceKind::Agent,
                action: ResourceAction::Switch("codex".to_string())
            })
        );
        assert_eq!(
            parse_thread_command("/agent status"),
            Some(ThreadCommand::AgentPassThrough("/status".to_string()))
        );
        assert_eq!(
            parse_thread_command("va agent /status"),
            Some(ThreadCommand::AgentPassThrough("/status".to_string()))
        );
        assert_eq!(
            parse_thread_command("/profile --switch deepseek"),
            Some(ThreadCommand::Resource {
                kind: ResourceKind::Profile,
                action: ResourceAction::Switch("deepseek".to_string())
            })
        );
        assert_eq!(
            parse_thread_command("/session --switch 6f94"),
            Some(ThreadCommand::Resource {
                kind: ResourceKind::Session,
                action: ResourceAction::Switch("6f94".to_string())
            })
        );
        assert_eq!(
            parse_thread_command("/vibearound switch workspace general"),
            Some(ThreadCommand::SwitchWorkspace("general".to_string()))
        );
        assert_eq!(
            parse_thread_command("/switch   workspace   general"),
            Some(ThreadCommand::SwitchWorkspace("general".to_string()))
        );
        assert_eq!(
            parse_thread_command("/switch host codex profileA"),
            Some(ThreadCommand::SwitchHost {
                agent: "codex".to_string(),
                profile: Some("profileA".to_string())
            })
        );
        assert_eq!(
            parse_thread_command("/switch codex"),
            Some(ThreadCommand::SwitchHost {
                agent: "codex".to_string(),
                profile: None
            })
        );
        assert_eq!(
            parse_thread_command("/switch codex profileA"),
            Some(ThreadCommand::SwitchHost {
                agent: "codex".to_string(),
                profile: Some("profileA".to_string())
            })
        );
        assert_eq!(
            parse_thread_command("/switch codex+profileA"),
            Some(ThreadCommand::SwitchHost {
                agent: "codex".to_string(),
                profile: Some("profileA".to_string())
            })
        );
        assert_eq!(
            parse_thread_command("/va switch codex"),
            Some(ThreadCommand::SwitchHost {
                agent: "codex".to_string(),
                profile: None
            })
        );
    }

    #[test]
    fn slash_commands_are_enabled_for_web_chat() {
        assert!(commands_enabled_for_route(&RouteKey::new("web", "chat-a")));
        assert!(commands_enabled_for_route(&RouteKey::new(
            "slack", "chat-a"
        )));
        assert!(commands_enabled_for_route(&RouteKey::new(
            "feishu", "chat-a"
        )));
    }
}
