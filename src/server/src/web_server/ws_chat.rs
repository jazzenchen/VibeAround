//! WebSocket handler for web chat channel.
//!
//! - GET /va/ws/chat — ACP-native websocket adapter
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
use common::workspace::threads::runtime::StartupReplay;
use common::workspace::threads::HostBinding;
use common::{agent_state, config};

use crate::api_types::{AgentInfo, ChatEvent};

use super::AppState;

mod bound;
mod event;
mod input;

pub(super) use bound::{parse_bound_chat_input, respond_to_web_permission, BoundChatInput};
pub(super) use event::{output_to_chat_event, permission_response_error_event};
use input::{parse_web_chat_input, WebChatInput, WebChatSessionIntent};

/// WebSocket upgrade handler for web chat.
pub async fn ws_chat_handler(
    State(state): State<AppState>,
    Query(query): Query<WsChatQuery>,
    headers: HeaderMap,
    ws: WebSocketUpgrade,
) -> Response {
    let tunnel_urls = state.tunnels.public_urls();
    if !super::auth::headers_have_allowed_dashboard_origin(&headers, state.port, &tunnel_urls) {
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

fn initial_route(client: ChatSocketClient, chat_id: Option<String>) -> RouteKey {
    RouteKey::new(
        client.channel_kind,
        chat_id.unwrap_or_else(|| Uuid::new_v4().to_string()),
    )
}

fn agent_ids_with_bound_host(
    enabled_agent_ids: &[String],
    bound_agent_id: Option<&str>,
) -> Vec<String> {
    let mut agent_ids = enabled_agent_ids.to_vec();
    if let Some(agent_id) = bound_agent_id {
        if !agent_ids
            .iter()
            .any(|enabled_agent_id| enabled_agent_id == agent_id)
        {
            agent_ids.push(agent_id.to_string());
        }
    }
    agent_ids
}

async fn read_config_and_prefs_snapshot() -> Option<(config::Config, agent_state::AgentsPrefsFile)>
{
    match tokio::task::spawn_blocking(agent_state::read_config_and_prefs).await {
        Ok(snapshot) => Some(snapshot),
        Err(error) => {
            tracing::error!(%error, "web chat settings snapshot task failed");
            None
        }
    }
}

async fn handle_chat_socket(
    socket: WebSocket,
    state: AppState,
    client: ChatSocketClient,
    chat_id: Option<String>,
) {
    let connection_id = Uuid::new_v4().to_string();
    let mut active_route = initial_route(client, chat_id);
    let chat_id = active_route.chat_id.clone();
    let channel_id = format!("{}:{}", client.channel_kind, chat_id);

    // Connections receive live output only. History is the client's own
    // cache or an explicit replay request — never an automatic dump.
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<ChannelOutput>();
    state
        .web_channel
        .register_connection(&active_route, connection_id.clone(), tx.clone())
        .await;
    let (event_tx, mut event_rx) = tokio::sync::mpsc::unbounded_channel::<ChatEvent>();

    let (mut ws_tx, mut ws_rx) = socket.split();

    // Load one settings snapshot for initial agent metadata.
    let Some((cfg, agent_prefs)) = read_config_and_prefs_snapshot().await else {
        state
            .web_channel
            .unregister_connection(&active_route, &connection_id)
            .await;
        return;
    };

    let bound_agent_id = match state
        .channel_hub
        .workspace_thread_manager()
        .active_route_runtime(&active_route)
        .await
    {
        Ok(Some(runtime)) => Some(runtime.state().await.host_binding.agent_id),
        Ok(None) => None,
        Err(error) => {
            tracing::warn!(%error, route = %active_route, "failed to resolve bound chat agent");
            None
        }
    };
    let agent_ids = agent_ids_with_bound_host(&cfg.enabled_agents, bound_agent_id.as_deref());

    // Send initial config event.
    let config_event = ChatEvent::Config {
        channel_id: channel_id.clone(),
        agents: AgentInfo::for_ids(&agent_ids),
        default_agent: agent_state::resolve_default_agent(&agent_prefs, &cfg),
    };
    if send_event(&mut ws_tx, &config_event).await.is_err() {
        state
            .web_channel
            .unregister_connection(&active_route, &connection_id)
            .await;
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
                                remember_web_route_agent(&state, &route, input_agent(&input)).await;
                                let wait_for_session_ready = should_wait_for_user_message_session(
                                    &state,
                                    &route,
                                    &session_intent,
                                )
                                .await;
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
                                                );
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
                                )
                                .await;
                            }
                            if dispatch_input {
                                enqueue_channel_input(&state.channel_hub, input);
                            }
                        }
                        WebChatInput::SetMode { mode_id } => {
                            apply_web_session_mode(&state, &active_route, &mode_id).await;
                        }
                        WebChatInput::SetConfigOption { config_id, value } => {
                            apply_web_session_config_option(
                                &state,
                                &active_route,
                                config_id,
                                value,
                            )
                            .await;
                        }
                        WebChatInput::Cancel(input) => {
                            abort_direct_resume_task(
                                &mut direct_resume_task,
                                &state,
                                &active_route,
                            )
                            .await;
                            // Message and Stop share this upstream FIFO before
                            // ingress. Sending Stop directly to ingress could
                            // overtake a message still waiting in input_rx and
                            // cancel an empty route before that message runs.
                            enqueue_channel_input(&state.channel_hub, input);
                        }
                        WebChatInput::PermissionResponse {
                            request_id,
                            response,
                        } => {
                            let result = respond_to_web_permission(
                                &state,
                                &active_route,
                                &request_id,
                                response,
                            )
                            .await;
                            if let Err(error) = result {
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
                            replay,
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
                                if let Some(route) = state
                                    .web_channel
                                    .route_for_session(&agent_id, &session_id)
                                    .await
                                {
                                    state
                                        .web_channel
                                        .unregister_connection(&active_route, &connection_id)
                                        .await;
                                    active_route = route;
                                    state
                                        .web_channel
                                        .register_connection(
                                            &active_route,
                                            connection_id.clone(),
                                            tx.clone(),
                                        )
                                        .await;
                                    if replay {
                                        // The transcript comes from the agent
                                        // itself, bracketed and addressed to
                                        // this connection only.
                                        replay_route_session_to_sink(
                                            &state,
                                            &active_route,
                                            tx.clone(),
                                        )
                                        .await;
                                    } else {
                                        let _ = tx.send(ChannelOutput::SessionReady {
                                            route: active_route.clone(),
                                            reply_to: None,
                                            session_id,
                                        });
                                    }
                                    continue;
                                }
                            }
                            let task_state = state.clone();
                            let task_route = active_route.clone();
                            let task_sink = tx.clone();
                            direct_resume_task = Some(tokio::spawn(async move {
                                apply_web_session_resume_now(
                                    &task_state,
                                    &task_route,
                                    agent,
                                    profile,
                                    session_id,
                                    cwd,
                                    replay,
                                    task_sink,
                                )
                                .await;
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
        .unregister_connection(&active_route, &connection_id)
        .await;
    if !state.web_channel.route_has_session(&active_route).await
        && !state.web_channel.route_is_active(&active_route).await
    {
        let _ = state
            .channel_hub
            .workspace_thread_manager()
            .detach_route(&active_route)
            .await;
        state.web_channel.forget_route(&active_route).await;
        return;
    }
    shutdown_host_for_departed_client(&state, &active_route).await;
}

/// Closing the window is a departure, so the ACP host it was talking to should
/// not be left running for nobody. The thread, its session and its routes all
/// survive; the next message starts the host again and resumes.
async fn shutdown_host_for_departed_client(state: &AppState, route: &RouteKey) {
    if state.web_channel.route_has_connections(route).await {
        return;
    }
    let manager = state.channel_hub.workspace_thread_manager();
    let Ok(Some(runtime)) = manager.active_route_runtime(route).await else {
        return;
    };
    let runtime_state = runtime.state().await;
    // A turn in flight may still be feeding other subscribers. Let it finish and
    // leave the host to the warm pool.
    if runtime_state.busy {
        return;
    }
    let thread_id = runtime_state.thread_id.clone();
    drop(runtime_state);

    let Ok(attached) = manager.attached_routes_for_thread(&thread_id).await else {
        return;
    };
    for other in attached.iter().filter(|other| *other != route) {
        if is_listening(state, other).await {
            return;
        }
    }
    let _ = manager.shutdown_thread_host(&thread_id).await;
}

async fn is_listening(state: &AppState, route: &RouteKey) -> bool {
    let has_connections = state.web_channel.route_has_connections(route).await;
    route_still_listening(route, has_connections)
}

/// A surface whose presence is a connection stops listening the moment its last
/// one closes. An IM plugin listens whether or not anyone has a window open.
fn route_still_listening(route: &RouteKey, has_connections: bool) -> bool {
    !common::routing::channel_traits(&route.channel_kind).presence_is_a_connection
        || has_connections
}

fn enqueue_channel_input(channel_hub: &common::channels::ChannelManager, input: ChannelInput) {
    channel_hub.handle_input(input);
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
    state.web_channel.forget_route(route).await;
}

fn input_route(input: &ChannelInput) -> Option<RouteKey> {
    match input {
        ChannelInput::Message { envelope }
        | ChannelInput::Callback {
            envelope,
            action_value: _,
        } => Some(envelope.route.clone()),
        ChannelInput::Cancel { route } => Some(route.clone()),
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

pub(super) async fn remember_web_route_agent(
    state: &AppState,
    route: &RouteKey,
    agent: Option<String>,
) {
    if let Some(agent_id) = resolve_web_session_agent(state, route, agent).await {
        state.web_channel.set_route_agent(route, agent_id).await;
    }
}

async fn should_wait_for_user_message_session(
    state: &AppState,
    route: &RouteKey,
    session_intent: &Option<WebChatSessionIntent>,
) -> bool {
    match session_intent {
        Some(WebChatSessionIntent::New { .. }) => true,
        Some(WebChatSessionIntent::Resume { session_id, .. }) => {
            state.web_channel.route_session_id(route).await.as_deref() != Some(session_id.as_str())
        }
        None => false,
    }
}

pub(super) async fn remember_web_user_message(
    state: &AppState,
    route: &RouteKey,
    input: &ChannelInput,
    wait_for_session_ready: bool,
) {
    let ChannelInput::Message { envelope } = input else {
        return;
    };
    let content = web_user_message_content(envelope);
    state
        .web_channel
        .record_user_message(
            route,
            envelope.message_id.clone(),
            content,
            wait_for_session_ready,
        )
        .await;
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
    let (cfg, agent_prefs) = read_config_and_prefs_snapshot().await?;
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
            send_web_system_text(state, route, &format!("❌ {}", error));
        }
    }
    if let Some(profile) = profile {
        let Ok(agent_id) = common::resources::resolve_agent_id(&agent) else {
            send_web_system_text(state, route, &format!("❌ Unknown agent `{}`.", agent));
            return;
        };
        let target = HostBinding::new(agent_id.clone(), Some(profile));
        let workspace_threads = state.channel_hub.workspace_thread_manager();
        match workspace_threads.active_route_runtime(route).await {
            Ok(Some(runtime)) => {
                if runtime.state().await.host_binding.agent_id == agent_id {
                    if let Err(error) = runtime.switch_profile_preserving_session(target).await {
                        send_web_system_text(state, route, &format!("❌ {}", error));
                        return;
                    }
                    state.web_channel.set_route_agent(route, agent_id).await;
                } else {
                    match workspace_threads
                        .create_thread_in_current_workspace_with_host(route, target)
                        .await
                    {
                        Ok(_) => {
                            state.web_channel.set_route_agent(route, agent_id).await;
                        }
                        Err(error) => {
                            send_web_system_text(state, route, &format!("❌ {}", error));
                        }
                    }
                }
            }
            Ok(None) => match workspace_threads
                .create_thread_in_current_workspace_with_host(route, target)
                .await
            {
                Ok(_) => {
                    state.web_channel.set_route_agent(route, agent_id).await;
                }
                Err(error) => {
                    send_web_system_text(state, route, &format!("❌ {}", error));
                }
            },
            Err(error) => {
                send_web_system_text(state, route, &format!("❌ {}", error));
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
        .set_route_agent(route, resume.agent.clone())
        .await;
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
        send_web_system_text(state, route, &format!("❌ {}", error));
    }
}

#[allow(clippy::too_many_arguments)]
async fn apply_web_session_resume_now(
    state: &AppState,
    route: &RouteKey,
    agent: Option<String>,
    profile: Option<String>,
    session_id: String,
    cwd: Option<String>,
    replay: bool,
    replay_sink: tokio::sync::mpsc::UnboundedSender<ChannelOutput>,
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
                replay,
                replay_sink,
            )
            .await;
        }
        return;
    };

    state
        .web_channel
        .set_route_agent(route, resume.agent.clone())
        .await;
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
            send_web_system_text(state, route, &format!("❌ {}", error));
            return;
        }
    };
    let expected_session_id = runtime
        .state()
        .await
        .session_id
        .unwrap_or_else(|| requested_session_id.clone());
    let workspace_threads = state.channel_hub.workspace_thread_manager();
    let target = common::routing::ChannelTarget::for_route(route.clone());
    let (replay_mode, replay_sink) = web_replay_request(replay, replay_sink);
    let started = match common::channels::prompt::start_runtime_and_notify(
        &workspace_threads,
        &runtime,
        &state.channel_hub.plugin_host(),
        &target,
        true,
        replay_mode,
        replay_sink,
    )
    .await
    {
        Ok(started) => started,
        Err(error) => {
            send_web_system_text(state, route, &format!("❌ {}", error));
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
        );
    }
}

async fn replay_current_route_session_if_matching(
    state: &AppState,
    route: &RouteKey,
    agent: Option<&str>,
    profile: Option<&str>,
    session_id: &str,
    replay: bool,
    replay_sink: tokio::sync::mpsc::UnboundedSender<ChannelOutput>,
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
    let target = common::routing::ChannelTarget::for_route(route.clone());
    let (replay_mode, replay_sink) = web_replay_request(replay, replay_sink);
    match common::channels::prompt::start_runtime_and_notify(
        &workspace_threads,
        &runtime,
        &state.channel_hub.plugin_host(),
        &target,
        true,
        replay_mode,
        replay_sink,
    )
    .await
    {
        Ok(started) => started,
        Err(error) => {
            send_web_system_text(state, route, &format!("❌ {}", error));
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
    let (cfg, agent_prefs) = read_config_and_prefs_snapshot().await?;
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
            send_web_system_text(state, route, &format!("❌ {}", error));
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

type WebReplaySink = tokio::sync::mpsc::UnboundedSender<ChannelOutput>;

/// Translate the client's replay wish into the start-notify arguments: a
/// silent resume carries no sink, a replay is addressed to the requester.
fn web_replay_request(replay: bool, sink: WebReplaySink) -> (StartupReplay, Option<WebReplaySink>) {
    if replay {
        (StartupReplay::Replay, Some(sink))
    } else {
        (StartupReplay::Silent, None)
    }
}

/// Start (or restart) the route's bound session and stream its transcript,
/// bracketed, to one connection.
async fn replay_route_session_to_sink(state: &AppState, route: &RouteKey, sink: WebReplaySink) {
    let workspace_threads = state.channel_hub.workspace_thread_manager();
    let Ok(runtime) = workspace_threads.resolve_route_runtime(route).await else {
        return;
    };
    let target = common::routing::ChannelTarget::for_route(route.clone());
    if let Err(error) = common::channels::prompt::start_runtime_and_notify(
        &workspace_threads,
        &runtime,
        &state.channel_hub.plugin_host(),
        &target,
        true,
        StartupReplay::Replay,
        Some(sink),
    )
    .await
    {
        send_web_system_text(state, route, &format!("❌ {}", error));
    }
}

fn send_web_system_text(state: &AppState, route: &RouteKey, text: &str) {
    state.channel_hub.send_output(ChannelOutput::SystemText {
        route: route.clone(),
        text: text.to_string(),
        reply_to: None,
    });
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
        ;
        return;
    };
    send_web_system_text(
        state,
        route,
        &format!(
            "Session mode `{}` is no longer a route-level setting; switch host/profile instead.",
            canonical
        ),
    );
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
    ;
}

pub(super) async fn send_event<S>(ws_tx: &mut S, event: &ChatEvent) -> Result<(), ()>
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

#[cfg(test)]
mod tests {
    use super::*;
    use common::channels::ChannelManager;
    use common::workspace::WorkspaceThreadManager;

    fn web_client() -> ChatSocketClient {
        ChatSocketClient::from_query(None).expect("web client")
    }

    fn tui_client() -> ChatSocketClient {
        ChatSocketClient::from_query(Some("tui")).expect("tui client")
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
    fn bound_agent_is_included_without_changing_enabled_order() {
        let enabled = vec!["codex".to_string(), "claude".to_string()];

        assert_eq!(
            super::agent_ids_with_bound_host(&enabled, Some("va-agent")),
            vec!["codex", "claude", "va-agent"]
        );
        assert_eq!(
            super::agent_ids_with_bound_host(&enabled, Some("codex")),
            enabled
        );
    }

    #[test]
    fn connections_without_chat_id_receive_distinct_routes() {
        let first = super::initial_route(web_client(), None);
        let second = super::initial_route(web_client(), None);

        assert_ne!(first, second);
        assert_eq!(first.channel_kind, "web");
        assert_eq!(second.channel_kind, "web");
    }

    #[test]
    fn only_connection_backed_surfaces_stop_listening_when_they_close() {
        let web = RouteKey::new("web", "ws_wt_1");
        let tui = RouteKey::new("tui", "ws_wt_1");
        let feishu = RouteKey::new("feishu", "oc_abc");

        assert!(super::route_still_listening(&web, true));
        assert!(!super::route_still_listening(&web, false));
        assert!(super::route_still_listening(&tui, true));
        assert!(!super::route_still_listening(&tui, false));
        // Nobody has a Feishu window open; the plugin is listening regardless.
        assert!(super::route_still_listening(&feishu, false));
    }

    #[test]
    fn canonical_web_session_mode_accepts_dash_alias() {
        assert_eq!(
            canonical_web_session_mode("bypass-permissions"),
            Some("bypassPermissions"),
        );
    }

    #[tokio::test]
    async fn message_and_stop_share_the_same_upstream_fifo() {
        let base = std::env::temp_dir().join(format!(
            "vibearound-ws-input-order-{}",
            uuid::Uuid::new_v4()
        ));
        let workspace_threads = WorkspaceThreadManager::with_paths(
            base.join("workspaces.jsonl"),
            base.join("threads.jsonl"),
            base.join("attachments.jsonl"),
        );
        let (channel_hub, mut input_rx) = ChannelManager::new(workspace_threads);
        let route = RouteKey::new("web", "ordered-stop");

        enqueue_channel_input(
            &channel_hub,
            ChannelInput::Message {
                envelope: ChannelEnvelope {
                    route: route.clone(),
                    message_id: "ordered-message".to_string(),
                    turn_id: None,
                    text: "hello".to_string(),
                    sender_id: "web-user".to_string(),
                    attachments: Vec::new(),
                    parent_id: None,
                    cli_kind: None,
                },
            },
        );
        enqueue_channel_input(&channel_hub, ChannelInput::Cancel { route });

        assert!(matches!(
            input_rx.try_recv(),
            Ok(ChannelInput::Message { .. })
        ));
        assert!(matches!(
            input_rx.try_recv(),
            Ok(ChannelInput::Cancel { .. })
        ));
    }
}
