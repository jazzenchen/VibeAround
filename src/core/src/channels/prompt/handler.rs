//! Workspace-thread prompt handling for channel inputs.

use std::sync::Arc;

use agent_client_protocol::schema as acp;

use crate::agent::AgentClientHandler;
use crate::channels::bridge_handler::ChannelBridgeHandler;
use crate::channels::plugin_host::PluginHost;
use crate::channels::subagent_handler::{SubagentBridgeHandler, SubagentReportTracker};
use crate::channels::types::{
    ChannelOutput, ChannelSessionAgent, ChannelSessionInfo, ChannelSessionStart,
};
use crate::routing::RouteKey;
use crate::workspace::context_transfer;
use crate::workspace::manager::{PendingThreadSelection, ThreadChoice, WorkspaceSwitch};
use crate::workspace::threads::runtime::{ThreadRuntime, ThreadRuntimeState};
use crate::workspace::threads::store::HostBinding;
use crate::workspace::WorkspaceThreadManager;

use super::send_system_text;

pub(crate) async fn handle_prompt(
    workspace_threads: &Arc<WorkspaceThreadManager>,
    plugin_host: &Arc<PluginHost>,
    route: RouteKey,
    mut content_blocks: Vec<acp::ContentBlock>,
) -> acp::Result<acp::PromptResponse> {
    let text = first_text(&content_blocks).unwrap_or_default();

    if commands_enabled_for_route(&route) {
        if let Some(command) = parse_thread_command(&text) {
            return handle_command(workspace_threads, plugin_host, &route, command).await;
        }
    }

    if commands_enabled_for_route(&route) {
        match workspace_threads
            .select_pending_thread(&route, &text)
            .await
            .map_err(internal_error)?
        {
            PendingThreadSelection::Selected(runtime) => {
                start_runtime_and_notify(workspace_threads, &runtime, plugin_host, &route, true)
                    .await?;
                send_system_text(
                    plugin_host,
                    &route,
                    &format!("Switched to thread {}.", runtime.state().await.thread_id),
                )
                .await;
                return Ok(acp::PromptResponse::new(acp::StopReason::EndTurn));
            }
            PendingThreadSelection::Invalid { threads } => {
                send_system_text(
                    plugin_host,
                    &route,
                    &format!(
                        "Invalid thread selection.\n{}",
                        format_thread_choices("workspace", &threads)
                    ),
                )
                .await;
                return Ok(acp::PromptResponse::new(acp::StopReason::EndTurn));
            }
            PendingThreadSelection::NoPending => {}
        }
    }

    if content_blocks.is_empty() {
        return Ok(acp::PromptResponse::new(acp::StopReason::EndTurn));
    }

    let runtime = workspace_threads
        .resolve_route_runtime(&route)
        .await
        .map_err(internal_error)?;
    start_runtime_and_notify(workspace_threads, &runtime, plugin_host, &route, false).await?;
    let state = runtime.state().await;
    let handler = bridge_handler(workspace_threads, plugin_host, &state);
    runtime
        .prompt(&route, std::mem::take(&mut content_blocks), handler)
        .await
}

async fn handle_command(
    workspace_threads: &Arc<WorkspaceThreadManager>,
    plugin_host: &Arc<PluginHost>,
    route: &RouteKey,
    command: ThreadCommand,
) -> acp::Result<acp::PromptResponse> {
    match command {
        ThreadCommand::New => {
            let runtime = workspace_threads
                .create_thread_in_current_workspace(route)
                .await
                .map_err(internal_error)?;
            start_runtime_and_notify(workspace_threads, &runtime, plugin_host, route, true).await?;
            send_system_text(
                plugin_host,
                route,
                &format!("Started new thread {}.", runtime.state().await.thread_id),
            )
            .await;
        }
        ThreadCommand::Close => {
            workspace_threads
                .close_route(route, Some("user closed".to_string()))
                .await
                .map_err(internal_error)?;
            send_system_text(
                plugin_host,
                route,
                "Thread closed. Use /new to start again.",
            )
            .await;
        }
        ThreadCommand::Pickup(code) => {
            let Some((agent, session_id, cwd)) = crate::workspace::handoff::consume(&code) else {
                send_system_text(plugin_host, route, "Handoff code is invalid or expired.").await;
                return Ok(acp::PromptResponse::new(acp::StopReason::EndTurn));
            };
            let agent_id = crate::resources::resolve_agent_id(&agent).map_err(invalid_params)?;
            let runtime = workspace_threads
                .attach_external_session(
                    route,
                    agent_id.clone(),
                    None,
                    session_id,
                    std::path::PathBuf::from(cwd),
                )
                .await
                .map_err(internal_error)?;
            start_runtime_and_notify(workspace_threads, &runtime, plugin_host, route, true).await?;
            send_system_text(
                plugin_host,
                route,
                &format!("Attached handoff session to {}.", agent_id),
            )
            .await;
        }
        ThreadCommand::SwitchWorkspace(token) => {
            match workspace_threads
                .switch_workspace(route, &token)
                .await
                .map_err(internal_error)?
            {
                WorkspaceSwitch::Started(runtime) => {
                    start_runtime_and_notify(workspace_threads, &runtime, plugin_host, route, true)
                        .await?;
                    send_system_text(
                        plugin_host,
                        route,
                        &format!(
                            "Entered workspace and started thread {}.",
                            runtime.state().await.thread_id
                        ),
                    )
                    .await;
                }
                WorkspaceSwitch::NeedsSelection { workspace, threads } => {
                    send_system_text(
                        plugin_host,
                        route,
                        &format_thread_choices(&workspace.name, &threads),
                    )
                    .await;
                }
            }
        }
        ThreadCommand::SwitchHost { agent, profile } => {
            let target =
                resolve_host_binding(&agent, profile.as_deref()).map_err(invalid_params)?;
            let runtime = workspace_threads
                .resolve_route_runtime(route)
                .await
                .map_err(internal_error)?;
            let before = runtime.state().await;
            let package = if let Some(session_id) = before.session_id.as_deref() {
                match context_transfer::capture(
                    route,
                    &before.workspace,
                    &before.workspace_id,
                    &before.thread_id,
                    &before.host_binding,
                    session_id,
                    &target,
                )
                .await
                {
                    Ok(package) => Some(package),
                    Err(error) => {
                        send_system_text(
                            plugin_host,
                            route,
                            &format!(
                                "Context transfer failed; switching host without replay: {:#}",
                                error
                            ),
                        )
                        .await;
                        None
                    }
                }
            } else {
                None
            };

            runtime
                .switch_host(target.clone(), package.is_some())
                .await?;
            start_runtime_and_notify(workspace_threads, &runtime, plugin_host, route, true).await?;
            if let Some(package) = package {
                match context_transfer::bootstrap_prompt(&package) {
                    Ok(blocks) => {
                        let state = runtime.state().await;
                        let handler = bridge_handler(workspace_threads, plugin_host, &state);
                        if let Err(error) = runtime.prompt(route, blocks, handler).await {
                            send_system_text(
                                plugin_host,
                                route,
                                &format!(
                                    "Context transfer bootstrap failed after switching host: {}",
                                    error
                                ),
                            )
                            .await;
                        }
                    }
                    Err(error) => {
                        send_system_text(
                            plugin_host,
                            route,
                            &format!(
                                "Context transfer bootstrap could not be prepared after switching host: {:#}",
                                error
                            ),
                        )
                        .await;
                    }
                }
            }
            send_system_text(
                plugin_host,
                route,
                &format!("Switched host to {}.", target.agent_id),
            )
            .await;
        }
        ThreadCommand::Help => {
            send_system_text(
                plugin_host,
                route,
                "Commands: /switch workspace <id|path>, /switch host <agent> [profile], /pickup <code>, /new, /close. You can also prefix commands with /va or /vibearound. Reply with a listed number or thread id after switching workspace.",
            )
            .await;
        }
        ThreadCommand::Unknown(command) => {
            send_system_text(plugin_host, route, &format!("Unknown command: {}", command)).await;
        }
    }
    Ok(acp::PromptResponse::new(acp::StopReason::EndTurn))
}

pub async fn start_runtime_and_notify(
    workspace_threads: &Arc<WorkspaceThreadManager>,
    runtime: &Arc<ThreadRuntime>,
    plugin_host: &Arc<PluginHost>,
    route: &RouteKey,
    force_session_ready: bool,
) -> acp::Result<()> {
    let before = runtime.state().await;
    let handler = bridge_handler(workspace_threads, plugin_host, &before);
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
    if should_send_session_ready {
        send_multi_agent_state_and_replay(workspace_threads, runtime, plugin_host, route, &after)
            .await;
    }
    workspace_threads.schedule_host_idle_shutdown(after.thread_id);
    Ok(())
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
    state: &ThreadRuntimeState,
) -> Arc<dyn AgentClientHandler> {
    Arc::new(ChannelBridgeHandler::for_thread(
        Arc::clone(plugin_host),
        workspace_threads,
        state.workspace_id.clone(),
        state.thread_id.clone(),
        state.host_binding.clone(),
    ))
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ThreadCommand {
    New,
    Close,
    Pickup(String),
    SwitchWorkspace(String),
    SwitchHost {
        agent: String,
        profile: Option<String>,
    },
    Help,
    Unknown(String),
}

fn parse_thread_command(text: &str) -> Option<ThreadCommand> {
    let trimmed = text.trim();
    if !trimmed.starts_with('/') {
        return None;
    }
    let normalized = trimmed.split_whitespace().collect::<Vec<_>>().join(" ");
    let normalized = canonical_thread_command(&normalized);
    let normalized = normalized.as_str();
    if normalized == "/new" {
        return Some(ThreadCommand::New);
    }
    if normalized == "/close" {
        return Some(ThreadCommand::Close);
    }
    if let Some(code) = normalized.strip_prefix("/pickup ") {
        let code = code.trim();
        if !code.is_empty() {
            return Some(ThreadCommand::Pickup(code.to_string()));
        }
    }
    if normalized == "/help" || normalized == "/commands" {
        return Some(ThreadCommand::Help);
    }
    if let Some(token) = normalized.strip_prefix("/switch workspace ") {
        let token = token.trim();
        if !token.is_empty() {
            return Some(ThreadCommand::SwitchWorkspace(token.to_string()));
        }
    }
    if let Some(rest) = normalized.strip_prefix("/switch host ") {
        let mut parts = rest.split_whitespace();
        if let Some(agent) = parts.next() {
            return Some(ThreadCommand::SwitchHost {
                agent: agent.to_string(),
                profile: parts.next().map(ToOwned::to_owned),
            });
        }
    }
    Some(ThreadCommand::Unknown(normalized.to_string()))
}

fn canonical_thread_command(normalized: &str) -> String {
    for prefix in ["/va", "/vibearound"] {
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

fn commands_enabled_for_route(route: &RouteKey) -> bool {
    route.channel_kind != "web"
}

fn first_text(content_blocks: &[acp::ContentBlock]) -> Option<String> {
    content_blocks.iter().find_map(|block| match block {
        acp::ContentBlock::Text(text) => Some(text.text.clone()),
        _ => None,
    })
}

fn format_thread_choices(workspace_name: &str, threads: &[ThreadChoice]) -> String {
    let mut lines = vec![format!("Threads in {}:", workspace_name)];
    for (index, thread) in threads.iter().enumerate() {
        let title = thread
            .first_user_prompt
            .as_deref()
            .filter(|prompt| !prompt.trim().is_empty())
            .unwrap_or("(no first prompt yet)");
        lines.push(format!(
            "{}. {} · {} · {}",
            index + 1,
            thread.thread_id,
            thread.host_binding.agent_id,
            title
        ));
    }
    lines.push("Reply with a number or thread id.".to_string());
    lines.join("\n")
}

fn internal_error(error: anyhow::Error) -> acp::Error {
    acp::Error::new(-32603, format!("{:#}", error))
}

fn invalid_params(error: String) -> acp::Error {
    acp::Error::new(-32602, error)
}

fn resolve_host_binding(agent: &str, profile: Option<&str>) -> Result<HostBinding, String> {
    let agent_id = crate::resources::resolve_agent_id(agent)?;
    let profile_id = profile
        .map(str::trim)
        .filter(|profile| !profile.is_empty())
        .map(ToOwned::to_owned)
        .or_else(|| Some("direct".to_string()));
    Ok(HostBinding::new(agent_id, profile_id))
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
    }

    #[test]
    fn slash_commands_are_not_enabled_for_web_chat() {
        assert!(!commands_enabled_for_route(&RouteKey::new("web", "chat-a")));
        assert!(commands_enabled_for_route(&RouteKey::new(
            "slack", "chat-a"
        )));
        assert!(commands_enabled_for_route(&RouteKey::new(
            "feishu", "chat-a"
        )));
    }
}
