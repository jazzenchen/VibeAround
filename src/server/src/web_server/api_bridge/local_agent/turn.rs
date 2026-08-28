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

/// Server-side deadline for the whole startup chain — spawn (which may
/// lazily install the agent binary), session create/attach, and config
/// application. The prompt itself is deliberately unbounded: long agent
/// turns are legitimate, and the client's disconnect or a displacing
/// request are the cancellation paths.
const STARTUP_DEADLINE: std::time::Duration = std::time::Duration::from_secs(180);

async fn with_startup_deadline<T>(
    stage: &str,
    action: impl std::future::Future<Output = Result<T, TurnStartError>>,
) -> Result<T, TurnStartError> {
    match tokio::time::timeout(STARTUP_DEADLINE, action).await {
        Ok(result) => result,
        Err(_) => Err(TurnStartError::Timeout(format!(
            "{stage} did not become ready within {}s",
            STARTUP_DEADLINE.as_secs()
        ))),
    }
}

/// Failure before the first byte of the answer. Upstream providers report
/// request-validation problems as real HTTP statuses even on streaming
/// requests, so the whole startup chain runs before response headers are
/// sent and its failures map here; only mid-generation errors ride inside
/// the stream.
pub(super) enum TurnStartError {
    /// The client asked for something the agent does not have. 400.
    BadRequest(String),
    /// The request contains nothing answerable. 422.
    Unprocessable(String),
    /// The conversation can no longer serve this request. 409.
    Conflict(String),
    /// The startup chain missed its deadline. 504.
    Timeout(String),
    /// The agent failed to come up. 502.
    Upstream(String),
}

impl TurnStartError {
    pub(super) fn into_response(self) -> Response {
        let (status, message) = match self {
            Self::BadRequest(message) => (StatusCode::BAD_REQUEST, message),
            Self::Unprocessable(message) => (StatusCode::UNPROCESSABLE_ENTITY, message),
            Self::Conflict(message) => (StatusCode::CONFLICT, message),
            Self::Timeout(message) => (StatusCode::GATEWAY_TIMEOUT, message),
            Self::Upstream(message) => (StatusCode::BAD_GATEWAY, message),
        };
        json_error(status, &message)
    }

    /// Invalid-params errors (unknown model, unknown mode) are the client's
    /// fault; everything else the agent's.
    fn from_acp(error: acp::Error) -> Self {
        if error.code == (-32602).into() {
            Self::BadRequest(error.message.to_string())
        } else {
            Self::Upstream(error.message.to_string())
        }
    }
}
use crate::web_server::api_bridge::completion::translated_completion_events_response;
use crate::web_server::api_bridge::stream::encode_wire_sse_event;
use common::agent::AgentClientHandler;

#[derive(Debug)]
pub(super) struct LocalAgentTurn {
    pub(super) agent_id: String,
    pub(super) profile_id: String,
    pub(super) model_id: Option<String>,
    pub(super) permission_mode: Option<String>,
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
    pub(super) permission_mode: Option<String>,
    pub(super) response_id: String,
    /// Full-history seed prompt; `None` when the client sent increments only
    /// (a chained Responses request) and the history cannot be rebuilt.
    pub(super) seed_prompt: Option<Vec<acp::ContentBlock>>,
    /// The new tail segment; `None` when the transcript ends with an
    /// assistant turn and there is nothing new to answer.
    pub(super) tail_prompt: Option<Vec<acp::ContentBlock>>,
}

/// Everything a turn needs after a fully successful startup: response
/// headers can be sent, and only mid-generation failures remain possible.
pub(super) struct PreparedTurn {
    kind: PreparedKind,
    agent: Arc<common::agent::Agent>,
    forwarder: Arc<TurnEventForwarder>,
    session_id: acp::SessionId,
    prompt: Vec<acp::ContentBlock>,
    response_id: String,
    model_id: Option<String>,
}

enum PreparedKind {
    /// Throwaway session: the agent process dies with the turn.
    Sessionless,
    Conversation {
        conversation: Arc<super::conversations::Conversation>,
        guard: super::conversations::TurnGuard,
        displaced: tokio::sync::watch::Receiver<bool>,
    },
}

/// Run the whole startup chain for a conversation turn: displace whatever is
/// in flight, bring the agent up, settle the session, apply model and mode.
/// On success the chain (for the Responses protocol) is advanced to the new
/// response id — a failed startup leaves the old id continuable.
pub(super) async fn prepare_conversation_turn(
    turn: ConversationTurn,
    advance_chain: bool,
) -> Result<PreparedTurn, TurnStartError> {
    let conversation = Arc::clone(&turn.conversation);
    let (guard, displaced) = conversation.begin_turn().await;

    let (generation, attach_options) =
        with_startup_deadline("agent startup", ensure_conversation_agent(&conversation)).await?;
    let agent = Arc::clone(&generation.agent);
    let forwarder = Arc::clone(&generation.forwarder);

    let (session_id, prompt) = match conversation.session_id() {
        Some(session_id) => {
            let prompt = turn.tail_prompt.clone().ok_or_else(|| {
                TurnStartError::Unprocessable("request adds no new input to answer".to_string())
            })?;
            let session_id = acp::SessionId::from(session_id);
            if let Some(options) = attach_options.as_deref() {
                // A respawn handed us the config options, so a model choice
                // can still be applied to the resumed session.
                let agent = &agent;
                let session_id = &session_id;
                let model_id = turn.model_id.as_deref();
                with_startup_deadline("session startup", async move {
                    apply_local_agent_model(agent, session_id, Some(options), model_id)
                        .await
                        .map_err(TurnStartError::from_acp)
                })
                .await?;
            }
            (session_id, prompt)
        }
        None => {
            // Fresh or lost session: seed it with the full history.
            let seed = turn.seed_prompt.clone().ok_or_else(|| {
                TurnStartError::Conflict(
                    "conversation state was lost; start a new chain without previous_response_id"
                        .to_string(),
                )
            })?;
            let agent_ref = &agent;
            let conversation_ref = &conversation;
            let model_id = turn.model_id.as_deref();
            let permission_mode = turn.permission_mode.as_deref();
            let session_id = with_startup_deadline("session startup", async move {
                let session = agent_ref
                    .new_session(acp::NewSessionRequest::new(
                        conversation_ref.workspace.clone(),
                    ))
                    .await
                    .map_err(TurnStartError::from_acp)?;
                conversation_ref.set_session_id(Some(session.session_id.to_string()));
                apply_local_agent_model(
                    agent_ref,
                    &session.session_id,
                    session.config_options.as_deref(),
                    model_id,
                )
                .await
                .map_err(TurnStartError::from_acp)?;
                apply_local_agent_permission_mode(
                    agent_ref,
                    &session.session_id,
                    session.modes.as_ref(),
                    permission_mode,
                )
                .await
                .map_err(TurnStartError::from_acp)?;
                Ok(session.session_id)
            })
            .await?;
            (session_id, seed)
        }
    };

    if advance_chain {
        super::conversations::registry().advance_response_id(&conversation, &turn.response_id);
    }

    Ok(PreparedTurn {
        kind: PreparedKind::Conversation {
            conversation,
            guard,
            displaced,
        },
        agent,
        forwarder,
        session_id,
        prompt,
        response_id: turn.response_id,
        model_id: turn.model_id,
    })
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
    TurnStartError,
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
    )
    .map_err(TurnStartError::Upstream)?;
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
    .map_err(|error| TurnStartError::Upstream(format!("{error:#}")))?;
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
    match prepare_local_agent_turn(turn).await {
        Ok(prepared) => completion_response_from(start_prepared_turn(prepared), protocol).await,
        Err(error) => error.into_response(),
    }
}

pub(super) async fn local_agent_stream_response(
    turn: LocalAgentTurn,
    protocol: BridgeProtocol,
) -> Response {
    match prepare_local_agent_turn(turn).await {
        Ok(prepared) => stream_response_from(start_prepared_turn(prepared), protocol),
        Err(error) => error.into_response(),
    }
}

pub(super) async fn conversation_completion_response(
    turn: ConversationTurn,
    protocol: BridgeProtocol,
    advance_chain: bool,
) -> Response {
    match prepare_conversation_turn(turn, advance_chain).await {
        Ok(prepared) => completion_response_from(start_prepared_turn(prepared), protocol).await,
        Err(error) => error.into_response(),
    }
}

pub(super) async fn conversation_stream_response(
    turn: ConversationTurn,
    protocol: BridgeProtocol,
    advance_chain: bool,
) -> Response {
    match prepare_conversation_turn(turn, advance_chain).await {
        Ok(prepared) => stream_response_from(start_prepared_turn(prepared), protocol),
        Err(error) => error.into_response(),
    }
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

/// Run the whole startup chain for a sessionless one-shot: spawn a throwaway
/// agent, create its session, apply model and mode.
pub(super) async fn prepare_local_agent_turn(
    turn: LocalAgentTurn,
) -> Result<PreparedTurn, TurnStartError> {
    let response_id = format!("resp_{}", Uuid::new_v4().simple());
    let route = common::routing::RouteKey::new(
        LOCAL_AGENT_CHANNEL_KIND,
        format!("api_{}", Uuid::new_v4().simple()),
    );
    let agent_id = common::resources::resolve_agent_id(&turn.agent_id)
        .map_err(|error| TurnStartError::BadRequest(error.to_string()))?;
    let (extra_args, env_vars) =
        launch_args_and_env(&agent_id, &turn.profile_id, &turn.workspace, &route)
            .map_err(TurnStartError::Upstream)?;
    let forwarder = TurnEventForwarder::new();
    let handler: Arc<dyn AgentClientHandler> = Arc::clone(&forwarder) as _;
    let ready = with_startup_deadline("agent startup", async {
        common::agent::Agent::spawn(
            agent_id,
            &route,
            &turn.workspace,
            common::agent::StartupSession::Fresh,
            handler,
            extra_args,
            env_vars,
        )
        .await
        .map_err(|error| TurnStartError::Upstream(format!("{error:#}")))
    })
    .await?;
    let agent = ready.agent;
    let startup = {
        let agent = &agent;
        let workspace = turn.workspace.clone();
        let model_id = turn.model_id.as_deref();
        let permission_mode = turn.permission_mode.as_deref();
        with_startup_deadline("session startup", async move {
            let session = agent
                .new_session(acp::NewSessionRequest::new(workspace))
                .await
                .map_err(TurnStartError::from_acp)?;
            apply_local_agent_model(
                agent,
                &session.session_id,
                session.config_options.as_deref(),
                model_id,
            )
            .await
            .map_err(TurnStartError::from_acp)?;
            apply_local_agent_permission_mode(
                agent,
                &session.session_id,
                session.modes.as_ref(),
                permission_mode,
            )
            .await
            .map_err(TurnStartError::from_acp)?;
            Ok(session.session_id)
        })
        .await
    };
    let session_id = match startup {
        Ok(session_id) => session_id,
        Err(error) => {
            agent.shutdown().await;
            return Err(error);
        }
    };
    Ok(PreparedTurn {
        kind: PreparedKind::Sessionless,
        agent,
        forwarder,
        session_id,
        prompt: turn.prompt,
        response_id,
        model_id: turn.model_id,
    })
}

/// Kick off the prompt phase of a fully prepared turn.
fn start_prepared_turn(prepared: PreparedTurn) -> TurnParts {
    let (tx, rx) = mpsc::unbounded_channel();
    let (cancel_tx, cancel_rx) = oneshot::channel();
    let run_tx = tx.clone();
    let run = async move {
        let result = run_prepared_turn(prepared, run_tx.clone(), cancel_rx).await;
        if let Err(message) = result {
            let _ = run_tx.send(LocalAgentTurnEvent::Failed(message));
        }
        let _ = run_tx.send(LocalAgentTurnEvent::Done);
    };
    (cancel_tx, rx, Box::pin(run))
}

async fn run_prepared_turn(
    prepared: PreparedTurn,
    tx: mpsc::UnboundedSender<LocalAgentTurnEvent>,
    mut client_cancel: oneshot::Receiver<()>,
) -> Result<(), String> {
    let PreparedTurn {
        kind,
        agent,
        forwarder,
        session_id,
        prompt,
        response_id,
        model_id,
    } = prepared;
    let mut displaced = match &kind {
        PreparedKind::Sessionless => None,
        PreparedKind::Conversation { displaced, .. } => Some(displaced.clone()),
    };

    let message_id = format!("msg_{}", Uuid::new_v4().simple());
    send_events(
        &tx,
        vec![
            UniversalEvent::ResponseStart {
                id: Some(response_id),
                model: model_id,
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

    forwarder.install(tx.clone());
    let result: Result<acp::PromptResponse, String> = {
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
                superseded = displacement_signal(&mut displaced), if cancelled.is_none() => {
                    if superseded {
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
    };
    // The reasoning block was opened lazily by the forwarder; upstream
    // dialects close every block they open (Anthropic sends a
    // content_block_stop for the thinking block), so mirror that before
    // reading the slot away.
    let reasoning_started = forwarder.reasoning_started();
    forwarder.clear();
    match kind {
        PreparedKind::Sessionless => {
            let _ = forwarder.prompt_finished(result.is_ok()).await;
            agent.shutdown().await;
        }
        PreparedKind::Conversation {
            conversation,
            guard,
            ..
        } => {
            conversation.clear_dead_agent();
            drop(guard);
        }
    }
    let response = result?;
    if reasoning_started {
        send_events(
            &tx,
            vec![UniversalEvent::ContentDone {
                index: 1,
                final_block: None,
            }],
        );
    }
    send_events(
        &tx,
        final_events(
            response.stop_reason,
            response.usage.as_ref().map(acp_usage_to_universal),
        ),
    );
    Ok(())
}

/// Resolves when the conversation displaces this turn; pends forever for
/// sessionless turns.
async fn displacement_signal(displaced: &mut Option<tokio::sync::watch::Receiver<bool>>) -> bool {
    match displaced {
        Some(receiver) => match receiver.changed().await {
            Ok(()) => *receiver.borrow(),
            Err(_) => true,
        },
        None => std::future::pending().await,
    }
}

/// Apply the client's requested permission mode to a freshly created
/// session, when the agent advertises one with that id. Tool permissions
/// over the API are otherwise auto-refused (there is no human to ask), so
/// autonomous use wants acceptEdits or bypassPermissions here.
async fn apply_local_agent_permission_mode(
    agent: &common::agent::Agent,
    session_id: &acp::SessionId,
    modes: Option<&acp::SessionModeState>,
    requested: Option<&str>,
) -> acp::Result<()> {
    let Some(requested) = requested
        .map(str::trim)
        .filter(|requested| !requested.is_empty())
    else {
        return Ok(());
    };
    let Some(modes) = modes else {
        return Ok(());
    };
    // Aliases translate to the agent's advertised id when one matches;
    // anything else goes through as-is and the agent judges it — its own
    // invalid-params error flows back, no pre-check on our side.
    let canonical = canonical_permission_mode(requested);
    let mode_id = modes
        .available_modes
        .iter()
        .find(|mode| {
            let id = mode.id.to_string();
            id == requested || Some(id.as_str()) == canonical
        })
        .map(|mode| mode.id.clone())
        .unwrap_or_else(|| acp::SessionModeId::new(requested.to_string()));
    agent
        .set_session_mode(acp::SetSessionModeRequest::new(session_id.clone(), mode_id))
        .await?;
    Ok(())
}

fn canonical_permission_mode(mode: &str) -> Option<&'static str> {
    match mode {
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
    // No membership pre-check: the agent owns its model list and its own
    // invalid-params error flows back to the client.
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

    /// Whether the current turn opened the lazy reasoning block; the turn
    /// closes it before the finals, mirroring upstream dialects.
    pub(super) fn reasoning_started(&self) -> bool {
        self.lock().reasoning_started
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
