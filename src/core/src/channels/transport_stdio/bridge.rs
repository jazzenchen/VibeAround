//! ACP bridge driver for a single stdio plugin.
//!
//! Owns the ACP connection to the child and two concurrent tasks:
//! - The SDK connection driver.
//! - A `ChannelOutput` forwarder that drains `output_rx` and dispatches
//!   each variant to the corresponding ACP Client call.
//!
//! Returns a [`BridgeExit`] describing why the bridge ended. The
//! supervisor observes the exit and drives the state transition
//! (Running → Crashed / Stopped) according to policy + intent.

use std::sync::Arc;

use tokio::sync::mpsc;
use tokio_util::compat::{TokioAsyncReadCompatExt, TokioAsyncWriteCompatExt};

use acp::schema::v1 as schema;
use agent_client_protocol as acp;

use crate::proc_log;
use crate::process::acp_transport::notifying_stdio_transport;
use crate::process::bridge::{BridgeExit, CancelSignal};
use crate::process::registry::ProcessKind;
use crate::workspace::WorkspaceThreadManager;

use super::super::plugin_host::PluginHost;
use super::super::ChannelInput;
use super::forwarder::forward_output_to_plugin;
use super::handler::PluginAgentHandler;
use super::runtime::{QueuedChannelOutput, StdioPluginRuntime};

/// Run the ACP agent-side connection for a plugin to completion. Returns
/// when the child closes stdout or the cancel signal fires.
pub(crate) async fn run_acp_plugin_bridge(
    channel_kind: String,
    config: serde_json::Value,
    stdin: tokio::process::ChildStdin,
    stdout: tokio::process::ChildStdout,
    input_tx: mpsc::UnboundedSender<ChannelInput>,
    mut output_rx: mpsc::UnboundedReceiver<QueuedChannelOutput>,
    workspace_thread_manager: Arc<WorkspaceThreadManager>,
    plugin_host: Arc<PluginHost>,
    runtime: Arc<StdioPluginRuntime>,
    mut cancel: CancelSignal,
) -> BridgeExit {
    // Two clones: one moved into the handler, one moved into the
    // forwarder task, one kept here for `cancel_channel_permissions` at
    // the end of the bridge.
    let forwarder_plugin_host = Arc::clone(&plugin_host);
    let drain_plugin_host = Arc::clone(&plugin_host);
    let handler = Arc::new(PluginAgentHandler::new(
        channel_kind.clone(),
        config.clone(),
        input_tx.clone(),
        workspace_thread_manager,
        plugin_host,
    ));
    let (transport, mut stdio_closed) =
        notifying_stdio_transport(stdin.compat_write(), stdout.compat());

    let init_handler = Arc::clone(&handler);
    let prompt_handler = Arc::clone(&handler);
    let prompt_channel = channel_kind.clone();
    let cancel_handler = Arc::clone(&handler);
    let ext_notification_handler = Arc::clone(&handler);
    let ext_request_handler = Arc::clone(&handler);
    let channel_for_run = channel_kind.clone();

    let run_result = acp::Agent
        .builder()
        .name(format!("{}-plugin-host", channel_kind))
        .on_receive_request(
            async move |args: schema::InitializeRequest, responder, _cx| {
                responder.respond_with_result(init_handler.initialize(args).await)
            },
            acp::on_receive_request!(),
        )
        .on_receive_request(
            async move |args: schema::PromptRequest, responder, cx| {
                let prompt_handler = Arc::clone(&prompt_handler);
                let prompt_channel = prompt_channel.clone();
                cx.spawn(async move {
                    let result = prompt_handler.prompt(args).await;
                    if let Err(error) = responder.respond_with_result(result) {
                        proc_log!(
                            warn,
                            kind = ProcessKind::ChannelPlugin,
                            label = prompt_channel,
                            event = "prompt_response_failed",
                            error = %error
                        );
                    }
                    Ok(())
                })?;
                Ok(())
            },
            acp::on_receive_request!(),
        )
        .on_receive_notification(
            async move |args: schema::CancelNotification, _cx| cancel_handler.cancel(args).await,
            acp::on_receive_notification!(),
        )
        .on_receive_notification(
            async move |notification: schema::ClientNotification, cx| match notification {
                schema::ClientNotification::ExtNotification(ext) => {
                    ext_notification_handler.ext_notification(ext).await?;
                    Ok(acp::Handled::Yes)
                }
                other => Ok(acp::Handled::No {
                    message: (other, cx),
                    retry: false,
                }),
            },
            acp::on_receive_notification!(),
        )
        .on_receive_request(
            async move |request: schema::ClientRequest, responder, _cx| match request {
                schema::ClientRequest::ExtMethodRequest(ext) => {
                    match ext_request_handler.ext_method(ext).await {
                        Ok(raw) => {
                            let value = serde_json::from_str(raw.0.get())
                                .map_err(acp::Error::into_internal_error)?;
                            responder.respond(value)?;
                        }
                        Err(error) => responder.respond_with_error(error)?,
                    }
                    Ok(acp::Handled::Yes)
                }
                other => Ok(acp::Handled::No {
                    message: (other, responder),
                    retry: false,
                }),
            },
            acp::on_receive_request!(),
        )
        .connect_with(transport, async move |conn| {
            let fwd_channel = channel_for_run.clone();
            let forward_conn = conn.clone();
            let forwarder = conn.spawn(async move {
                proc_log!(
                    info,
                    kind = ProcessKind::ChannelPlugin,
                    label = fwd_channel,
                    event = "forwarder_started"
                );
                while let Some(queued) = output_rx.recv().await {
                    let route = queued.output.route_key().clone();
                    let output_id = queued.output_id;
                    let result = forward_output_to_plugin(
                        &forward_conn,
                        &fwd_channel,
                        &forwarder_plugin_host,
                        queued.output,
                    )
                    .await;
                    if let Some(output_id) = output_id {
                        match result {
                            Ok(()) => {
                                if let Err(error) =
                                    forwarder_plugin_host.mark_outbox_sent(&output_id)
                                {
                                    proc_log!(
                                        warn,
                                        kind = ProcessKind::ChannelPlugin,
                                        label = fwd_channel,
                                        event = "outbox_forward_mark_sent_failed",
                                        route = %route,
                                        output_id = %output_id,
                                        error = %error
                                    );
                                }
                            }
                            Err(error) => {
                                if let Err(mark_error) =
                                    forwarder_plugin_host.mark_outbox_nacked(&output_id)
                                {
                                    proc_log!(
                                        warn,
                                        kind = ProcessKind::ChannelPlugin,
                                        label = fwd_channel,
                                        event = "outbox_forward_mark_nacked_failed",
                                        route = %route,
                                        output_id = %output_id,
                                        error = %mark_error
                                    );
                                }
                                proc_log!(
                                    warn,
                                    kind = ProcessKind::ChannelPlugin,
                                    label = fwd_channel,
                                    event = "outbox_forward_failed",
                                    route = %route,
                                    output_id = %output_id,
                                    error = %error
                                );
                            }
                        }
                    }
                }
                proc_log!(
                    info,
                    kind = ProcessKind::ChannelPlugin,
                    label = fwd_channel,
                    event = "forwarder_ended"
                );
                Ok(())
            });
            forwarder?;

            tokio::select! {
                _ = &mut stdio_closed => {
                    proc_log!(
                        info,
                        kind = ProcessKind::ChannelPlugin,
                        label = channel_for_run,
                        event = "io_exited_clean"
                    );
                    Ok(BridgeExit::Clean)
                }
                _ = cancel.wait_for(|v| *v) => {
                    proc_log!(
                        info,
                        kind = ProcessKind::ChannelPlugin,
                        label = channel_for_run,
                        event = "bridge_cancelled"
                    );
                    Ok(BridgeExit::Cancelled)
                }
            }
        })
        .await;

    let exit = match run_result {
        Ok(exit) => exit,
        Err(error) => {
            proc_log!(
                info,
                kind = ProcessKind::ChannelPlugin,
                label = channel_kind,
                event = "io_terminated",
                error = %error
            );
            BridgeExit::ProtocolError(anyhow::anyhow!(error))
        }
    };

    // Drain pending permission senders for this channel — otherwise any
    // `ChannelBridgeHandler::request_permission` caller blocked on a
    // reply from the dying plugin stalls forever. Previously invoked by
    // the old `ChannelMonitor::mark_crashed`; now lives here because the
    // supervisor is protocol-agnostic.
    drain_plugin_host.remove_stdio_runtime_if_current(&channel_kind, &runtime);
    drain_plugin_host.cancel_channel_permissions(&channel_kind);

    exit
}
