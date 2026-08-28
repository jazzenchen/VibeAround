use std::io;
use std::path::PathBuf;
use std::sync::Arc;

use agent_client_protocol::schema::v1 as acp;
use axum::body::Body;
use axum::http::{header, StatusCode};
use axum::response::Response;
use bytes::Bytes as ResponseBytes;
use tokio::sync::{mpsc, oneshot};
use uuid::Uuid;
use va_ai_api_bridge::{
    ContentBlock as UniversalContentBlock, EncodeState, Extensions, Role, UniversalEvent,
};

use super::events::{acp_notification_to_events, acp_usage_to_universal, final_events};
use super::{
    json_error, launch_args_and_env, send_events, BridgeProtocol, LOCAL_AGENT_CHANNEL_KIND,
};
use crate::web_server::api_bridge::completion::translated_completion_events_response;
use crate::web_server::api_bridge::stream::encode_wire_sse_event;
use common::agent::AgentClientHandler;

#[derive(Debug)]
pub(super) struct LocalAgentTurn {
    pub(super) agent_id: String,
    pub(super) profile_id: String,
    pub(super) model_id: Option<String>,
    pub(super) workspace: PathBuf,
    pub(super) prompt: Vec<acp::ContentBlock>,
}

pub(super) enum LocalAgentTurnEvent {
    Events(Vec<UniversalEvent>),
    Failed(String),
    Done,
}

/// One turn on a registered conversation (the stateful mode). The caller
/// builds both prompt renderings up front because whether the backend session
/// needs seeding is only known once the turn holds the conversation.
pub(super) struct ConversationTurn {
    pub(super) conversation: Arc<super::conversations::Conversation>,
    pub(super) model_id: Option<String>,
    pub(super) response_id: String,
    /// Full-history seed prompt; `None` when the client sent increments only
    /// (a chained Responses request) and the history cannot be rebuilt.
    pub(super) seed_prompt: Option<Vec<acp::ContentBlock>>,
    /// The new tail segment; `None` when the transcript ends with an
    /// assistant turn and there is nothing new to answer.
    pub(super) tail_prompt: Option<Vec<acp::ContentBlock>>,
}

async fn run_conversation_turn(
    turn: ConversationTurn,
    tx: mpsc::UnboundedSender<LocalAgentTurnEvent>,
    mut client_cancel: oneshot::Receiver<()>,
) -> Result<(), String> {
    let conversation = Arc::clone(&turn.conversation);
    let (guard, mut displaced) = conversation.begin_turn().await;
    let _guard = guard;

    let message_id = format!("msg_{}", Uuid::new_v4().simple());
    send_events(
        &tx,
        vec![
            UniversalEvent::ResponseStart {
                id: Some(turn.response_id.clone()),
                model: turn.model_id.clone(),
                extensions: Extensions::new(),
            },
            UniversalEvent::MessageStart {
                id: message_id,
                role: Role::Assistant,
                extensions: Extensions::new(),
            },
            UniversalEvent::ContentStart {
                index: 0,
                block: UniversalContentBlock::Text {
                    text: String::new(),
                },
            },
        ],
    );

    let (generation, attach_options) = ensure_conversation_agent(&conversation).await?;
    let agent = Arc::clone(&generation.agent);
    let forwarder = Arc::clone(&generation.forwarder);

    let result: Result<acp::PromptResponse, String> = async {
        let (session_id, prompt) = match conversation.session_id() {
            Some(session_id) => {
                let prompt = turn
                    .tail_prompt
                    .clone()
                    .ok_or_else(|| "request adds no new input to answer".to_string())?;
                let session_id = acp::SessionId::from(session_id);
                if let Some(options) = attach_options.as_deref() {
                    // A respawn handed us the config options, so a model
                    // choice can still be applied to the resumed session.
                    apply_local_agent_model(
                        &agent,
                        &session_id,
                        Some(options),
                        turn.model_id.as_deref(),
                    )
                    .await
                    .map_err(|error| error.message.to_string())?;
                }
                (session_id, prompt)
            }
            None => {
                // Fresh or lost session: seed it with the full history.
                let seed = turn.seed_prompt.clone().ok_or_else(|| {
                    "conversation state was lost; start a new chain without previous_response_id"
                        .to_string()
                })?;
                let session = agent
                    .new_session(acp::NewSessionRequest::new(conversation.workspace.clone()))
                    .await
                    .map_err(|error| error.message.to_string())?;
                conversation.set_session_id(Some(session.session_id.to_string()));
                apply_local_agent_model(
                    &agent,
                    &session.session_id,
                    session.config_options.as_deref(),
                    turn.model_id.as_deref(),
                )
                .await
                .map_err(|error| error.message.to_string())?;
                (session.session_id, seed)
            }
        };

        forwarder.install(tx.clone());
        let prompt_call = agent.prompt(acp::PromptRequest::new(session_id.clone(), prompt));
        tokio::pin!(prompt_call);
        let mut cancelled: Option<String> = None;
        loop {
            tokio::select! {
                response = &mut prompt_call => {
                    break match cancelled {
                        Some(reason) => Err(reason),
                        None => response.map_err(|error| error.message.to_string()),
                    };
                }
                _ = &mut client_cancel, if cancelled.is_none() => {
                    cancelled = Some("local agent request cancelled".to_string());
                    let _ = agent
                        .cancel(acp::CancelNotification::new(session_id.clone()))
                        .await;
                }
                changed = displaced.changed(), if cancelled.is_none() => {
                    if changed.is_err() || *displaced.borrow() {
                        cancelled = Some(
                            "superseded by a newer request in this conversation".to_string(),
                        );
                        let _ = agent
                            .cancel(acp::CancelNotification::new(session_id.clone()))
                            .await;
                    }
                }
            }
        }
    }
    .await;
    forwarder.clear();
    conversation.clear_dead_agent();
    let response = result?;
    send_events(
        &tx,
        final_events(
            response.stop_reason,
            response.usage.as_ref().map(acp_usage_to_universal),
        ),
    );
    Ok(())
}

/// The conversation's live agent generation, spawning a fresh one when the
/// previous process is gone. A respawn resumes the recorded session when the
/// agent still has it; a session the agent lost is cleared so the caller
/// reseeds.
async fn ensure_conversation_agent(
    conversation: &Arc<super::conversations::Conversation>,
) -> Result<
    (
        super::conversations::AgentGeneration,
        Option<Vec<acp::SessionConfigOption>>,
    ),
    String,
> {
    if let Some(generation) = conversation.live_agent() {
        return Ok((generation, None));
    }
    let recorded_session = conversation.session_id();
    let startup = match recorded_session.clone() {
        Some(session_id) => common::agent::StartupSession::Resume(session_id),
        None => common::agent::StartupSession::Fresh,
    };
    let (extra_args, env_vars) = launch_args_and_env(
        &conversation.agent_id,
        &conversation.profile_id,
        &conversation.workspace,
        &conversation.route,
    )?;
    let forwarder = TurnEventForwarder::new();
    let handler: Arc<dyn AgentClientHandler> = Arc::clone(&forwarder) as _;
    let ready = common::agent::Agent::spawn(
        conversation.agent_id.clone(),
        &conversation.route,
        &conversation.workspace,
        startup,
        handler,
        extra_args,
        env_vars,
    )
    .await
    .map_err(|error| format!("{error:#}"))?;
    if recorded_session.is_some() && ready.startup_session_id.is_none() {
        // The agent no longer has the session; the next prompt must reseed.
        conversation.set_session_id(None);
    }
    let attach_options = ready
        .startup_session_id
        .is_some()
        .then_some(ready.startup_config_options)
        .flatten();
    let generation = super::conversations::AgentGeneration {
        agent: ready.agent,
        forwarder,
    };
    conversation.set_agent(generation.clone());
    Ok((generation, attach_options))
}

type TurnParts = (
    oneshot::Sender<()>,
    mpsc::UnboundedReceiver<LocalAgentTurnEvent>,
    std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send + 'static>>,
);

pub(super) async fn local_agent_completion_response(
    turn: LocalAgentTurn,
    protocol: BridgeProtocol,
) -> Response {
    completion_response_from(start_local_agent_turn(turn), protocol).await
}

pub(super) fn local_agent_stream_response(
    turn: LocalAgentTurn,
    protocol: BridgeProtocol,
) -> Response {
    stream_response_from(start_local_agent_turn(turn), protocol)
}

pub(super) async fn conversation_completion_response(
    turn: ConversationTurn,
    protocol: BridgeProtocol,
) -> Response {
    completion_response_from(start_conversation_turn(turn), protocol).await
}

pub(super) fn conversation_stream_response(
    turn: ConversationTurn,
    protocol: BridgeProtocol,
) -> Response {
    stream_response_from(start_conversation_turn(turn), protocol)
}

async fn completion_response_from(parts: TurnParts, protocol: BridgeProtocol) -> Response {
    let (_cancel_tx, mut rx, run) = parts;
    tokio::spawn(run);
    let mut events = Vec::new();
    let mut failed = None;
    while let Some(item) = rx.recv().await {
        match item {
            LocalAgentTurnEvent::Events(mut next) => events.append(&mut next),
            LocalAgentTurnEvent::Failed(message) => failed = Some(message),
            LocalAgentTurnEvent::Done => break,
        }
    }
    if let Some(message) = failed {
        return super::record_json_error(None, StatusCode::BAD_GATEWAY, &message);
    }
    translated_completion_events_response(events, protocol, None, None)
}

fn stream_response_from(parts: TurnParts, protocol: BridgeProtocol) -> Response {
    let (cancel_tx, rx, run) = parts;
    tokio::spawn(run);
    let stream = futures_util::stream::unfold(
        (rx, EncodeState::default(), protocol, cancel_tx),
        |(mut rx, mut encode_state, protocol, cancel_tx)| async move {
            loop {
                let item = rx.recv().await?;
                match item {
                    LocalAgentTurnEvent::Events(events) => {
                        let wire_events =
                            match protocol.encode_agent_events(&events, &mut encode_state) {
                                Ok(events) => events,
                                Err(error) => {
                                    return Some((
                                        Err(io::Error::new(
                                            io::ErrorKind::InvalidData,
                                            error.to_string(),
                                        )),
                                        (rx, encode_state, protocol, cancel_tx),
                                    ));
                                }
                            };
                        let body = wire_events
                            .into_iter()
                            .map(encode_wire_sse_event)
                            .collect::<String>();
                        if body.is_empty() {
                            continue;
                        }
                        return Some((
                            Ok(ResponseBytes::from(body)),
                            (rx, encode_state, protocol, cancel_tx),
                        ));
                    }
                    LocalAgentTurnEvent::Failed(message) => {
                        let event = UniversalEvent::Error { message, raw: None };
                        let body = protocol
                            .encode_agent_events(&[event], &mut encode_state)
                            .map(|events| {
                                events
                                    .into_iter()
                                    .map(encode_wire_sse_event)
                                    .collect::<String>()
                            })
                            .map_err(|error| {
                                io::Error::new(io::ErrorKind::InvalidData, error.to_string())
                            });
                        return Some((
                            body.map(ResponseBytes::from),
                            (rx, encode_state, protocol, cancel_tx),
                        ));
                    }
                    LocalAgentTurnEvent::Done => return None,
                }
            }
        },
    );

    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "text/event-stream")
        .header(header::CACHE_CONTROL, "no-cache")
        .body(Body::from_stream(stream))
        .unwrap_or_else(|_| {
            json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "failed to build local agent stream response",
            )
        })
}

fn start_local_agent_turn(
    turn: LocalAgentTurn,
) -> (
    oneshot::Sender<()>,
    mpsc::UnboundedReceiver<LocalAgentTurnEvent>,
    std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send + 'static>>,
) {
    let (tx, rx) = mpsc::unbounded_channel();
    let (cancel_tx, cancel_rx) = oneshot::channel();
    let handler = TurnEventForwarder::new();
    handler.install(tx.clone());
    let run_tx = tx.clone();
    let run = async move {
        let result =
            run_local_agent_turn(turn, Arc::clone(&handler), run_tx.clone(), cancel_rx).await;
        if let Err(message) = result {
            let _ = run_tx.send(LocalAgentTurnEvent::Failed(message));
        }
        let _ = run_tx.send(LocalAgentTurnEvent::Done);
    };
    (cancel_tx, rx, Box::pin(run))
}

/// Kick off one turn on a registered conversation. Shape-compatible with
/// [`start_local_agent_turn`] so both feed the same response builders.
pub(super) fn start_conversation_turn(
    turn: ConversationTurn,
) -> (
    oneshot::Sender<()>,
    mpsc::UnboundedReceiver<LocalAgentTurnEvent>,
    std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send + 'static>>,
) {
    let (tx, rx) = mpsc::unbounded_channel();
    let (cancel_tx, cancel_rx) = oneshot::channel();
    let run_tx = tx.clone();
    let run = async move {
        let result = run_conversation_turn(turn, run_tx.clone(), cancel_rx).await;
        if let Err(message) = result {
            let _ = run_tx.send(LocalAgentTurnEvent::Failed(message));
        }
        let _ = run_tx.send(LocalAgentTurnEvent::Done);
    };
    (cancel_tx, rx, Box::pin(run))
}

async fn run_local_agent_turn(
    turn: LocalAgentTurn,
    handler: Arc<TurnEventForwarder>,
    tx: mpsc::UnboundedSender<LocalAgentTurnEvent>,
    mut cancel_rx: oneshot::Receiver<()>,
) -> Result<(), String> {
    let response_id = format!("resp_{}", Uuid::new_v4().simple());
    let message_id = format!("msg_{}", Uuid::new_v4().simple());
    let route = common::routing::RouteKey::new(
        LOCAL_AGENT_CHANNEL_KIND,
        format!("api_{}", Uuid::new_v4().simple()),
    );
    send_events(
        &tx,
        vec![
            UniversalEvent::ResponseStart {
                id: Some(response_id),
                model: turn.model_id.clone(),
                extensions: Extensions::new(),
            },
            UniversalEvent::MessageStart {
                id: message_id,
                role: Role::Assistant,
                extensions: Extensions::new(),
            },
            UniversalEvent::ContentStart {
                index: 0,
                block: UniversalContentBlock::Text {
                    text: String::new(),
                },
            },
        ],
    );

    let agent_id =
        common::resources::resolve_agent_id(&turn.agent_id).map_err(|error| error.to_string())?;
    let (extra_args, env_vars) =
        launch_args_and_env(&agent_id, &turn.profile_id, &turn.workspace, &route)?;
    let ready = common::agent::Agent::spawn(
        agent_id,
        &route,
        &turn.workspace,
        common::agent::StartupSession::Fresh,
        handler.clone(),
        extra_args,
        env_vars,
    )
    .await
    .map_err(|error| format!("{error:#}"))?;
    let agent = ready.agent;
    let result: Result<acp::PromptResponse, String> = async {
        let session = agent
            .new_session(acp::NewSessionRequest::new(turn.workspace.clone()))
            .await
            .map_err(|error| error.message.to_string())?;
        apply_local_agent_model(
            &agent,
            &session.session_id,
            session.config_options.as_deref(),
            turn.model_id.as_deref(),
        )
        .await
        .map_err(|error| error.message.to_string())?;
        let session_id = session.session_id.clone();
        tokio::select! {
            response = agent.prompt(acp::PromptRequest::new(session.session_id, turn.prompt)) => {
                response.map_err(|error| error.message.to_string())
            }
            _ = &mut cancel_rx => {
                let _ = agent.cancel(acp::CancelNotification::new(session_id)).await;
                Err("local agent request cancelled".to_string())
            }
        }
    }
    .await;
    let _ = handler.prompt_finished(result.is_ok()).await;
    agent.shutdown().await;
    let response = result?;
    send_events(
        &tx,
        final_events(
            response.stop_reason,
            response.usage.as_ref().map(acp_usage_to_universal),
        ),
    );
    Ok(())
}

async fn apply_local_agent_model(
    agent: &common::agent::Agent,
    session_id: &acp::SessionId,
    config_options: Option<&[acp::SessionConfigOption]>,
    model_id: Option<&str>,
) -> acp::Result<()> {
    let Some(model_id) = model_id.map(str::trim).filter(|model| !model.is_empty()) else {
        return Ok(());
    };
    let Some(config_id) = model_config_option_id(config_options) else {
        return Ok(());
    };
    agent
        .set_session_config_option(acp::SetSessionConfigOptionRequest::new(
            session_id.clone(),
            config_id,
            model_id,
        ))
        .await?;
    Ok(())
}

pub(super) fn model_config_option_id(
    options: Option<&[acp::SessionConfigOption]>,
) -> Option<String> {
    options?
        .iter()
        .find(|option| is_model_config_option(option))
        .map(|option| option.id.to_string())
}

fn is_model_config_option(option: &acp::SessionConfigOption) -> bool {
    matches!(
        option.category,
        Some(acp::SessionConfigOptionCategory::Model)
    ) || option.id.to_string().eq_ignore_ascii_case("model")
}

/// Forwards agent notifications to whichever turn is currently listening.
///
/// Installed once at agent spawn and kept for the whole generation: a
/// sessionless turn installs its sender immediately and the process dies with
/// the turn, while a conversation installs a fresh sender per request and
/// clears it afterwards — notifications between turns are dropped.
pub(super) struct TurnEventForwarder {
    slot: std::sync::Mutex<ForwarderSlot>,
}

#[derive(Default)]
struct ForwarderSlot {
    tx: Option<mpsc::UnboundedSender<LocalAgentTurnEvent>>,
    reasoning_started: bool,
}

impl TurnEventForwarder {
    pub(super) fn new() -> Arc<Self> {
        Arc::new(Self {
            slot: std::sync::Mutex::new(ForwarderSlot::default()),
        })
    }

    /// Route notifications to this turn from now on.
    pub(super) fn install(&self, tx: mpsc::UnboundedSender<LocalAgentTurnEvent>) {
        *self.lock() = ForwarderSlot {
            tx: Some(tx),
            reasoning_started: false,
        };
    }

    pub(super) fn clear(&self) {
        *self.lock() = ForwarderSlot::default();
    }

    fn forward(&self, mut events: Vec<UniversalEvent>) {
        let mut slot = self.lock();
        if slot.tx.is_none() {
            return;
        }
        if events
            .iter()
            .any(|event| matches!(event, UniversalEvent::ReasoningDelta { .. }))
            && !slot.reasoning_started
        {
            slot.reasoning_started = true;
            events.insert(0, super::events::reasoning_content_start());
        }
        if events.is_empty() {
            return;
        }
        if let Some(tx) = slot.tx.as_ref() {
            let _ = tx.send(LocalAgentTurnEvent::Events(events));
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, ForwarderSlot> {
        self.slot
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

#[async_trait::async_trait]
impl common::agent::AgentClientHandler for TurnEventForwarder {
    async fn session_notification(&self, args: acp::SessionNotification) -> acp::Result<()> {
        self.forward(acp_notification_to_events(&args));
        Ok(())
    }

    async fn request_permission(
        &self,
        _args: acp::RequestPermissionRequest,
    ) -> acp::Result<acp::RequestPermissionResponse> {
        Ok(acp::RequestPermissionResponse::new(
            acp::RequestPermissionOutcome::Cancelled,
        ))
    }
}
