//! WebSocket handler for web chat channel.
//!
//! - GET /ws/chat — ACP-native websocket adapter
//!
//! Inbound user messages are dispatched to workspace threads via the channel-input
//! task (fire-and-forget through ChannelManager). ACP events flow back
//! through the WebChannelManager outbound channel to the websocket,
//! wrapped in a tagged [`crate::api_types::ChatEvent`] envelope so the
//! frontend can discriminate exhaustively.

use axum::extract::{
    ws::{Message, WebSocket, WebSocketUpgrade},
    Query, State,
};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use futures_util::{SinkExt, StreamExt};
use tokio::task::JoinHandle;
use uuid::Uuid;

use common::channels::{ChannelEnvelope, ChannelInput, ChannelOutput};
use common::routing::{
    is_external_attachment_uri, is_safe_attachment_file_key, Attachment, RouteKey,
};
use common::workspace::manager::ExternalSessionAttachMode;
use common::workspace::threads::HostBinding;
use common::{agent_state, config};

use crate::api_types::{AgentInfo, ChatEvent};

use super::AppState;

mod input;

use input::{parse_web_chat_input, WebChatInput, WebChatSessionIntent};

/// WebSocket upgrade handler for web chat.
pub async fn ws_chat_handler(
    State(state): State<AppState>,
    Query(query): Query<WsChatQuery>,
    headers: HeaderMap,
    ws: WebSocketUpgrade,
) -> Response {
    let tunnel_urls = state.tunnels.public_urls();
    if !super::auth::headers_have_allowed_ws_origin(&headers, state.port, &tunnel_urls) {
        return StatusCode::FORBIDDEN.into_response();
    }

    let Ok(client) = ChatSocketClient::from_query(query.channel.as_deref()) else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    let chat_id = sanitize_chat_id(query.chat_id.as_deref());

    ws.on_upgrade(move |socket| handle_chat_socket(socket, state, client, chat_id))
}

#[derive(Debug, serde::Deserialize)]
pub(crate) struct WsChatQuery {
    channel: Option<String>,
    chat_id: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ChatSocketClient {
    channel_kind: &'static str,
    sender_id: &'static str,
}

impl ChatSocketClient {
    fn from_query(channel: Option<&str>) -> Result<Self, ()> {
        match channel.map(str::trim).filter(|value| !value.is_empty()) {
            None | Some("web") => Ok(Self {
                channel_kind: "web",
                sender_id: "web-user",
            }),
            Some("tui") => Ok(Self {
                channel_kind: "tui",
                sender_id: "tui-user",
            }),
            Some(_) => Err(()),
        }
    }
}

fn sanitize_chat_id(value: Option<&str>) -> Option<String> {
    let value = value?.trim();
    if value.is_empty() || value.len() > 128 {
        return None;
    }
    if value
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_')
    {
        Some(value.to_string())
    } else {
        None
    }
}

fn should_replay_initial_route_history(chat_id: &Option<String>) -> bool {
    chat_id.is_some()
}

async fn handle_chat_socket(
    socket: WebSocket,
    state: AppState,
    client: ChatSocketClient,
    chat_id: Option<String>,
) {
    let connection_id = Uuid::new_v4().to_string();
    let replay_history = should_replay_initial_route_history(&chat_id);
    let chat_id = chat_id.unwrap_or_else(|| Uuid::new_v4().to_string());
    let channel_id = format!("{}:{}", client.channel_kind, chat_id);
    let mut active_route = RouteKey::new(client.channel_kind, &chat_id);

    // Explicit chat_id attachments are reconnects or existing thread views, so
    // replay the bounded route history independent of runtime lifetime.
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<ChannelOutput>();
    state.web_channel.register_connection(
        &active_route,
        connection_id.clone(),
        tx.clone(),
        replay_history,
    );
    let (event_tx, mut event_rx) = tokio::sync::mpsc::unbounded_channel::<ChatEvent>();

    let (mut ws_tx, mut ws_rx) = socket.split();

    // Load config for initial agent metadata.
    let cfg = config::ensure_loaded();
    let agent_prefs = agent_state::read_prefs();

    // Send initial config event.
    let config_event = ChatEvent::Config {
        channel_id: channel_id.clone(),
        agents: AgentInfo::for_ids(&cfg.enabled_agents),
        default_agent: agent_state::resolve_default_agent(&agent_prefs, &cfg),
    };
    if send_event(&mut ws_tx, &config_event).await.is_err() {
        state
            .web_channel
            .unregister_connection(&active_route.chat_id, &connection_id);
        return;
    }

    // Outbound: drain ChannelOutput → ChatEvent → websocket.
    let outbound_task = tokio::spawn(async move {
        loop {
            tokio::select! {
                Some(output) = rx.recv() => {
                    let event = output_to_chat_event(output);
                    if send_event(&mut ws_tx, &event).await.is_err() {
                        break;
                    }
                }
                Some(event) = event_rx.recv() => {
                    if send_event(&mut ws_tx, &event).await.is_err() {
                        break;
                    }
                }
                else => break,
            }
        }
    });

    // Inbound: ws messages → channel-input thread / permission bridge.
    let mut direct_resume_task: Option<JoinHandle<()>> = None;
    while let Some(Ok(msg)) = ws_rx.next().await {
        match msg {
            Message::Text(text) => {
                if let Some(input) = parse_web_chat_input(&active_route, client.sender_id, &text) {
                    match input {
                        WebChatInput::Message {
                            input,
                            profile,
                            session_intent,
                            session_mode,
                        } => {
                            abort_direct_resume_task(
                                &mut direct_resume_task,
                                &state,
                                &active_route,
                            )
                            .await;
                            let mut dispatch_input = true;
                            if let Some(route) = input_route(&input) {
                                state.web_channel.mark_route_active(&route);
                                remember_web_route_agent(&state, &route, input_agent(&input)).await;
                                let wait_for_session_ready = should_wait_for_user_message_session(
                                    &state,
                                    &route,
                                    &session_intent,
                                );
                                match session_intent {
                                    Some(WebChatSessionIntent::Resume {
                                        agent,
                                        session_id,
                                        cwd,
                                    }) => {
                                        apply_web_session_resume(
                                            &state, &route, agent, profile, session_id, cwd,
                                        )
                                        .await;
                                    }
                                    Some(WebChatSessionIntent::New { cwd }) => {
                                        let manager = state.channel_hub.workspace_thread_manager();
                                        let created = match cwd {
                                            Some(cwd) => {
                                                manager
                                                    .create_thread_for_cwd(
                                                        &route,
                                                        std::path::PathBuf::from(cwd),
                                                    )
                                                    .await
                                            }
                                            None => {
                                                manager
                                                    .create_thread_in_current_workspace(&route)
                                                    .await
                                            }
                                        };
                                        match created {
                                            Ok(_) => {
                                                apply_web_launch_selection(
                                                    &state, &route, &input, profile, None,
                                                )
                                                .await;
                                            }
                                            Err(error) => {
                                                dispatch_input = false;
                                                send_web_system_text(
                                                    &state,
                                                    &route,
                                                    &format!("❌ {}", error),
                                                )
                                                .await;
                                            }
                                        }
                                    }
                                    None => {
                                        apply_web_launch_selection(
                                            &state, &route, &input, profile, None,
                                        )
                                        .await;
                                    }
                                }
                                if let Some(mode_id) = session_mode {
                                    apply_web_session_mode(&state, &route, &mode_id).await;
                                }
                                remember_web_user_message(
                                    &state,
                                    &route,
                                    &input,
                                    wait_for_session_ready,
                                );
                            }
                            if dispatch_input {
                                state.channel_hub.handle_input(input);
                            }
                        }
                        WebChatInput::SetMode { mode_id } => {
                            apply_web_session_mode(&state, &active_route, &mode_id).await;
                            if let Some(deadline) = state.web_channel.bump_idle_route(&active_route)
                            {
                                state.web_channel.schedule_idle_close(
                                    state.channel_hub.workspace_thread_manager(),
                                    deadline,
                                );
                            }
                        }
                        WebChatInput::SetConfigOption { config_id, value } => {
                            apply_web_session_config_option(
                                &state,
                                &active_route,
                                config_id,
                                value,
                            )
                            .await;
                            if let Some(deadline) = state.web_channel.bump_idle_route(&active_route)
                            {
                                state.web_channel.schedule_idle_close(
                                    state.channel_hub.workspace_thread_manager(),
                                    deadline,
                                );
                            }
                        }
                        WebChatInput::Stop(input) => {
                            abort_direct_resume_task(
                                &mut direct_resume_task,
                                &state,
                                &active_route,
                            )
                            .await;
                            let route = input_route(&input).unwrap_or_else(|| active_route.clone());
                            if let Err(error) = state
                                .channel_hub
                                .workspace_thread_manager()
                                .cancel_route(&route)
                                .await
                            {
                                tracing::warn!(
                                    route = %route,
                                    error = %error,
                                    "failed to cancel web chat route"
                                );
                            }
                            let deadline = state.web_channel.mark_route_idle(&active_route);
                            state.web_channel.schedule_idle_close(
                                state.channel_hub.workspace_thread_manager(),
                                deadline,
                            );
                        }
                        WebChatInput::PermissionResponse {
                            request_id,
                            response,
                        } => {
                            state.web_channel.clear_pending_permission(&request_id);
                            if let Err(error) = state.channel_hub.respond_permission(
                                &active_route.channel_kind,
                                &request_id,
                                response,
                            ) {
                                tracing::warn!(
                                    request_id = %request_id,
                                    error = %error,
                                    "web permission response ignored"
                                );
                                let _ = event_tx
                                    .send(permission_response_error_event(&request_id, &error));
                            }
                        }
                        WebChatInput::ResumeSession {
                            agent,
                            profile,
                            session_id,
                            cwd,
                        } => {
                            abort_direct_resume_task(
                                &mut direct_resume_task,
                                &state,
                                &active_route,
                            )
                            .await;
                            if let Some(agent_id) =
                                resolve_web_session_agent(&state, &active_route, agent.clone())
                                    .await
                            {
                                if let Some(route) =
                                    state.web_channel.route_for_session(&agent_id, &session_id)
                                {
                                    state.web_channel.unregister_connection(
                                        &active_route.chat_id,
                                        &connection_id,
                                    );
                                    active_route = route;
                                    state.web_channel.register_connection(
                                        &active_route,
                                        connection_id.clone(),
                                        tx.clone(),
                                        true,
                                    );
                                    let _ = tx.send(ChannelOutput::SessionReady {
                                        route: active_route.clone(),
                                        session_id,
                                    });
                                    if let Ok(runtime) = state
                                        .channel_hub
                                        .workspace_thread_manager()
                                        .resolve_route_runtime(&active_route)
                                        .await
                                    {
                                        let workspace_threads =
                                            state.channel_hub.workspace_thread_manager();
                                        common::channels::prompt::send_runtime_multi_agent_state_and_replay(
                                            &workspace_threads,
                                            &runtime,
                                            &state.channel_hub.plugin_host(),
                                            &active_route,
                                        )
                                        .await;
                                    }
                                    if let Some(deadline) =
                                        state.web_channel.bump_idle_route(&active_route)
                                    {
                                        state.web_channel.schedule_idle_close(
                                            state.channel_hub.workspace_thread_manager(),
                                            deadline,
                                        );
                                    }
                                    continue;
                                }
                            }
                            let task_state = state.clone();
                            let task_route = active_route.clone();
                            direct_resume_task = Some(tokio::spawn(async move {
                                apply_web_session_resume_now(
                                    &task_state,
                                    &task_route,
                                    agent,
                                    profile,
                                    session_id,
                                    cwd,
                                )
                                .await;
                                let deadline = task_state.web_channel.mark_route_idle(&task_route);
                                task_state.web_channel.schedule_idle_close(
                                    task_state.channel_hub.workspace_thread_manager(),
                                    deadline,
                                );
                            }));
                        }
                    }
                }
            }
            Message::Close(_) => break,
            _ => {}
        }
    }

    if let Some(task) = direct_resume_task.take() {
        task.abort();
    }
    outbound_task.abort();
    state
        .web_channel
        .unregister_connection(&active_route.chat_id, &connection_id);
    if !state.web_channel.route_has_session(&active_route.chat_id)
        && !state.web_channel.route_is_active(&active_route.chat_id)
    {
        let _ = state
            .channel_hub
            .workspace_thread_manager()
            .detach_route(&active_route)
            .await;
        state.web_channel.forget_route(&active_route.chat_id);
    }
}

async fn abort_direct_resume_task(
    task: &mut Option<JoinHandle<()>>,
    state: &AppState,
    route: &RouteKey,
) {
    let Some(handle) = task.take() else {
        return;
    };
    if handle.is_finished() {
        let _ = handle.await;
        return;
    }

    handle.abort();
    let _ = state
        .channel_hub
        .workspace_thread_manager()
        .detach_route(route)
        .await;
    state.web_channel.forget_route(&route.chat_id);
}

fn input_route(input: &ChannelInput) -> Option<RouteKey> {
    match input {
        ChannelInput::Message { envelope }
        | ChannelInput::Callback {
            envelope,
            action_value: _,
        } => Some(envelope.route.clone()),
        ChannelInput::Stop { route } | ChannelInput::Close { route, .. } => Some(route.clone()),
        ChannelInput::SwitchAgent { route, .. } => Some(route.clone()),
        ChannelInput::Log { .. } => None,
    }
}

fn input_agent(input: &ChannelInput) -> Option<String> {
    match input {
        ChannelInput::Message { envelope }
        | ChannelInput::Callback {
            envelope,
            action_value: _,
        } => envelope.cli_kind.clone(),
        _ => None,
    }
}

async fn remember_web_route_agent(state: &AppState, route: &RouteKey, agent: Option<String>) {
    if let Some(agent_id) = resolve_web_session_agent(state, route, agent).await {
        state.web_channel.set_route_agent(&route.chat_id, agent_id);
    }
}

fn should_wait_for_user_message_session(
    state: &AppState,
    route: &RouteKey,
    session_intent: &Option<WebChatSessionIntent>,
) -> bool {
    match session_intent {
        Some(WebChatSessionIntent::New { .. }) => true,
        Some(WebChatSessionIntent::Resume { session_id, .. }) => {
            state
                .web_channel
                .route_session_id(&route.chat_id)
                .as_deref()
                != Some(session_id.as_str())
        }
        None => false,
    }
}

fn remember_web_user_message(
    state: &AppState,
    route: &RouteKey,
    input: &ChannelInput,
    wait_for_session_ready: bool,
) {
    let ChannelInput::Message { envelope } = input else {
        return;
    };
    let content = web_user_message_content(envelope);
    state.web_channel.record_user_message(
        route,
        envelope.message_id.clone(),
        content,
        wait_for_session_ready,
    );
}

fn web_user_message_content(envelope: &ChannelEnvelope) -> Vec<serde_json::Value> {
    let mut blocks =
        Vec::with_capacity(usize::from(!envelope.text.is_empty()) + envelope.attachments.len());
    if !envelope.text.is_empty() {
        blocks.push(serde_json::json!({
            "type": "text",
            "text": envelope.text.clone(),
        }));
    }
    blocks.extend(
        envelope
            .attachments
            .iter()
            .filter_map(web_attachment_content_block),
    );
    blocks
}

fn web_attachment_content_block(attachment: &Attachment) -> Option<serde_json::Value> {
    let uri = match web_attachment_uri(&attachment.file_key) {
        Some(uri) => uri,
        None => {
            tracing::warn!(
                file_key = %attachment.file_key,
                "dropping web attachment with unsafe file key"
            );
            return None;
        }
    };
    let mut block = serde_json::Map::new();
    block.insert(
        "type".to_string(),
        serde_json::Value::String("resource_link".to_string()),
    );
    block.insert(
        "name".to_string(),
        serde_json::Value::String(attachment.file_name.clone()),
    );
    block.insert(
        "title".to_string(),
        serde_json::Value::String(attachment.file_name.clone()),
    );
    block.insert("uri".to_string(), serde_json::Value::String(uri));
    if !attachment.resource_type.trim().is_empty() {
        block.insert(
            "mimeType".to_string(),
            serde_json::Value::String(attachment.resource_type.clone()),
        );
    }
    if let Some(size) = attachment.size {
        block.insert("size".to_string(), serde_json::Value::Number(size.into()));
    }
    Some(serde_json::Value::Object(block))
}

fn web_attachment_uri(file_key: &str) -> Option<String> {
    if is_external_attachment_uri(file_key) {
        return Some(file_key.to_string());
    }
    if !is_safe_attachment_file_key(file_key) {
        return None;
    }
    Some(format!(
        "file://{}",
        config::data_dir()
            .join(".cache")
            .join(file_key)
            .to_string_lossy()
    ))
}

async fn resolve_web_session_agent(
    state: &AppState,
    route: &RouteKey,
    agent: Option<String>,
) -> Option<String> {
    let current_state = state
        .channel_hub
        .workspace_thread_manager()
        .resolve_route_runtime(route)
        .await
        .ok()
        .map(|runtime| async move { runtime.state().await });
    let current_state = match current_state {
        Some(state) => Some(state.await),
        None => None,
    };
    let cfg = config::ensure_loaded();
    let agent_prefs = agent_state::read_prefs();
    let agent = agent
        .or_else(|| {
            current_state
                .as_ref()
                .map(|state| state.host_binding.agent_id.clone())
        })
        .unwrap_or_else(|| agent_state::resolve_default_agent(&agent_prefs, &cfg));
    match common::resources::resolve_agent_id(&agent) {
        Ok(agent_id) => Some(agent_id),
        Err(error) => {
            tracing::warn!(route = %route, agent = %agent, error = %error, "web chat agent resolution failed");
            None
        }
    }
}

async fn apply_web_launch_selection(
    state: &AppState,
    route: &RouteKey,
    input: &ChannelInput,
    profile: Option<String>,
    workspace: Option<String>,
) {
    let Some(agent) = input_agent(input) else {
        return;
    };
    if profile.is_none() && workspace.is_none() {
        return;
    }
    if let Some(workspace) = workspace {
        if let Err(error) = state
            .channel_hub
            .workspace_thread_manager()
            .switch_workspace(route, &workspace)
            .await
        {
            send_web_system_text(state, route, &format!("❌ {}", error)).await;
        }
    }
    if let Some(profile) = profile {
        let Ok(agent_id) = common::resources::resolve_agent_id(&agent) else {
            send_web_system_text(state, route, &format!("❌ Unknown agent `{}`.", agent)).await;
            return;
        };
        let target = HostBinding::new(agent_id.clone(), Some(profile));
        let workspace_threads = state.channel_hub.workspace_thread_manager();
        match workspace_threads.active_route_runtime(route).await {
            Ok(Some(runtime)) => {
                if runtime.state().await.host_binding.agent_id == agent_id {
                    if let Err(error) = runtime.switch_profile_preserving_session(target).await {
                        send_web_system_text(state, route, &format!("❌ {}", error)).await;
                        return;
                    }
                    state.web_channel.set_route_agent(&route.chat_id, agent_id);
                } else {
                    match workspace_threads
                        .create_thread_in_current_workspace_with_host(route, target)
                        .await
                    {
                        Ok(_) => {
                            state.web_channel.set_route_agent(&route.chat_id, agent_id);
                        }
                        Err(error) => {
                            send_web_system_text(state, route, &format!("❌ {}", error)).await;
                        }
                    }
                }
            }
            Ok(None) => match workspace_threads
                .create_thread_in_current_workspace_with_host(route, target)
                .await
            {
                Ok(_) => {
                    state.web_channel.set_route_agent(&route.chat_id, agent_id);
                }
                Err(error) => {
                    send_web_system_text(state, route, &format!("❌ {}", error)).await;
                }
            },
            Err(error) => {
                send_web_system_text(state, route, &format!("❌ {}", error)).await;
            }
        }
    }
}

async fn apply_web_session_resume(
    state: &AppState,
    route: &RouteKey,
    agent: Option<String>,
    profile: Option<String>,
    session_id: String,
    cwd: Option<String>,
) {
    let Some(resume) =
        resolve_web_session_resume(state, route, agent, profile, session_id, cwd).await
    else {
        return;
    };

    state
        .web_channel
        .set_route_agent(&route.chat_id, resume.agent.clone());
    if let Err(error) = state
        .channel_hub
        .workspace_thread_manager()
        .attach_external_session(
            route,
            resume.agent,
            resume.profile,
            resume.session_id,
            std::path::PathBuf::from(resume.cwd),
            ExternalSessionAttachMode::ReuseOpenThread,
        )
        .await
    {
        send_web_system_text(state, route, &format!("❌ {}", error)).await;
    }
}

async fn apply_web_session_resume_now(
    state: &AppState,
    route: &RouteKey,
    agent: Option<String>,
    profile: Option<String>,
    session_id: String,
    cwd: Option<String>,
) {
    let requested_agent = agent
        .as_deref()
        .and_then(|agent| common::resources::resolve_agent_id(agent).ok());
    let requested_agent_invalid = agent.is_some() && requested_agent.is_none();
    let requested_profile = profile.clone();
    let requested_session_id = session_id.clone();
    let Some(resume) =
        resolve_web_session_resume(state, route, agent, profile, session_id, cwd).await
    else {
        if !requested_agent_invalid {
            replay_current_route_session_if_matching(
                state,
                route,
                requested_agent.as_deref(),
                requested_profile.as_deref(),
                &requested_session_id,
            )
            .await;
        }
        return;
    };

    state
        .web_channel
        .set_route_agent(&route.chat_id, resume.agent.clone());
    let requested_session_id = resume.session_id.clone();
    let runtime = match state
        .channel_hub
        .workspace_thread_manager()
        .attach_external_session(
            route,
            resume.agent,
            resume.profile,
            resume.session_id,
            std::path::PathBuf::from(resume.cwd),
            ExternalSessionAttachMode::ReuseOpenThread,
        )
        .await
    {
        Ok(runtime) => runtime,
        Err(error) => {
            send_web_system_text(state, route, &format!("❌ {}", error)).await;
            return;
        }
    };
    let expected_session_id = runtime
        .state()
        .await
        .session_id
        .unwrap_or_else(|| requested_session_id.clone());
    let workspace_threads = state.channel_hub.workspace_thread_manager();
    let started = match common::channels::prompt::start_runtime_and_notify(
        &workspace_threads,
        &runtime,
        &state.channel_hub.plugin_host(),
        route,
        true,
    )
    .await
    {
        Ok(started) => started,
        Err(error) => {
            send_web_system_text(state, route, &format!("❌ {}", error)).await;
            return;
        }
    };
    if !started {
        return;
    }
    let actual_session_id = runtime.state().await.session_id;
    if actual_session_id.as_deref() != Some(expected_session_id.as_str()) {
        let actual = actual_session_id.unwrap_or_else(|| "a new session".to_string());
        send_web_system_text(
            state,
            route,
            &format!(
                "Could not resume session {requested_session_id}; agent started {actual} instead."
            ),
        )
        .await;
    }
}

async fn replay_current_route_session_if_matching(
    state: &AppState,
    route: &RouteKey,
    agent: Option<&str>,
    profile: Option<&str>,
    session_id: &str,
) -> bool {
    let manager = state.channel_hub.workspace_thread_manager();
    let Ok(Some(runtime)) = manager.active_route_runtime(route).await else {
        return false;
    };
    let runtime_state = runtime.state().await;
    let agent_matches = agent
        .map(|agent| runtime_state.host_binding.agent_id == agent)
        .unwrap_or(true);
    let profile_matches = profile
        .map(|profile| runtime_state.host_binding.profile_id.as_deref() == Some(profile))
        .unwrap_or(true);
    if runtime_state.session_id.as_deref() != Some(session_id) || !agent_matches || !profile_matches
    {
        return false;
    }
    drop(runtime_state);

    let workspace_threads = state.channel_hub.workspace_thread_manager();
    match common::channels::prompt::start_runtime_and_notify(
        &workspace_threads,
        &runtime,
        &state.channel_hub.plugin_host(),
        route,
        true,
    )
    .await
    {
        Ok(started) => started,
        Err(error) => {
            send_web_system_text(state, route, &format!("❌ {}", error)).await;
            false
        }
    }
}

struct WebSessionResume {
    agent: String,
    profile: Option<String>,
    session_id: String,
    cwd: String,
}

async fn resolve_web_session_resume(
    state: &AppState,
    route: &RouteKey,
    agent: Option<String>,
    profile: Option<String>,
    session_id: String,
    cwd: Option<String>,
) -> Option<WebSessionResume> {
    let current_state = state
        .channel_hub
        .workspace_thread_manager()
        .resolve_route_runtime(route)
        .await
        .ok()
        .map(|runtime| async move { runtime.state().await });
    let current_state = match current_state {
        Some(state) => Some(state.await),
        None => None,
    };
    let cfg = config::ensure_loaded();
    let agent_prefs = agent_state::read_prefs();
    let agent = agent
        .or_else(|| {
            current_state
                .as_ref()
                .map(|state| state.host_binding.agent_id.clone())
        })
        .unwrap_or_else(|| agent_state::resolve_default_agent(&agent_prefs, &cfg));
    let canonical_agent = match common::resources::resolve_agent_id(&agent) {
        Ok(agent_id) => agent_id,
        Err(error) => {
            send_web_system_text(state, route, &format!("❌ {}", error)).await;
            return None;
        }
    };
    let profile = profile.or_else(|| {
        current_state.as_ref().and_then(|state| {
            (state.host_binding.agent_id == canonical_agent)
                .then(|| state.host_binding.profile_id.clone())
                .flatten()
        })
    });

    if current_state.as_ref().is_some_and(|state| {
        let profile_matches = profile
            .as_deref()
            .map(|profile| state.host_binding.profile_id.as_deref() == Some(profile))
            .unwrap_or(true);
        state.session_id.as_deref() == Some(session_id.as_str())
            && state.host_binding.agent_id == canonical_agent
            && profile_matches
    }) {
        return None;
    }

    let cwd = cwd
        .or_else(|| {
            current_state
                .as_ref()
                .map(|state| state.workspace.to_string_lossy().to_string())
        })
        .unwrap_or_else(|| {
            cfg.resolve_workspace(&canonical_agent)
                .to_string_lossy()
                .to_string()
        });

    Some(WebSessionResume {
        agent: canonical_agent,
        profile,
        session_id,
        cwd,
    })
}

async fn send_web_system_text(state: &AppState, route: &RouteKey, text: &str) {
    state
        .channel_hub
        .send_output(ChannelOutput::SystemText {
            route: route.clone(),
            text: text.to_string(),
            reply_to: None,
        })
        .await;
}

fn canonical_web_session_mode(mode_id: &str) -> Option<&'static str> {
    match mode_id.trim() {
        "default" => Some("default"),
        "plan" => Some("plan"),
        "acceptEdits" | "accept_edits" | "accept-edits" | "accept" => Some("acceptEdits"),
        "bypassPermissions" | "bypass_permissions" | "bypass-permissions" | "bypass" => {
            Some("bypassPermissions")
        }
        "dontAsk" | "dont_ask" | "dont-ask" | "dontask" => Some("dontAsk"),
        _ => None,
    }
}

async fn apply_web_session_mode(state: &AppState, route: &RouteKey, mode_id: &str) {
    let Some(canonical) = canonical_web_session_mode(mode_id) else {
        send_web_system_text(
            state,
            route,
            &format!(
                "❌ Unknown mode `{}`. Valid: default, plan, acceptEdits, bypassPermissions, dontAsk.",
                mode_id
            ),
        )
        .await;
        return;
    };
    send_web_system_text(
        state,
        route,
        &format!(
            "Session mode `{}` is no longer a route-level setting; switch host/profile instead.",
            canonical
        ),
    )
    .await;
}

async fn apply_web_session_config_option(
    state: &AppState,
    route: &RouteKey,
    config_id: String,
    value: String,
) {
    send_web_system_text(
        state,
        route,
        &format!(
            "Session config `{}` is no longer a route-level setting; requested value `{}` was ignored.",
            config_id, value
        ),
    )
    .await;
}

async fn send_event<S>(ws_tx: &mut S, event: &ChatEvent) -> Result<(), ()>
where
    S: SinkExt<Message, Error = axum::Error> + Unpin,
{
    let body = match serde_json::to_string(event) {
        Ok(s) => s,
        Err(e) => {
            tracing::error!(error = %e, "ws_chat serialize failed");
            return Ok(());
        }
    };
    ws_tx.send(Message::Text(body.into())).await.map_err(|_| ())
}

/// Translate a `ChannelOutput` into a wire `ChatEvent`.
fn output_to_chat_event(output: ChannelOutput) -> ChatEvent {
    match output {
        ChannelOutput::ThreadReply { reply, .. } => match reply.payload {
            common::channels::types::ThreadReplyPayload::AcpSessionNotification {
                notification,
            } => acp_passthrough(notification),
        },
        ChannelOutput::RawAcp { payload, .. } => acp_passthrough(payload),
        ChannelOutput::SystemText { text, .. } => ChatEvent::SystemText { text },
        ChannelOutput::AgentReady { agent, version, .. } => {
            ChatEvent::AgentReady { agent, version }
        }
        ChannelOutput::SessionReady { session_id, .. } => ChatEvent::SessionReady { session_id },
        ChannelOutput::SessionInfo { info, .. } => ChatEvent::SystemText {
            text: format!(
                "Workspace: {}\nAgent: {}{}\nProfile: {}\n{}: {}",
                info.workspace_path,
                info.agent.name,
                if info.agent.version.is_empty() {
                    String::new()
                } else {
                    format!(" v{}", info.agent.version)
                },
                info.agent
                    .profile_id
                    .unwrap_or_else(|| "Native".to_string()),
                match info.start {
                    common::channels::types::ChannelSessionStart::New => "New session started",
                    common::channels::types::ChannelSessionStart::Resumed =>
                        "Continuing from session",
                },
                info.session_id
            ),
        },
        ChannelOutput::SessionMode { session_mode, .. } => ChatEvent::SessionMode { session_mode },
        ChannelOutput::CommandMenu {
            system_commands,
            agent_commands,
            ..
        } => ChatEvent::CommandMenu {
            system_commands,
            agent_commands,
        },
        ChannelOutput::PermissionRequest {
            request_id,
            payload,
            ..
        } => ChatEvent::PermissionRequest {
            request_id,
            request: payload,
        },
        ChannelOutput::MultiAgentTurn { turn, agents, .. } => {
            ChatEvent::MultiAgentTurn { turn, agents }
        }
        ChannelOutput::SubagentStatus { agent, .. } => ChatEvent::SubagentStatus { agent },
        ChannelOutput::SubagentAcp { agent, payload, .. } => {
            ChatEvent::SubagentAcpNotification { agent, payload }
        }
        ChannelOutput::PromptDone { message_id, .. } => ChatEvent::PromptDone { message_id },
        ChannelOutput::TurnStatus { active, .. } => ChatEvent::TurnStatus { active },
    }
}

/// Pass ACP session notifications through as `AcpNotification`.
fn acp_passthrough(payload: serde_json::Value) -> ChatEvent {
    ChatEvent::AcpNotification { payload }
}

fn permission_response_error_event(request_id: &str, error: &str) -> ChatEvent {
    ChatEvent::Error {
        error: format!("Permission response for request `{request_id}` was ignored: {error}"),
    }
}

#[cfg(test)]
mod tests {
    use agent_client_protocol::schema::v1 as acp;

    use super::*;

    fn web_client() -> ChatSocketClient {
        ChatSocketClient::from_query(None).expect("web client")
    }

    fn tui_client() -> ChatSocketClient {
        ChatSocketClient::from_query(Some("tui")).expect("tui client")
    }

    fn parse_web_chat_input(chat_id: &str, text: &str) -> Option<WebChatInput> {
        let client = web_client();
        let route = RouteKey::new(client.channel_kind, chat_id);
        super::parse_web_chat_input(&route, client.sender_id, text)
    }

    #[test]
    fn chat_socket_client_defaults_to_web_and_accepts_tui() {
        assert_eq!(web_client().channel_kind, "web");
        assert_eq!(web_client().sender_id, "web-user");
        assert_eq!(tui_client().channel_kind, "tui");
        assert_eq!(tui_client().sender_id, "tui-user");
        assert!(ChatSocketClient::from_query(Some("unknown")).is_err());
    }

    #[test]
    fn chat_id_query_accepts_only_safe_stable_ids() {
        assert_eq!(
            super::sanitize_chat_id(Some("web_abc-123")).as_deref(),
            Some("web_abc-123")
        );
        assert_eq!(
            super::sanitize_chat_id(Some(" web_abc ")).as_deref(),
            Some("web_abc")
        );
        assert!(super::sanitize_chat_id(Some("web:abc")).is_none());
        assert!(super::sanitize_chat_id(Some("../secret")).is_none());
        assert!(super::sanitize_chat_id(Some(&"a".repeat(129))).is_none());
    }

    #[test]
    fn explicit_chat_id_replays_initial_route_history() {
        assert!(!super::should_replay_initial_route_history(&None));
        assert!(super::should_replay_initial_route_history(&Some(
            "ws_thread".to_string()
        )));
    }

    #[test]
    fn permission_response_error_is_user_visible() {
        let ChatEvent::Error { error } = super::permission_response_error_event(
            "req-1",
            "permission request is no longer pending",
        ) else {
            panic!("expected error event");
        };

        assert!(error.contains("req-1"));
        assert!(error.contains("permission request is no longer pending"));
    }

    #[test]
    fn parses_tui_message_with_tui_route_identity() {
        let input = super::parse_web_chat_input(
            &RouteKey::new("tui", "chat-1"),
            tui_client().sender_id,
            r#"{"type":"message","text":"hello"}"#,
        )
        .expect("message input");

        let WebChatInput::Message {
            input:
                ChannelInput::Message {
                    envelope:
                        ChannelEnvelope {
                            route, sender_id, ..
                        },
                },
            ..
        } = input
        else {
            panic!("expected tui message");
        };

        assert_eq!(route, RouteKey::new("tui", "chat-1"));
        assert_eq!(sender_id, "tui-user");
    }

    #[test]
    fn parses_selected_permission_response() {
        let input = parse_web_chat_input(
            "chat-1",
            r#"{"type":"permission_response","requestId":"req-1","optionId":"allow-once"}"#,
        )
        .expect("permission response");

        let WebChatInput::PermissionResponse {
            request_id,
            response,
        } = input
        else {
            panic!("expected permission response");
        };

        assert_eq!(request_id, "req-1");
        match response.outcome {
            acp::RequestPermissionOutcome::Selected(selected) => {
                assert_eq!(selected.option_id.to_string(), "allow-once");
            }
            acp::RequestPermissionOutcome::Cancelled => panic!("expected selected outcome"),
            _ => panic!("expected selected outcome"),
        }
    }

    #[test]
    fn parses_cancelled_permission_response() {
        let input = parse_web_chat_input(
            "chat-1",
            r#"{"type":"permission_response","requestId":"req-2","outcome":"cancelled"}"#,
        )
        .expect("permission response");

        let WebChatInput::PermissionResponse {
            request_id,
            response,
        } = input
        else {
            panic!("expected permission response");
        };

        assert_eq!(request_id, "req-2");
        assert!(matches!(
            response.outcome,
            acp::RequestPermissionOutcome::Cancelled
        ));
    }

    #[test]
    fn parses_stop_message() {
        let input = parse_web_chat_input("chat-1", r#"{"type":"stop"}"#).expect("stop input");

        let WebChatInput::Stop(ChannelInput::Stop { route }) = input else {
            panic!("expected stop input");
        };

        assert_eq!(route, RouteKey::new("web", "chat-1"));
    }

    #[test]
    fn parses_resume_session_intent() {
        let input = parse_web_chat_input(
            "chat-1",
            r#"{"type":"message","text":"continue","agent":"codex","sessionAction":"resume","sessionId":"sid-1","sessionWorkspace":"/tmp/project"}"#,
        )
        .expect("message input");

        let WebChatInput::Message {
            input:
                ChannelInput::Message {
                    envelope:
                        ChannelEnvelope {
                            cli_kind: Some(agent),
                            ..
                        },
                },
            profile: None,
            session_intent:
                Some(WebChatSessionIntent::Resume {
                    agent: Some(intent_agent),
                    session_id,
                    cwd: Some(cwd),
                }),
            session_mode: None,
        } = input
        else {
            panic!("expected resume message");
        };

        assert_eq!(agent, "codex");
        assert_eq!(intent_agent, "codex");
        assert_eq!(session_id, "sid-1");
        assert_eq!(cwd, "/tmp/project");
    }

    #[test]
    fn parses_direct_resume_session() {
        let input = parse_web_chat_input(
            "chat-1",
            r#"{"type":"resume_session","agent":"codex","profileId":"deepseek","sessionId":"sid-1","sessionWorkspace":"/tmp/project"}"#,
        )
        .expect("resume session input");

        let WebChatInput::ResumeSession {
            agent: Some(agent),
            profile: Some(profile),
            session_id,
            cwd: Some(cwd),
        } = input
        else {
            panic!("expected direct resume input");
        };

        assert_eq!(agent, "codex");
        assert_eq!(profile, "deepseek");
        assert_eq!(session_id, "sid-1");
        assert_eq!(cwd, "/tmp/project");
    }

    #[test]
    fn parses_new_session_intent() {
        let input = parse_web_chat_input(
            "chat-1",
            r#"{"type":"message","text":"start over","sessionAction":"new"}"#,
        )
        .expect("message input");

        let WebChatInput::Message {
            session_intent: Some(WebChatSessionIntent::New { cwd: None }),
            session_mode: None,
            ..
        } = input
        else {
            panic!("expected new-session message");
        };
    }

    #[test]
    fn parses_new_session_workspace() {
        let input = parse_web_chat_input(
            "chat-1",
            r#"{"type":"message","text":"start here","sessionAction":"new","sessionWorkspace":"/tmp/new-project"}"#,
        )
        .expect("message input");

        let WebChatInput::Message {
            session_intent: Some(WebChatSessionIntent::New { cwd: Some(cwd) }),
            session_mode: None,
            ..
        } = input
        else {
            panic!("expected new-session message with workspace");
        };

        assert_eq!(cwd, "/tmp/new-project");
    }

    #[test]
    fn parses_profile_selection() {
        let input = parse_web_chat_input(
            "chat-1",
            r#"{"type":"message","text":"hello","agent":"claude","profileId":"deepseek"}"#,
        )
        .expect("message input");

        let WebChatInput::Message {
            profile: Some(profile),
            session_mode: None,
            ..
        } = input
        else {
            panic!("expected profile message");
        };

        assert_eq!(profile, "deepseek");
    }

    #[test]
    fn parses_message_permission_mode() {
        let input = parse_web_chat_input(
            "chat-1",
            r#"{"type":"message","text":"hello","permissionMode":"acceptEdits"}"#,
        )
        .expect("message input");

        let WebChatInput::Message {
            session_mode: Some(mode_id),
            ..
        } = input
        else {
            panic!("expected message mode");
        };

        assert_eq!(mode_id, "acceptEdits");
    }

    #[test]
    fn parses_set_mode_message() {
        let input = parse_web_chat_input(
            "chat-1",
            r#"{"type":"set_mode","modeId":"bypassPermissions"}"#,
        )
        .expect("set mode input");

        let WebChatInput::SetMode { mode_id } = input else {
            panic!("expected set mode");
        };

        assert_eq!(mode_id, "bypassPermissions");
        assert_eq!(
            canonical_web_session_mode("bypass-permissions"),
            Some("bypassPermissions"),
        );
    }

    #[test]
    fn parses_set_config_option_message() {
        let input = parse_web_chat_input(
            "chat-1",
            r#"{"type":"set_config_option","configId":"permissions","value":"fullAccess"}"#,
        )
        .expect("set config option input");

        let WebChatInput::SetConfigOption { config_id, value } = input else {
            panic!("expected set config option");
        };

        assert_eq!(config_id, "permissions");
        assert_eq!(value, "fullAccess");
    }

    #[test]
    fn parses_message_attachments() {
        let input = parse_web_chat_input(
            "chat-1",
            r#"{"type":"message","messageId":"msg-1","agent":"codex","attachments":[{"uri":"file:///tmp/report.md","name":"report.md","mimeType":"text/markdown","size":42}]}"#,
        )
        .expect("message input");

        let WebChatInput::Message {
            input:
                ChannelInput::Message {
                    envelope:
                        ChannelEnvelope {
                            message_id,
                            attachments,
                            ..
                        },
                },
            session_mode: None,
            ..
        } = input
        else {
            panic!("expected attachment message");
        };

        assert_eq!(message_id, "msg-1");
        assert_eq!(attachments.len(), 1);
        assert_eq!(attachments[0].message_id, "msg-1");
        assert_eq!(attachments[0].file_key, "file:///tmp/report.md");
        assert_eq!(attachments[0].file_name, "report.md");
        assert_eq!(attachments[0].resource_type, "text/markdown");
        assert_eq!(attachments[0].size, Some(42));
    }

    #[test]
    fn dedupes_message_attachments() {
        let input = parse_web_chat_input(
            "chat-1",
            r#"{"type":"message","messageId":"msg-1","attachments":[{"uri":"file:///tmp/logo.png","name":"Logo.png","mimeType":"image/png","size":42},{"uri":"file:///tmp/logo.png","name":"Logo.png","mimeType":"image/png","size":42}]}"#,
        )
        .expect("message input");

        let WebChatInput::Message {
            input:
                ChannelInput::Message {
                    envelope: ChannelEnvelope { attachments, .. },
                },
            ..
        } = input
        else {
            panic!("expected attachment message");
        };

        assert_eq!(attachments.len(), 1);
        assert_eq!(attachments[0].file_key, "file:///tmp/logo.png");
    }

    #[test]
    fn rejects_unsafe_relative_attachment_keys() {
        let input = parse_web_chat_input(
            "chat-1",
            r#"{"type":"message","messageId":"msg-1","text":"see file","attachments":[{"fileKey":"../secret","name":"secret.txt"}]}"#,
        )
        .expect("message input");

        let WebChatInput::Message {
            input:
                ChannelInput::Message {
                    envelope: ChannelEnvelope { attachments, .. },
                },
            ..
        } = input
        else {
            panic!("expected attachment message");
        };

        assert!(attachments.is_empty());
    }
}
