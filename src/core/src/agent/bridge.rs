//! `AcpAgentBridge` — the manager-owned `ProcessBridge` for ACP coding
//! CLIs.
//!
//! On spawn the supervisor hands this bridge a `StdioPipes`. The bridge
//! wires the pipes into an ACP client connection, performs the initial
//! handshake, optionally resumes a session, then publishes the assembled
//! [`AgentReady`] to the caller via a oneshot before holding the connection
//! open until cancellation or child stdout EOF.
//!
//! Unlike [`ChannelPluginBridge`], the agent has **no restart policy**
//! ([`RestartPolicy::Never`]). One bridge = one spawn = one `AgentReady`.
//! The `BridgeFactory` handed to [`Supervisor::register`] is therefore
//! single-shot: we build the bridge eagerly, stash it in a `Mutex<Option>`,
//! and `take()` it the first (and only) time the factory is invoked.
//!
//! [`ChannelPluginBridge`]: crate::channels::plugin_bridge::ChannelPluginBridge
//! [`RestartPolicy::Never`]: crate::process::RestartPolicy::Never
//! [`Supervisor::register`]: crate::process::Supervisor::register

use std::path::PathBuf;
use std::sync::Arc;

use acp::schema;
use agent_client_protocol as acp;
use anyhow::anyhow;
use tokio::sync::oneshot;
use tokio_util::compat::{TokioAsyncReadCompatExt, TokioAsyncWriteCompatExt};

use crate::process::acp_transport::notifying_stdio_transport;
use crate::process::bridge::{BridgeExit, BridgeFuture, CancelSignal, ProcessBridge, StdioPipes};

use super::runtime::{Agent, AgentClientHandler, AgentReady, StartupSession};

/// Single-shot bridge that initializes an ACP connection on its own
/// thread, then runs the IO loop until cancelled or the child exits.
pub struct AcpAgentBridge {
    pub agent_id: String,
    pub cwd: PathBuf,
    pub startup_session: StartupSession,
    pub client_handler: Arc<dyn AgentClientHandler>,
    pub ready_tx: oneshot::Sender<anyhow::Result<AgentReady>>,
}

impl ProcessBridge for AcpAgentBridge {
    fn run(self: Box<Self>, pipes: StdioPipes, cancel: CancelSignal) -> BridgeFuture {
        let this = *self;
        Box::pin(async move {
            let AcpAgentBridge {
                agent_id,
                cwd,
                startup_session,
                client_handler,
                ready_tx,
            } = this;

            drive_agent_bridge(
                agent_id,
                cwd,
                startup_session,
                client_handler,
                ready_tx,
                pipes,
                cancel,
            )
            .await
        })
    }
}

async fn drive_agent_bridge(
    agent_id: String,
    cwd: PathBuf,
    startup_session: StartupSession,
    client_handler: Arc<dyn AgentClientHandler>,
    ready_tx: oneshot::Sender<anyhow::Result<AgentReady>>,
    pipes: StdioPipes,
    mut cancel: CancelSignal,
) -> BridgeExit {
    let (transport, mut stdio_closed) =
        notifying_stdio_transport(pipes.stdin.compat_write(), pipes.stdout.compat());

    let permission_handler = Arc::clone(&client_handler);
    let notification_handler = Arc::clone(&client_handler);
    let agent_id_for_run = agent_id.clone();

    let run_result = acp::Client
        .builder()
        .name(format!("{}-agent-client", agent_id))
        .on_receive_request(
            async move |args: schema::RequestPermissionRequest, responder, _cx| {
                responder.respond_with_result(permission_handler.request_permission(args).await)
            },
            acp::on_receive_request!(),
        )
        .on_receive_notification(
            async move |args: schema::SessionNotification, _cx| {
                notification_handler.session_notification(args).await
            },
            acp::on_receive_notification!(),
        )
        .connect_with(transport, async move |conn| {
            let init_req = schema::InitializeRequest::new(schema::ProtocolVersion::V1).client_info(
                schema::Implementation::new("vibearound", env!("CARGO_PKG_VERSION"))
                    .title("VibeAround"),
            );
            let initialize = match conn.send_request(init_req).block_task().await {
                Ok(response) => response,
                Err(error) => {
                    let _ = ready_tx.send(Err(anyhow!(
                        "ACP initialize failed for {}: {}",
                        agent_id_for_run,
                        error
                    )));
                    return Err(error);
                }
            };

            // Optional startup session attachment. `session/load` is used by
            // web playback flows; `session/resume` attaches without replay.
            let mut startup_modes = None;
            let mut startup_config_options = None;
            let startup_session_id = match startup_session.clone() {
                StartupSession::Fresh => None,
                StartupSession::Load(session_id) => {
                    match conn
                        .send_request(schema::LoadSessionRequest::new(
                            session_id.clone(),
                            cwd.clone(),
                        ))
                        .block_task()
                        .await
                    {
                        Ok(response) => {
                            startup_modes = response.modes;
                            startup_config_options = response.config_options;
                            Some(session_id)
                        }
                        Err(error) => {
                            tracing::info!(
                                "[{}-agent] failed to load session {}, starting without session: {}",
                                agent_id_for_run,
                                session_id,
                                error
                            );
                            None
                        }
                    }
                }
                StartupSession::Resume(session_id) => {
                    if initialize
                        .agent_capabilities
                        .session_capabilities
                        .resume
                        .is_none()
                    {
                        tracing::info!(
                            "[{}-agent] session/resume unsupported for {}, starting without session",
                            agent_id_for_run,
                            session_id
                        );
                        None
                    } else {
                        match conn
                            .send_request(schema::ResumeSessionRequest::new(
                                session_id.clone(),
                                cwd.clone(),
                            ))
                            .block_task()
                            .await
                        {
                            Ok(response) => {
                                startup_modes = response.modes;
                                startup_config_options = response.config_options;
                                Some(session_id)
                            }
                            Err(error) => {
                                tracing::info!(
                                    "[{}-agent] failed to resume session {}, starting without session: {}",
                                    agent_id_for_run,
                                    session_id,
                                    error
                                );
                                None
                            }
                        }
                    }
                }
            };

            let agent = Agent::from_connection(
                conn,
                agent_id_for_run.clone(),
                initialize.clone(),
                startup_session_id.clone(),
            );
            let ready = AgentReady {
                agent,
                startup_session_id,
                startup_modes,
                startup_config_options,
                initialize,
            };
            if ready_tx.send(Ok(ready)).is_err() {
                return Ok(BridgeExit::Cancelled);
            }

            tokio::select! {
                _ = &mut stdio_closed => Ok(BridgeExit::Clean),
                _ = cancel.wait_for(|v| *v) => {
                    tracing::info!("[{}-agent] cancelled by supervisor", agent_id_for_run);
                    Ok(BridgeExit::Cancelled)
                }
            }
        })
        .await;

    match run_result {
        Ok(exit) => exit,
        Err(error) => BridgeExit::ProtocolError(
            anyhow!(error).context(format!("ACP IO terminated for {}", agent_id)),
        ),
    }
}
