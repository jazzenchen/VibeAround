use std::io;
use std::path::PathBuf;
use std::sync::Arc;

use agent_client_protocol::schema::v1 as acp;
use axum::body::Body;
use axum::http::{header, StatusCode};
use axum::response::Response;
use bytes::Bytes as ResponseBytes;
use tokio::sync::{mpsc, oneshot, Mutex};
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

pub(super) async fn local_agent_completion_response(
    turn: LocalAgentTurn,
    protocol: BridgeProtocol,
) -> Response {
    let (_cancel_tx, mut rx, run) = start_local_agent_turn(turn);
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

pub(super) fn local_agent_stream_response(
    turn: LocalAgentTurn,
    protocol: BridgeProtocol,
) -> Response {
    let (cancel_tx, rx, run) = start_local_agent_turn(turn);
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
    impl std::future::Future<Output = ()> + Send + 'static,
) {
    let (tx, rx) = mpsc::unbounded_channel();
    let (cancel_tx, cancel_rx) = oneshot::channel();
    let handler = Arc::new(ApiAgentClientHandler::new(tx.clone()));
    let run_tx = tx.clone();
    let run = async move {
        let result =
            run_local_agent_turn(turn, Arc::clone(&handler), run_tx.clone(), cancel_rx).await;
        if let Err(message) = result {
            let _ = run_tx.send(LocalAgentTurnEvent::Failed(message));
        }
        let _ = run_tx.send(LocalAgentTurnEvent::Done);
    };
    (cancel_tx, rx, run)
}

async fn run_local_agent_turn(
    turn: LocalAgentTurn,
    handler: Arc<ApiAgentClientHandler>,
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
        apply_local_agent_model(&agent, &session, turn.model_id.as_deref())
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
    session: &acp::NewSessionResponse,
    model_id: Option<&str>,
) -> acp::Result<()> {
    let Some(model_id) = model_id.map(str::trim).filter(|model| !model.is_empty()) else {
        return Ok(());
    };
    let Some(config_id) = model_config_option_id(session.config_options.as_deref()) else {
        return Ok(());
    };
    agent
        .set_session_config_option(acp::SetSessionConfigOptionRequest::new(
            session.session_id.clone(),
            config_id,
            model_id.to_string(),
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

struct ApiAgentClientHandler {
    tx: mpsc::UnboundedSender<LocalAgentTurnEvent>,
    state: Mutex<ApiHandlerState>,
}

#[derive(Default)]
struct ApiHandlerState {
    reasoning_started: bool,
}

impl ApiAgentClientHandler {
    fn new(tx: mpsc::UnboundedSender<LocalAgentTurnEvent>) -> Self {
        Self {
            tx,
            state: Mutex::new(ApiHandlerState::default()),
        }
    }
}

#[async_trait::async_trait]
impl common::agent::AgentClientHandler for ApiAgentClientHandler {
    async fn session_notification(&self, args: acp::SessionNotification) -> acp::Result<()> {
        let mut events = acp_notification_to_events(&args);
        if events
            .iter()
            .any(|event| matches!(event, UniversalEvent::ReasoningDelta { .. }))
        {
            let mut state = self.state.lock().await;
            if !state.reasoning_started {
                state.reasoning_started = true;
                events.insert(0, super::events::reasoning_content_start());
            }
        }
        send_events(&self.tx, events);
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
