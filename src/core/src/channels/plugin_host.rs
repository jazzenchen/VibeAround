//! `PluginHost` — the per-daemon **routing table** for outbound channel
//! traffic, plus a small amount of bridge-adjacent bookkeeping.
//!
//! One owner task serializes two tables:
//!
//! 1. **`runtimes`** — "which
//!    live sender does a `ChannelOutput` for configured Bot instance X use?".
//!    supervisor's bridge factory calls [`PluginHost::replace_stdio_runtime`]
//!    on every (re)spawn so the table always points at the live bridge;
//!    `ws_chat` calls [`PluginHost::register_websocket_plugin`] once per
//!    dashboard connection.
//!
//! 2. **`pending_permissions`** — in-flight `requestPermission` replies,
//!    keyed by a fresh `request_id`. The plugin-side forwarder pops from
//!    here when the plugin answers; [`PluginHost::cancel_channel_permissions`]
//!    drains the map when a plugin dies so waiting callers don't stall.
//!
//! A write-once **`monitor: Weak<ChannelMonitor>`** back-pointer is used by the ACP
//!    bridge to report `_va/heartbeat` liveness. Weak to avoid a
//!    reference cycle (`ChannelMonitor` holds `Arc<PluginHost>`).
//!
//! `PluginHost` does **not** spawn processes, drive protocols, or own
//! state machines — those are `process::Supervisor`, the bridge threads,
//! and `ChannelMonitor` respectively.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, OnceLock, Weak};

use agent_client_protocol::schema::v1 as acp;
use tokio::sync::{mpsc, oneshot};

use crate::proc_log;
use crate::process::ProcessKind;
use crate::routing::{ChannelInstanceId, ChannelKind};

use super::monitor::ChannelMonitor;
use super::plugin_runtime::PluginRuntime;
use super::transport_stdio::StdioPluginRuntime;
use super::transport_websocket::WebSocketPluginRuntime;
use super::{ChannelInput, ChannelOutput};

pub struct PluginHost {
    command_tx: mpsc::UnboundedSender<HostCommand>,
    input_tx: mpsc::UnboundedSender<ChannelInput>,
    /// Back-pointer to the ChannelMonitor. Weak to avoid a reference cycle
    /// (ChannelMonitor holds `Arc<PluginHost>`). Used by the plugin bridge
    /// to call `touch` on `_va/heartbeat`. `mark_crashed` is no longer
    /// needed here — the supervisor observes `BridgeExit` directly.
    monitor: OnceLock<Weak<ChannelMonitor>>,
}

enum HostCommand {
    ReplaceRuntime {
        instance_id: ChannelInstanceId,
        runtime: PluginRuntime,
    },
    RemoveStdioRuntime {
        instance_id: ChannelInstanceId,
        runtime: Arc<StdioPluginRuntime>,
    },
    StdioOutputBarrier {
        instance_id: ChannelInstanceId,
        runtime: Arc<StdioPluginRuntime>,
        reply: oneshot::Sender<Result<(), String>>,
    },
    SendOutput(Box<ChannelOutput>),
    RegisterPermission {
        request_id: String,
        channel_instances: HashSet<ChannelInstanceId>,
        tx: oneshot::Sender<acp::RequestPermissionResponse>,
    },
    RemovePermission(String),
    CancelChannelPermissions(ChannelInstanceId),
    RespondPermission {
        channel_instance_id: ChannelInstanceId,
        request_id: String,
        response: acp::RequestPermissionResponse,
        reply: oneshot::Sender<Result<(), String>>,
    },
    Shutdown(oneshot::Sender<()>),
}

struct PluginHostOwner {
    runtimes: HashMap<ChannelInstanceId, PluginRuntime>,
    pending_permissions: HashMap<String, PendingPermission>,
    command_rx: mpsc::UnboundedReceiver<HostCommand>,
}

/// Cancellation-safe ownership of one pending permission entry.
///
/// Dropping the waiting prompt task removes the entry and drops its oneshot
/// sender, so neither host nor subagent turns can leave stale approvals.
pub(crate) struct PendingPermissionRegistration {
    plugin_host: Arc<PluginHost>,
    request_id: String,
}

impl PendingPermissionRegistration {
    pub(crate) fn register(
        plugin_host: &Arc<PluginHost>,
        request_id: String,
        channel_instance_id: String,
        tx: oneshot::Sender<acp::RequestPermissionResponse>,
    ) -> Self {
        plugin_host.register_pending_permission(request_id.clone(), [channel_instance_id], tx);
        Self {
            plugin_host: Arc::clone(plugin_host),
            request_id,
        }
    }
}

impl Drop for PendingPermissionRegistration {
    fn drop(&mut self) {
        self.plugin_host.remove_pending_permission(&self.request_id);
    }
}

impl PluginHost {
    pub fn new(input_tx: mpsc::UnboundedSender<ChannelInput>) -> Self {
        let (command_tx, command_rx) = mpsc::unbounded_channel();
        tokio::spawn(
            PluginHostOwner {
                runtimes: HashMap::new(),
                pending_permissions: HashMap::new(),
                command_rx,
            }
            .run(),
        );
        Self {
            command_tx,
            input_tx,
            monitor: OnceLock::new(),
        }
    }

    /// Called once at daemon boot after both `PluginHost` and `ChannelMonitor`
    /// exist. Establishes the back-pointer so bridge threads can signal the
    /// monitor.
    pub fn set_monitor(&self, monitor: Weak<ChannelMonitor>) {
        let _ = self.monitor.set(monitor);
    }

    pub fn monitor_weak(&self) -> Weak<ChannelMonitor> {
        self.monitor.get().cloned().unwrap_or_default()
    }

    pub fn input_tx(&self) -> mpsc::UnboundedSender<ChannelInput> {
        self.input_tx.clone()
    }

    /// Insert or replace the stdio runtime for a configured channel instance. Called by
    /// the supervisor's bridge factory on every (re)spawn so `send_output`
    /// always routes to the live process.
    pub fn replace_stdio_runtime(&self, instance_id: &str, runtime: Arc<StdioPluginRuntime>) {
        let _ = self.command_tx.send(HostCommand::ReplaceRuntime {
            instance_id: instance_id.to_string(),
            runtime: PluginRuntime::Stdio(runtime),
        });
    }

    pub fn remove_stdio_runtime_if_current(
        &self,
        instance_id: &str,
        runtime: &Arc<StdioPluginRuntime>,
    ) {
        let _ = self.command_tx.send(HostCommand::RemoveStdioRuntime {
            instance_id: instance_id.to_string(),
            runtime: Arc::clone(runtime),
        });
    }

    pub fn register_websocket_plugin(
        &self,
        channel_kind: impl Into<ChannelKind>,
        outbound_tx: mpsc::UnboundedSender<ChannelOutput>,
    ) {
        let channel_kind = channel_kind.into();
        let runtime = WebSocketPluginRuntime::new(channel_kind.clone(), outbound_tx);
        let _ = self.command_tx.send(HostCommand::ReplaceRuntime {
            instance_id: channel_kind,
            runtime: PluginRuntime::WebSocket(runtime),
        });
    }

    /// Enqueue realtime delivery into the current plugin generation's FIFO.
    /// The prompt response waits on a barrier in that same FIFO.
    pub fn send_output(&self, output: ChannelOutput) {
        let _ = self
            .command_tx
            .send(HostCommand::SendOutput(Box::new(output)));
    }

    pub(crate) async fn wait_for_stdio_output(
        &self,
        instance_id: &str,
        runtime: &Arc<StdioPluginRuntime>,
    ) -> Result<(), String> {
        let (reply, done) = oneshot::channel();
        self.command_tx
            .send(HostCommand::StdioOutputBarrier {
                instance_id: instance_id.to_string(),
                runtime: Arc::clone(runtime),
                reply,
            })
            .map_err(|_| "plugin host is shut down".to_string())?;
        done.await
            .unwrap_or_else(|_| Err("ACP plugin output barrier was dropped".to_string()))
    }

    pub async fn shutdown_all(&self) {
        let (reply, done) = oneshot::channel();
        let _ = self.command_tx.send(HostCommand::Shutdown(reply));
        let _ = done.await;
    }

    /// Drop every pending permission request belonging to `instance_id`.
    /// Called from `run_acp_plugin_bridge` right before it returns its
    /// `BridgeExit` — guaranteed to fire exactly once per bridge death.
    /// Without this drain, oneshot senders waiting on a reply from the
    /// dying plugin would stall `ChannelBridgeHandler::request_permission`
    /// callers indefinitely.
    pub fn cancel_channel_permissions(&self, instance_id: &str) {
        let _ = self.command_tx.send(HostCommand::CancelChannelPermissions(
            instance_id.to_string(),
        ));
    }

    pub fn register_pending_permission<I>(
        &self,
        request_id: String,
        channel_instances: I,
        tx: oneshot::Sender<acp::RequestPermissionResponse>,
    ) where
        I: IntoIterator<Item = ChannelInstanceId>,
    {
        let _ = self.command_tx.send(HostCommand::RegisterPermission {
            request_id,
            channel_instances: channel_instances.into_iter().collect(),
            tx,
        });
    }

    pub fn remove_pending_permission(&self, request_id: &str) {
        let _ = self
            .command_tx
            .send(HostCommand::RemovePermission(request_id.to_string()));
    }

    /// Resolve a pending permission request from an in-process client such as
    /// the web chat channel. Stdio plugins answer through ACP in
    /// `transport_stdio::forwarder`; websocket channels need this small
    /// bridge back into the same pending-permission table.
    pub async fn respond_permission(
        &self,
        channel_instance_id: &str,
        request_id: &str,
        response: acp::RequestPermissionResponse,
    ) -> Result<(), String> {
        let (reply, done) = oneshot::channel();
        self.command_tx
            .send(HostCommand::RespondPermission {
                channel_instance_id: channel_instance_id.to_string(),
                request_id: request_id.to_string(),
                response,
                reply,
            })
            .map_err(|_| "plugin host is shut down".to_string())?;
        done.await
            .unwrap_or_else(|_| Err("plugin host is shut down".to_string()))
    }
}

impl PluginHostOwner {
    async fn run(mut self) {
        while let Some(command) = self.command_rx.recv().await {
            match command {
                HostCommand::ReplaceRuntime {
                    instance_id,
                    runtime,
                } => {
                    self.runtimes.insert(instance_id, runtime);
                }
                HostCommand::RemoveStdioRuntime {
                    instance_id,
                    runtime,
                } => {
                    if matches!(
                        self.runtimes.get(&instance_id),
                        Some(PluginRuntime::Stdio(current)) if Arc::ptr_eq(current, &runtime)
                    ) {
                        self.runtimes.remove(&instance_id);
                    }
                }
                HostCommand::StdioOutputBarrier {
                    instance_id,
                    runtime,
                    reply,
                } => {
                    if matches!(
                        self.runtimes.get(&instance_id),
                        Some(PluginRuntime::Stdio(current)) if Arc::ptr_eq(current, &runtime)
                    ) {
                        runtime.enqueue_barrier(reply);
                    } else {
                        let _ =
                            reply
                                .send(Err("ACP plugin runtime changed before the output barrier"
                                    .to_string()));
                    }
                }
                HostCommand::SendOutput(output) => self.send_output(*output),
                HostCommand::RegisterPermission {
                    request_id,
                    channel_instances,
                    tx,
                } => {
                    self.pending_permissions.insert(
                        request_id,
                        PendingPermission {
                            channel_instances,
                            tx,
                        },
                    );
                }
                HostCommand::RemovePermission(request_id) => {
                    self.pending_permissions.remove(&request_id);
                }
                HostCommand::CancelChannelPermissions(instance_id) => {
                    self.cancel_channel_permissions(&instance_id);
                }
                HostCommand::RespondPermission {
                    channel_instance_id,
                    request_id,
                    response,
                    reply,
                } => {
                    let result =
                        self.respond_permission(&channel_instance_id, &request_id, response);
                    let _ = reply.send(result);
                }
                HostCommand::Shutdown(reply) => {
                    self.runtimes.clear();
                    self.pending_permissions.clear();
                    let _ = reply.send(());
                }
            }
        }
    }

    fn send_output(&mut self, output: ChannelOutput) {
        let route = output.route_key().clone();
        let instance_id = route.channel_instance_id().to_string();
        let permission_request_id = match &output {
            ChannelOutput::PermissionRequest { request_id, .. } => Some(request_id.clone()),
            _ => None,
        };
        proc_log!(
            debug,
            kind = ProcessKind::ChannelPlugin,
            label = instance_id,
            event = "send_output_live",
            route = %route
        );

        if let Some(runtime) = self.runtimes.get(&instance_id) {
            if let Err(error) = runtime.send_output_now(output) {
                if let Some(request_id) = permission_request_id.as_deref() {
                    self.cancel_permission_surface(request_id, &instance_id);
                }
                proc_log!(
                    warn,
                    kind = ProcessKind::ChannelPlugin,
                    label = instance_id,
                    event = "send_output_failed",
                    route = %route,
                    error = %error
                );
            }
        } else {
            if let Some(request_id) = permission_request_id.as_deref() {
                self.cancel_permission_surface(request_id, &instance_id);
            }
            let known = self.runtimes.keys().cloned().collect::<Vec<_>>();
            proc_log!(
                warn,
                kind = ProcessKind::ChannelPlugin,
                label = instance_id,
                event = "no_runtime_for_route",
                route = %route,
                known = ?known
            );
        }
    }

    fn cancel_channel_permissions(&mut self, instance_id: &str) {
        self.pending_permissions.retain(|_, pending| {
            pending.channel_instances.remove(instance_id);
            !pending.channel_instances.is_empty()
        });
    }

    fn cancel_permission_surface(&mut self, request_id: &str, instance_id: &str) {
        let should_remove = self
            .pending_permissions
            .get_mut(request_id)
            .is_some_and(|pending| {
                pending.channel_instances.remove(instance_id);
                pending.channel_instances.is_empty()
            });
        if should_remove {
            self.pending_permissions.remove(request_id);
        }
    }

    fn respond_permission(
        &mut self,
        channel_instance_id: &str,
        request_id: &str,
        response: acp::RequestPermissionResponse,
    ) -> Result<(), String> {
        let Some(pending) = self.pending_permissions.get(request_id) else {
            return Err("permission request is no longer pending".to_string());
        };
        if !pending.channel_instances.contains(channel_instance_id) {
            return Err("permission request belongs to a different channel".to_string());
        }
        self.pending_permissions
            .remove(request_id)
            .expect("permission checked above")
            .tx
            .send(response)
            .map_err(|_| "permission requester is no longer listening".to_string())
    }
}

struct PendingPermission {
    channel_instances: HashSet<ChannelInstanceId>,
    tx: oneshot::Sender<acp::RequestPermissionResponse>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::channels::transport_stdio::StdioBridgeMessage;
    use crate::routing::RouteKey;

    async fn recv_stdio_output(
        rx: &mut tokio::sync::mpsc::UnboundedReceiver<StdioBridgeMessage>,
    ) -> ChannelOutput {
        let Some(StdioBridgeMessage::Output(output)) = rx.recv().await else {
            panic!("expected stdio output");
        };
        output
    }

    fn permission_response() -> acp::RequestPermissionResponse {
        acp::RequestPermissionResponse::new(acp::RequestPermissionOutcome::Cancelled)
    }

    #[tokio::test]
    async fn pending_permission_accepts_any_registered_channel() {
        let (input_tx, _input_rx) = tokio::sync::mpsc::unbounded_channel();
        let host = PluginHost::new(input_tx);
        let (tx, rx) = tokio::sync::oneshot::channel();
        host.register_pending_permission(
            "req-1".to_string(),
            vec!["feishu".to_string(), "web".to_string()],
            tx,
        );

        assert!(host
            .respond_permission("slack", "req-1", permission_response())
            .await
            .is_err());

        host.respond_permission("web", "req-1", permission_response())
            .await
            .unwrap();

        assert!(rx.await.is_ok());
    }

    #[tokio::test]
    async fn disconnected_outputs_are_not_queued_for_replay() {
        let (input_tx, _input_rx) = tokio::sync::mpsc::unbounded_channel();
        let host = PluginHost::new(input_tx);

        host.send_output(ChannelOutput::SystemText {
            route: RouteKey::new("feishu", "chat-a"),
            text: "hello".to_string(),
            reply_to: None,
        });
    }

    #[tokio::test]
    async fn disconnected_permission_delivery_cancels_the_waiter() {
        let (input_tx, _input_rx) = tokio::sync::mpsc::unbounded_channel();
        let host = PluginHost::new(input_tx);
        let (tx, rx) = tokio::sync::oneshot::channel();
        host.register_pending_permission("req-1".to_string(), vec!["feishu".to_string()], tx);

        host.send_output(ChannelOutput::PermissionRequest {
            route: RouteKey::new("feishu", "chat-a"),
            reply_to: None,
            request_id: "req-1".to_string(),
            payload: serde_json::json!({}),
        });

        assert!(rx.await.is_err());
    }

    #[tokio::test]
    async fn queued_permission_is_delivered_without_cancellation() {
        let (input_tx, _input_rx) = tokio::sync::mpsc::unbounded_channel();
        let host = PluginHost::new(input_tx);
        let (output_tx, mut output_rx) = tokio::sync::mpsc::unbounded_channel();
        host.replace_stdio_runtime(
            "slack-work",
            Arc::new(StdioPluginRuntime::new("slack-work", output_tx)),
        );

        host.send_output(ChannelOutput::SystemText {
            route: RouteKey::with_actor("slack", "slack-work", "chat-a", "bot-a", None),
            text: "first".to_string(),
            reply_to: None,
        });
        let (permission_tx, permission_rx) = tokio::sync::oneshot::channel();
        host.register_pending_permission(
            "req-queued".to_string(),
            vec!["slack-work".to_string()],
            permission_tx,
        );
        host.send_output(ChannelOutput::PermissionRequest {
            route: RouteKey::with_actor("slack", "slack-work", "chat-a", "bot-a", None),
            reply_to: None,
            request_id: "req-queued".to_string(),
            payload: serde_json::json!({}),
        });

        let _ = recv_stdio_output(&mut output_rx).await;
        let permission = recv_stdio_output(&mut output_rx).await;
        assert!(matches!(
            permission,
            ChannelOutput::PermissionRequest { .. }
        ));
        host.respond_permission("slack-work", "req-queued", permission_response())
            .await
            .unwrap();
        assert!(permission_rx.await.is_ok());
    }

    #[tokio::test]
    async fn remove_stdio_runtime_only_removes_current_runtime() {
        let (input_tx, _input_rx) = tokio::sync::mpsc::unbounded_channel();
        let host = PluginHost::new(input_tx);
        let (output_tx, _output_rx) = tokio::sync::mpsc::unbounded_channel();
        let old_runtime = Arc::new(StdioPluginRuntime::new("feishu", output_tx));
        host.replace_stdio_runtime("feishu", Arc::clone(&old_runtime));

        let (output_tx, mut output_rx) = tokio::sync::mpsc::unbounded_channel();
        let new_runtime = Arc::new(StdioPluginRuntime::new("feishu", output_tx));
        host.replace_stdio_runtime("feishu", Arc::clone(&new_runtime));

        host.remove_stdio_runtime_if_current("feishu", &old_runtime);
        host.send_output(ChannelOutput::SystemText {
            route: RouteKey::new("feishu", "chat-a"),
            text: "still live".to_string(),
            reply_to: None,
        });
        let _ = recv_stdio_output(&mut output_rx).await;

        host.remove_stdio_runtime_if_current("feishu", &new_runtime);
        let (permission_tx, permission_rx) = oneshot::channel();
        host.register_pending_permission(
            "req-removed".to_string(),
            ["feishu".to_string()],
            permission_tx,
        );
        host.send_output(ChannelOutput::PermissionRequest {
            route: RouteKey::new("feishu", "chat-a"),
            reply_to: None,
            request_id: "req-removed".to_string(),
            payload: serde_json::json!({}),
        });
        assert!(permission_rx.await.is_err());
    }

    #[tokio::test]
    async fn same_kind_instances_route_to_distinct_runtimes() {
        let (input_tx, _input_rx) = tokio::sync::mpsc::unbounded_channel();
        let host = PluginHost::new(input_tx);
        let (work_tx, mut work_rx) = tokio::sync::mpsc::unbounded_channel();
        let (personal_tx, mut personal_rx) = tokio::sync::mpsc::unbounded_channel();
        host.replace_stdio_runtime(
            "slack-work",
            Arc::new(StdioPluginRuntime::new("slack-work", work_tx)),
        );
        host.replace_stdio_runtime(
            "slack-personal",
            Arc::new(StdioPluginRuntime::new("slack-personal", personal_tx)),
        );

        let work_route = RouteKey::with_actor("slack", "slack-work", "chat-a", "U-WORK", None);
        host.send_output(ChannelOutput::SystemText {
            route: work_route.clone(),
            text: "hello work".to_string(),
            reply_to: None,
        });

        assert_eq!(
            recv_stdio_output(&mut work_rx).await.route_key(),
            &work_route
        );
        assert!(personal_rx.try_recv().is_err());
    }

    #[tokio::test]
    async fn runtime_barrier_does_not_block_other_instances() {
        let (input_tx, _input_rx) = tokio::sync::mpsc::unbounded_channel();
        let host = Arc::new(PluginHost::new(input_tx));
        let (blocked_tx, mut blocked_rx) = tokio::sync::mpsc::unbounded_channel();
        let blocked_runtime = Arc::new(StdioPluginRuntime::new("slack-blocked", blocked_tx));
        let (live_tx, mut live_rx) = tokio::sync::mpsc::unbounded_channel();
        host.replace_stdio_runtime("slack-blocked", Arc::clone(&blocked_runtime));
        host.replace_stdio_runtime(
            "slack-live",
            Arc::new(StdioPluginRuntime::new("slack-live", live_tx)),
        );
        host.send_output(ChannelOutput::SystemText {
            route: RouteKey::with_channel_instance("slack", "slack-blocked", "chat-a"),
            text: "fills buffer".to_string(),
            reply_to: None,
        });

        let barrier_host = Arc::clone(&host);
        let barrier_runtime = Arc::clone(&blocked_runtime);
        let barrier = tokio::spawn(async move {
            barrier_host
                .wait_for_stdio_output("slack-blocked", &barrier_runtime)
                .await
        });
        host.send_output(ChannelOutput::SystemText {
            route: RouteKey::with_channel_instance("slack", "slack-live", "chat-b"),
            text: "still delivered".to_string(),
            reply_to: None,
        });

        let live_output = tokio::time::timeout(
            std::time::Duration::from_millis(100),
            recv_stdio_output(&mut live_rx),
        )
        .await
        .expect("barrier blocked the plugin host owner");
        assert_eq!(live_output.route_key().channel_instance_id(), "slack-live");
        let _ = recv_stdio_output(&mut blocked_rx).await;
        let Some(StdioBridgeMessage::Barrier(reply)) = blocked_rx.recv().await else {
            panic!("expected barrier");
        };
        reply.send(Ok(())).unwrap();
        assert_eq!(barrier.await.unwrap(), Ok(()));
    }

    #[tokio::test]
    async fn cancel_channel_permissions_keeps_other_surfaces_alive() {
        let (input_tx, _input_rx) = tokio::sync::mpsc::unbounded_channel();
        let host = PluginHost::new(input_tx);
        let (tx, rx) = tokio::sync::oneshot::channel();
        host.register_pending_permission(
            "req-1".to_string(),
            vec!["feishu".to_string(), "web".to_string()],
            tx,
        );

        host.cancel_channel_permissions("feishu");

        host.respond_permission("web", "req-1", permission_response())
            .await
            .unwrap();
        assert!(rx.await.is_ok());

        let (tx, rx) = tokio::sync::oneshot::channel();
        host.register_pending_permission("req-2".to_string(), vec!["feishu".to_string()], tx);

        host.cancel_channel_permissions("feishu");

        assert!(rx.await.is_err());
    }
}
