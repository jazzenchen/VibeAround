//! `AcpAgentBridge` — the manager-owned `ProcessBridge` for ACP coding
//! CLIs.
//!
//! On spawn the supervisor hands this bridge a `StdioPipes`. The bridge
//! wires the pipes into an ACP client connection, performs the initial
//! handshake, optionally resumes a session, then publishes the assembled
//! [`AgentReady`] to the caller via a oneshot before holding the connection
//! open until cancellation or child stdout EOF.
//!
//! Unlike [`ChannelPluginRunner`], the agent has **no restart policy**
//! ([`RestartPolicy::Never`]). One bridge = one spawn = one `AgentReady`.
//! The `BridgeFactory` handed to [`Supervisor::register`] is therefore
//! single-shot: we build the bridge eagerly and `take()` it from the
//! `FnMut` factory the first (and only) time the factory is invoked.
//!
//! [`ChannelPluginRunner`]: crate::channels::plugin_runner::ChannelPluginRunner
//! [`RestartPolicy::Never`]: crate::process::RestartPolicy::Never
//! [`Supervisor::register`]: crate::process::Supervisor::register

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use acp::schema::v1 as schema;
use agent_client_protocol as acp;
use anyhow::anyhow;
use tokio::sync::oneshot;
use tokio_util::compat::{TokioAsyncReadCompatExt, TokioAsyncWriteCompatExt};

use crate::process::acp_transport::notifying_stdio_transport;
use crate::process::bridge::{BridgeExit, BridgeFuture, CancelSignal, ProcessBridge, StdioPipes};

use super::runtime::{AcpSessionGeneration, Agent, AgentClientHandler, AgentReady, StartupSession};

struct GenerationGuard(AcpSessionGeneration);

impl Drop for GenerationGuard {
    fn drop(&mut self) {
        self.0.mark_stopped();
    }
}

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
    let suppress_startup_notifications = Arc::new(AtomicBool::new(false));
    let notification_suppression = Arc::clone(&suppress_startup_notifications);
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
                if notification_suppression.load(Ordering::SeqCst) {
                    tracing::debug!(
                        session_id = %args.session_id,
                        "suppressing startup session notification"
                    );
                    return Ok(());
                }
                notification_handler.session_notification(args).await
            },
            acp::on_receive_notification!(),
        )
        .connect_with(transport, async move |conn| {
            let init_req =
                schema::InitializeRequest::new(acp::schema::ProtocolVersion::V1).client_info(
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
                StartupSession::Resume(session_id) | StartupSession::ResumeOnly(session_id) => {
                    let allow_load_fallback = matches!(startup_session, StartupSession::Resume(_));
                    // Keep this on after a successful startup attach. `Agent`
                    // clears it before the first real prompt/new session.
                    suppress_startup_notifications.store(true, Ordering::SeqCst);
                    let resume_result = if initialize
                        .agent_capabilities
                        .session_capabilities
                        .resume
                        .is_none()
                    {
                        tracing::info!(
                            "[{}-agent] session/resume unsupported for {}{}",
                            agent_id_for_run,
                            session_id,
                            if allow_load_fallback {
                                ", trying suppressed session/load"
                            } else {
                                ""
                            }
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
                                Some(session_id.clone())
                            }
                            Err(error) => {
                                tracing::info!(
                                    "[{}-agent] failed to resume session {}{}: {}",
                                    agent_id_for_run,
                                    session_id,
                                    if allow_load_fallback {
                                        ", trying suppressed session/load"
                                    } else {
                                        ""
                                    },
                                    error
                                );
                                None
                            }
                        }
                    };

                    match resume_result {
                        Some(session_id) => Some(session_id),
                        None if allow_load_fallback => {
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
                                    suppress_startup_notifications.store(false, Ordering::SeqCst);
                                    tracing::info!(
                                        "[{}-agent] failed to attach session {}, starting without session: {}",
                                        agent_id_for_run,
                                        session_id,
                                        error
                                    );
                                    None
                                }
                            }
                        }
                        None => {
                            suppress_startup_notifications.store(false, Ordering::SeqCst);
                            None
                        }
                    }
                }
            };

            let agent = Agent::from_connection(
                conn,
                agent_id_for_run.clone(),
                initialize.clone(),
                suppress_startup_notifications,
            );
            // This guard also runs if the ACP driver cancels this callback,
            // so liveness cannot remain stuck at `true` on abnormal exit.
            let _generation_guard = GenerationGuard(agent.generation());
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

#[cfg(test)]
mod lifecycle_tests {
    use super::{AcpSessionGeneration, GenerationGuard};

    #[test]
    fn dropping_bridge_guard_stops_the_shared_generation() {
        let generation = AcpSessionGeneration::running();
        {
            let _guard = GenerationGuard(generation.clone());
            assert!(generation.is_live());
        }
        assert!(!generation.is_live());
    }
}
