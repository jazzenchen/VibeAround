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
    State,
};
use axum::response::Response;
use futures_util::{SinkExt, StreamExt};
use tokio::task::JoinHandle;
use uuid::Uuid;

use agent_client_protocol::schema as acp;
use common::channels::{ChannelEnvelope, ChannelInput, ChannelOutput};
use common::routing::{Attachment, RouteKey};
use common::workspace::threads::HostBinding;
use common::{agent_state, config};

use crate::api_types::{AgentInfo, ChatEvent};

use super::AppState;

/// WebSocket upgrade handler for web chat.
pub async fn ws_chat_handler(State(state): State<AppState>, ws: WebSocketUpgrade) -> Response {
    ws.on_upgrade(move |socket| handle_chat_socket(socket, state))
}

async fn handle_chat_socket(socket: WebSocket, state: AppState) {
    let connection_id = Uuid::new_v4().to_string();
    let chat_id = Uuid::new_v4().to_string();
    let channel_id = format!("web:{}", chat_id);
    let mut active_route = RouteKey::new("web", &chat_id);

    // Register this connection for outbound ACP events
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<ChannelOutput>();
    state.web_channel.register_connection(
        active_route.chat_id.clone(),
        connection_id.clone(),
        tx.clone(),
        false,
    );

    let (mut ws_tx, mut ws_rx) = socket.split();

    // Load config for initial agent metadata. Web chat always receives the
    // complete ACP transcript; the browser applies its own visibility filter
    // so replay cache stays independent from the current UI settings.
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
        while let Some(output) = rx.recv().await {
            let event = output_to_chat_event(output);
            if send_event(&mut ws_tx, &event).await.is_err() {
                break;
            }
        }
    });

    // Inbound: ws messages → channel-input thread / permission bridge.
    let mut direct_resume_task: Option<JoinHandle<()>> = None;
    while let Some(Ok(msg)) = ws_rx.next().await {
        match msg {
            Message::Text(text) => {
                if let Some(input) = parse_web_chat_input(&active_route.chat_id, &text) {
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
                            state.channel_hub.handle_input(input);
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
                            if let Err(error) =
                                state
                                    .channel_hub
                                    .respond_permission("web", &request_id, response)
                            {
                                tracing::warn!(
                                    request_id = %request_id,
                                    error = %error,
                                    "web permission response ignored"
                                );
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
                                if let Some(route_chat_id) =
                                    state.web_channel.route_for_session(&agent_id, &session_id)
                                {
                                    state.web_channel.unregister_connection(
                                        &active_route.chat_id,
                                        &connection_id,
                                    );
                                    active_route = RouteKey::new("web", &route_chat_id);
                                    state.web_channel.register_connection(
                                        active_route.chat_id.clone(),
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
            .map(web_attachment_content_block),
    );
    blocks
}

fn web_attachment_content_block(attachment: &Attachment) -> serde_json::Value {
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
    block.insert(
        "uri".to_string(),
        serde_json::Value::String(web_attachment_uri(&attachment.file_key)),
    );
    if !attachment.resource_type.trim().is_empty() {
        block.insert(
            "mimeType".to_string(),
            serde_json::Value::String(attachment.resource_type.clone()),
        );
    }
    if let Some(size) = attachment.size {
        block.insert("size".to_string(), serde_json::Value::Number(size.into()));
    }
    serde_json::Value::Object(block)
}

fn web_attachment_uri(file_key: &str) -> String {
    if file_key.starts_with("file://")
        || file_key.starts_with("http://")
        || file_key.starts_with("https://")
    {
        return file_key.to_string();
    }
    format!(
        "file://{}",
        config::data_dir()
            .join(".cache")
            .join(file_key)
            .to_string_lossy()
    )
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
        match state
            .channel_hub
            .workspace_thread_manager()
            .resolve_route_runtime(route)
            .await
        {
            Ok(runtime) => {
                if let Err(error) = runtime
                    .switch_host(HostBinding::new(agent_id.clone(), Some(profile)), false)
                    .await
                {
                    send_web_system_text(state, route, &format!("❌ {}", error)).await;
                } else {
                    state.web_channel.set_route_agent(&route.chat_id, agent_id);
                }
            }
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
    let Some(resume) =
        resolve_web_session_resume(state, route, agent, profile, session_id, cwd).await
    else {
        return;
    };

    state
        .web_channel
        .set_route_agent(&route.chat_id, resume.agent.clone());
    let runtime = match state
        .channel_hub
        .workspace_thread_manager()
        .attach_external_session(
            route,
            resume.agent,
            resume.profile,
            resume.session_id,
            std::path::PathBuf::from(resume.cwd),
        )
        .await
    {
        Ok(runtime) => runtime,
        Err(error) => {
            send_web_system_text(state, route, &format!("❌ {}", error)).await;
            return;
        }
    };
    let workspace_threads = state.channel_hub.workspace_thread_manager();
    if let Err(error) = common::channels::prompt::start_runtime_and_notify(
        &workspace_threads,
        &runtime,
        &state.channel_hub.plugin_host(),
        route,
        true,
    )
    .await
    {
        send_web_system_text(state, route, &format!("❌ {}", error)).await;
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

enum WebChatInput {
    Message {
        input: ChannelInput,
        profile: Option<String>,
        session_intent: Option<WebChatSessionIntent>,
        session_mode: Option<String>,
    },
    SetMode {
        mode_id: String,
    },
    SetConfigOption {
        config_id: String,
        value: String,
    },
    Stop(ChannelInput),
    PermissionResponse {
        request_id: String,
        response: acp::RequestPermissionResponse,
    },
    ResumeSession {
        agent: Option<String>,
        profile: Option<String>,
        session_id: String,
        cwd: Option<String>,
    },
}

enum WebChatSessionIntent {
    Resume {
        agent: Option<String>,
        session_id: String,
        cwd: Option<String>,
    },
    New {
        cwd: Option<String>,
    },
}

fn parse_web_chat_input(chat_id: &str, text: &str) -> Option<WebChatInput> {
    let parsed = serde_json::from_str::<serde_json::Value>(text);

    match parsed {
        Ok(v) => {
            let ty = v.get("type").and_then(|x| x.as_str()).unwrap_or("");
            match ty {
                "message" => {
                    let text = v.get("text").and_then(|x| x.as_str()).unwrap_or("").trim();
                    let message_id = v
                        .get("messageId")
                        .and_then(|x| x.as_str())
                        .map(ToOwned::to_owned)
                        .unwrap_or_else(|| Uuid::new_v4().to_string());
                    let attachments = parse_web_attachments(&v, &message_id);
                    if text.is_empty() && attachments.is_empty() {
                        return None;
                    }
                    let agent = parse_web_agent(&v);
                    let session_intent = parse_web_session_intent(&v, agent.clone());
                    let profile = parse_web_profile(&v);
                    let session_mode = parse_web_session_mode(&v);
                    Some(WebChatInput::Message {
                        input: ChannelInput::Message {
                            envelope: ChannelEnvelope {
                                route: RouteKey::new("web", chat_id),
                                message_id,
                                turn_id: None,
                                text: text.to_string(),
                                sender_id: "web-user".to_string(),
                                attachments,
                                parent_id: None,
                                cli_kind: agent,
                            },
                        },
                        profile,
                        session_intent,
                        session_mode,
                    })
                }
                "set_mode" => {
                    let mode_id = string_field(&v, &["modeId", "mode_id", "permissionMode"])?;
                    Some(WebChatInput::SetMode { mode_id })
                }
                "set_config_option" => {
                    let config_id = string_field(&v, &["configId", "config_id"])?;
                    let value = string_field(&v, &["value"])?;
                    Some(WebChatInput::SetConfigOption { config_id, value })
                }
                "resume_session" => {
                    let agent = parse_web_agent(&v);
                    let profile = parse_web_profile(&v);
                    let session_id = v
                        .get("sessionId")
                        .and_then(|x| x.as_str())
                        .map(str::trim)
                        .filter(|x| !x.is_empty())?
                        .to_string();
                    let cwd = v
                        .get("sessionWorkspace")
                        .and_then(|x| x.as_str())
                        .map(str::trim)
                        .filter(|x| !x.is_empty())
                        .map(ToOwned::to_owned);

                    Some(WebChatInput::ResumeSession {
                        agent,
                        profile,
                        session_id,
                        cwd,
                    })
                }
                "stop" => Some(WebChatInput::Stop(ChannelInput::Stop {
                    route: RouteKey::new("web", chat_id),
                })),
                "permission_response" => {
                    let request_id = v.get("requestId").and_then(|x| x.as_str())?.to_string();
                    let outcome = match v.get("outcome").and_then(|x| x.as_str()) {
                        Some("cancelled") => acp::RequestPermissionOutcome::Cancelled,
                        _ => {
                            let option_id = v.get("optionId").and_then(|x| x.as_str())?;
                            acp::RequestPermissionOutcome::Selected(
                                acp::SelectedPermissionOutcome::new(option_id.to_string()),
                            )
                        }
                    };
                    Some(WebChatInput::PermissionResponse {
                        request_id,
                        response: acp::RequestPermissionResponse::new(outcome),
                    })
                }
                _ => None,
            }
        }
        Err(_) => {
            let trimmed = text.trim();
            if trimmed.is_empty() {
                None
            } else {
                Some(WebChatInput::Message {
                    input: ChannelInput::Message {
                        envelope: ChannelEnvelope {
                            route: RouteKey::new("web", chat_id),
                            message_id: Uuid::new_v4().to_string(),
                            turn_id: None,
                            text: trimmed.to_string(),
                            sender_id: "web-user".to_string(),
                            attachments: vec![],
                            parent_id: None,
                            cli_kind: None,
                        },
                    },
                    profile: None,
                    session_intent: None,
                    session_mode: None,
                })
            }
        }
    }
}

fn parse_web_attachments(value: &serde_json::Value, message_id: &str) -> Vec<Attachment> {
    value
        .get("attachments")
        .and_then(|items| items.as_array())
        .map(|items| {
            items
                .iter()
                .filter_map(|item| parse_web_attachment(item, message_id))
                .collect()
        })
        .unwrap_or_default()
}

fn parse_web_attachment(value: &serde_json::Value, message_id: &str) -> Option<Attachment> {
    let file_key = string_field(value, &["fileKey", "file_key", "uri", "url"])?;
    let file_name = string_field(value, &["fileName", "file_name", "name"]).unwrap_or_else(|| {
        file_key
            .rsplit('/')
            .next()
            .unwrap_or("attachment")
            .to_string()
    });
    let resource_type = string_field(
        value,
        &["resourceType", "resource_type", "mimeType", "mime_type"],
    )
    .unwrap_or_else(|| "application/octet-stream".to_string());
    let size = value
        .get("size")
        .and_then(|size| {
            size.as_i64()
                .or_else(|| size.as_u64().map(|size| size as i64))
        })
        .filter(|size| *size >= 0);

    Some(Attachment {
        message_id: message_id.to_string(),
        file_key,
        file_name,
        resource_type,
        size,
    })
}

fn string_field(value: &serde_json::Value, keys: &[&str]) -> Option<String> {
    keys.iter()
        .find_map(|key| value.get(*key).and_then(|item| item.as_str()))
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

fn parse_web_agent(value: &serde_json::Value) -> Option<String> {
    value
        .get("agent")
        .and_then(|x| x.as_str())
        .map(str::trim)
        .filter(|x| !x.is_empty())
        .map(ToOwned::to_owned)
}

fn parse_web_profile(value: &serde_json::Value) -> Option<String> {
    value
        .get("profileId")
        .and_then(|x| x.as_str())
        .map(str::trim)
        .filter(|x| !x.is_empty())
        .map(ToOwned::to_owned)
}

fn parse_web_session_mode(value: &serde_json::Value) -> Option<String> {
    string_field(value, &["permissionMode", "modeId", "mode_id"])
}

fn parse_web_session_intent(
    value: &serde_json::Value,
    agent: Option<String>,
) -> Option<WebChatSessionIntent> {
    match value.get("sessionAction").and_then(|x| x.as_str()) {
        Some("new") => {
            return Some(WebChatSessionIntent::New {
                cwd: parse_web_session_workspace(value),
            });
        }
        Some("resume") | None => {}
        Some(_) => return None,
    }

    let session_id = value
        .get("sessionId")
        .and_then(|x| x.as_str())
        .map(str::trim)
        .filter(|x| !x.is_empty())?;
    let cwd = parse_web_session_workspace(value);

    Some(WebChatSessionIntent::Resume {
        agent,
        session_id: session_id.to_string(),
        cwd,
    })
}

fn parse_web_session_workspace(value: &serde_json::Value) -> Option<String> {
    value
        .get("sessionWorkspace")
        .and_then(|x| x.as_str())
        .map(str::trim)
        .filter(|x| !x.is_empty())
        .map(ToOwned::to_owned)
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
                    .unwrap_or_else(|| "default".to_string()),
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

#[cfg(test)]
mod tests {
    use super::*;

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
}
