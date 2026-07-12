//! ACP bridge driver for a single stdio plugin.
//!
//! Owns the ACP connection to the child and two concurrent tasks:
//! - The SDK connection driver.
//! - A `ChannelOutput` forwarder that drains `output_rx` and dispatches
//!   each variant to the corresponding ACP Client call.
//!
//! Returns a [`BridgeExit`] describing why the bridge ended. The
//! supervisor observes the exit and drives the state transition
//! (Running → Crashed / Stopped) according to state + restart policy.

use std::sync::Arc;

use tokio_util::compat::{TokioAsyncReadCompatExt, TokioAsyncWriteCompatExt};

use acp::schema::v1 as schema;
use agent_client_protocol as acp;

use super::super::plugin_host::PluginHost;
use super::super::plugin_runner::ChannelPluginRunner;
use super::forwarder::forward_output_to_plugin;
use super::handler::PluginAgentHandler;
use crate::proc_log;
use crate::process::acp_transport::notifying_stdio_transport;
use crate::process::bridge::{BridgeExit, CancelSignal};
use crate::process::registry::ProcessKind;

/// Run the ACP agent-side connection for a plugin to completion. Returns
/// when the child closes stdout or the cancel signal fires.
pub(crate) async fn run_acp_plugin_bridge(
    runner: ChannelPluginRunner,
    stdin: tokio::process::ChildStdin,
    stdout: tokio::process::ChildStdout,
    mut cancel: CancelSignal,
) -> BridgeExit {
    let ChannelPluginRunner {
        channel_kind,
        instance_id,
        actor_id,
        raw_config: config,
        input_tx,
        mut output_rx,
        ingress,
        plugin_host,
        runtime,
    } = runner;
    // This guard must outlive every bridge await. Supervisor shutdown may
    // abort this task after its timeout, so function-tail cleanup alone is
    // insufficient.
    let _cleanup = BridgeCleanup::new(
        Arc::clone(&plugin_host),
        instance_id.clone(),
        Arc::clone(&runtime),
    );
    let forwarder_plugin_host = Arc::clone(&plugin_host);
    let handler = Arc::new(PluginAgentHandler::new(
        channel_kind.clone(),
        instance_id.clone(),
        actor_id,
        config.clone(),
        input_tx.clone(),
        ingress,
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
                while let Some(output) = output_rx.recv().await {
                    if requires_independent_forwarding(&output) {
                        let permission_conn = forward_conn.clone();
                        let permission_channel = fwd_channel.clone();
                        let permission_host = Arc::clone(&forwarder_plugin_host);
                        forward_conn.spawn(async move {
                            forward_output(
                                &permission_conn,
                                &permission_channel,
                                &permission_host,
                                output,
                            )
                            .await;
                            Ok(())
                        })?;
                    } else {
                        forward_output(&forward_conn, &fwd_channel, &forwarder_plugin_host, output)
                            .await;
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

    exit
}

struct BridgeCleanup {
    plugin_host: Arc<PluginHost>,
    instance_id: String,
    runtime: Arc<super::StdioPluginRuntime>,
}

impl BridgeCleanup {
    fn new(
        plugin_host: Arc<PluginHost>,
        instance_id: String,
        runtime: Arc<super::StdioPluginRuntime>,
    ) -> Self {
        Self {
            plugin_host,
            instance_id,
            runtime,
        }
    }
}

impl Drop for BridgeCleanup {
    fn drop(&mut self) {
        self.plugin_host
            .remove_stdio_runtime_if_current(&self.instance_id, &self.runtime);
        self.plugin_host
            .cancel_channel_permissions(&self.instance_id);
    }
}

fn requires_independent_forwarding(output: &super::super::ChannelOutput) -> bool {
    matches!(
        output,
        super::super::ChannelOutput::PermissionRequest { .. }
    )
}

async fn forward_output(
    conn: &acp::ConnectionTo<acp::Client>,
    channel_kind: &str,
    plugin_host: &Arc<PluginHost>,
    output: super::super::ChannelOutput,
) {
    let route = output.route_key().clone();
    if let Err(error) = forward_output_to_plugin(conn, channel_kind, plugin_host, output).await {
        proc_log!(
            warn,
            kind = ProcessKind::ChannelPlugin,
            label = channel_kind,
            event = "forward_output_failed",
            route = %route,
            error = %error
        );
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use tokio::sync::{mpsc, oneshot};

    use super::{requires_independent_forwarding, BridgeCleanup};
    use crate::channels::plugin_host::PluginHost;
    use crate::channels::transport_stdio::StdioPluginRuntime;
    use crate::channels::ChannelOutput;
    use crate::routing::RouteKey;

    #[test]
    fn permission_wait_does_not_use_the_serial_output_lane() {
        let route = RouteKey::new("feishu", "chat-a");
        assert!(requires_independent_forwarding(
            &ChannelOutput::PermissionRequest {
                route: route.clone(),
                reply_to: None,
                request_id: "permission-a".to_string(),
                payload: serde_json::json!({}),
            }
        ));
        assert!(!requires_independent_forwarding(
            &ChannelOutput::SystemText {
                route,
                text: "still flows".to_string(),
                reply_to: None,
            }
        ));
    }

    #[tokio::test]
    async fn bridge_cleanup_drains_permissions_without_removing_replacement_runtime() {
        let (input_tx, _input_rx) = mpsc::unbounded_channel();
        let host = Arc::new(PluginHost::new(input_tx));
        let (old_tx, _old_rx) = mpsc::channel(1);
        let old_runtime = Arc::new(StdioPluginRuntime::new("slack-work", old_tx));
        host.replace_stdio_runtime("slack-work", Arc::clone(&old_runtime));
        let cleanup = BridgeCleanup::new(Arc::clone(&host), "slack-work".to_string(), old_runtime);

        let (permission_tx, mut permission_rx) = oneshot::channel();
        host.register_pending_permission(
            "permission-1".to_string(),
            ["slack-work".to_string()],
            permission_tx,
        );

        let (new_tx, mut new_rx) = mpsc::channel(1);
        let new_runtime = Arc::new(StdioPluginRuntime::new("slack-work", new_tx));
        host.replace_stdio_runtime("slack-work", new_runtime);

        drop(cleanup);

        assert!(matches!(
            permission_rx.try_recv(),
            Err(oneshot::error::TryRecvError::Closed)
        ));
        let output = ChannelOutput::SystemText {
            route: RouteKey::with_bot_id("slack", "slack-work", "chat-1"),
            text: "still routed".to_string(),
            reply_to: None,
        };
        host.send_output(output.clone());
        assert_eq!(new_rx.recv().await, Some(output));
    }
}
