//! `PluginHost` — the per-daemon **routing table** for outbound channel
//! traffic, plus a small amount of bridge-adjacent bookkeeping.
//!
//! Three tables, one job each:
//!
//! 1. **`runtimes`** (`DashMap<ChannelInstanceId, PluginRuntime>`) — "which
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
//! 3. **`monitor: Weak<ChannelMonitor>`** — back-pointer used by the ACP
//!    bridge to report `_va/heartbeat` liveness. Weak to avoid a
//!    reference cycle (`ChannelMonitor` holds `Arc<PluginHost>`).
//!
//! `PluginHost` does **not** spawn processes, drive protocols, or own
//! state machines — those are `process::Supervisor`, the bridge threads,
//! and `ChannelMonitor` respectively.

use std::collections::HashSet;
use std::sync::{Arc, Weak};

use agent_client_protocol::schema::v1 as acp;
use dashmap::DashMap;
use parking_lot::RwLock;
use tokio::sync::{mpsc, oneshot};

use crate::proc_log;
use crate::process::registry::ProcessKind;
use crate::routing::{ChannelInstanceId, ChannelKind};

use super::monitor::ChannelMonitor;
use super::plugin_runtime::PluginRuntime;
use super::transport_stdio::StdioPluginRuntime;
use super::transport_websocket::WebSocketPluginRuntime;
use super::{ChannelInput, ChannelOutput};

pub struct PluginHost {
    runtimes: DashMap<ChannelInstanceId, PluginRuntime>,
    input_tx: mpsc::UnboundedSender<ChannelInput>,
    /// Pending `requestPermission` replies keyed by a fresh request_id.
    /// Each entry records every channel instance that received the permission card.
    /// The first valid answer consumes the sender; plugin shutdown removes only
    /// that instance and drops the sender once no visible surface can answer.
    pending_permissions: DashMap<String, PendingPermission>,
    /// Back-pointer to the ChannelMonitor. Weak to avoid a reference cycle
    /// (ChannelMonitor holds `Arc<PluginHost>`). Used by the plugin bridge
    /// to call `touch` on `_va/heartbeat`. `mark_crashed` is no longer
    /// needed here — the supervisor observes `BridgeExit` directly.
    monitor: RwLock<Weak<ChannelMonitor>>,
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
        Self {
            runtimes: DashMap::new(),
            input_tx,
            pending_permissions: DashMap::new(),
            monitor: RwLock::new(Weak::new()),
        }
    }

    /// Called once at daemon boot after both `PluginHost` and `ChannelMonitor`
    /// exist. Establishes the back-pointer so bridge threads can signal the
    /// monitor.
    pub fn set_monitor(&self, monitor: Weak<ChannelMonitor>) {
        *self.monitor.write() = monitor;
    }

    pub fn monitor_weak(&self) -> Weak<ChannelMonitor> {
        self.monitor.read().clone()
    }

    pub fn input_tx(&self) -> mpsc::UnboundedSender<ChannelInput> {
        self.input_tx.clone()
    }

    /// Insert or replace the stdio runtime for a configured channel instance. Called by
    /// the supervisor's bridge factory on every (re)spawn so `send_output`
    /// always routes to the live process.
    pub fn replace_stdio_runtime(&self, instance_id: &str, runtime: Arc<StdioPluginRuntime>) {
        self.runtimes
            .insert(instance_id.to_string(), PluginRuntime::Stdio(runtime));
    }

    pub fn remove_stdio_runtime_if_current(
        &self,
        instance_id: &str,
        runtime: &Arc<StdioPluginRuntime>,
    ) -> bool {
        self.runtimes
            .remove_if(instance_id, |_, current| match current {
                PluginRuntime::Stdio(current) => Arc::ptr_eq(current, runtime),
                PluginRuntime::WebSocket(_) => false,
            })
            .is_some()
    }

    pub fn register_websocket_plugin(
        &self,
        channel_kind: impl Into<ChannelKind>,
        outbound_tx: mpsc::UnboundedSender<ChannelOutput>,
    ) {
        let channel_kind = channel_kind.into();
        let runtime = WebSocketPluginRuntime::new(channel_kind.clone(), outbound_tx);
        self.runtimes
            .insert(channel_kind.clone(), PluginRuntime::WebSocket(runtime));
    }

    /// Best-effort realtime delivery. This method never waits for a plugin:
    /// a full per-generation transport buffer drops the output and logs it.
    pub fn send_output(&self, output: ChannelOutput) {
        let route = output.route_key().clone();
        let instance_id = route.channel_instance_id().to_string();
        let permission_request_id = match &output {
            ChannelOutput::PermissionRequest { request_id, .. } => Some(request_id.clone()),
            _ => None,
        };
        let runtime = self.runtime_for_instance(&instance_id);
        proc_log!(
            debug,
            kind = ProcessKind::ChannelPlugin,
            label = instance_id,
            event = "send_output_live",
            route = %route
        );

        if let Some(runtime) = runtime {
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
            let known: Vec<String> = self
                .runtimes
                .iter()
                .map(|e| format!("{:?}", e.key()))
                .collect();
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

    fn runtime_for_instance(&self, instance_id: &str) -> Option<PluginRuntime> {
        self.runtimes
            .get(instance_id)
            .map(|entry| match entry.value() {
                PluginRuntime::Stdio(runtime) => PluginRuntime::Stdio(Arc::clone(runtime)),
                PluginRuntime::WebSocket(runtime) => PluginRuntime::WebSocket(Arc::clone(runtime)),
            })
    }

    pub fn shutdown_all(&self) {
        self.runtimes.clear();
        // Drop every pending oneshot sender so waiting `request_permission`
        // callers in `ChannelBridgeHandler` see `rx.await -> Err` and fall
        // through to `Cancelled` instead of stalling forever.
        self.pending_permissions.clear();
    }

    /// Drop every pending permission request belonging to `instance_id`.
    /// Called from `run_acp_plugin_bridge` right before it returns its
    /// `BridgeExit` — guaranteed to fire exactly once per bridge death.
    /// Without this drain, oneshot senders waiting on a reply from the
    /// dying plugin would stall `ChannelBridgeHandler::request_permission`
    /// callers indefinitely.
    pub fn cancel_channel_permissions(&self, instance_id: &str) {
        let request_ids: Vec<String> = self
            .pending_permissions
            .iter()
            .filter(|entry| entry.value().channel_instances.contains(instance_id))
            .map(|entry| entry.key().clone())
            .collect();
        for id in request_ids {
            let mut should_remove = false;
            if let Some(mut pending) = self.pending_permissions.get_mut(&id) {
                pending.channel_instances.remove(instance_id);
                should_remove = pending.channel_instances.is_empty();
            }
            if should_remove {
                self.pending_permissions.remove(&id);
            }
        }
    }

    fn cancel_permission_surface(&self, request_id: &str, instance_id: &str) {
        let mut should_remove = false;
        if let Some(mut pending) = self.pending_permissions.get_mut(request_id) {
            pending.channel_instances.remove(instance_id);
            should_remove = pending.channel_instances.is_empty();
        }
        if should_remove {
            self.pending_permissions.remove(request_id);
        }
    }

    pub fn register_pending_permission<I>(
        &self,
        request_id: String,
        channel_instances: I,
        tx: oneshot::Sender<acp::RequestPermissionResponse>,
    ) where
        I: IntoIterator<Item = ChannelInstanceId>,
    {
        self.pending_permissions.insert(
            request_id,
            PendingPermission {
                channel_instances: channel_instances.into_iter().collect(),
                tx,
            },
        );
    }

    pub fn remove_pending_permission(&self, request_id: &str) {
        self.pending_permissions.remove(request_id);
    }

    /// Resolve a pending permission request from an in-process client such as
    /// the web chat channel. Stdio plugins answer through ACP in
    /// `transport_stdio::forwarder`; websocket channels need this small
    /// bridge back into the same pending-permission table.
    pub fn respond_permission(
        &self,
        channel_instance_id: &str,
        request_id: &str,
        response: acp::RequestPermissionResponse,
    ) -> Result<(), String> {
        let Some((_, pending)) = self
            .pending_permissions
            .remove_if(request_id, |_, pending| {
                pending.channel_instances.contains(channel_instance_id)
            })
        else {
            if self.pending_permissions.contains_key(request_id) {
                return Err("permission request belongs to a different channel".to_string());
            }
            return Err("permission request is no longer pending".to_string());
        };

        pending
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
    use crate::routing::RouteKey;

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
            .is_err());
        assert!(host.pending_permissions.contains_key("req-1"));

        host.respond_permission("web", "req-1", permission_response())
            .unwrap();

        assert!(!host.pending_permissions.contains_key("req-1"));
        assert!(rx.await.is_ok());
    }

    #[test]
    fn disconnected_outputs_are_not_queued_for_replay() {
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

        assert!(!host.pending_permissions.contains_key("req-1"));
        assert!(rx.await.is_err());
    }

    #[tokio::test]
    async fn full_plugin_buffer_cancels_permission_without_blocking_other_instances() {
        let (input_tx, _input_rx) = tokio::sync::mpsc::unbounded_channel();
        let host = PluginHost::new(input_tx);
        let (blocked_tx, mut blocked_rx) = tokio::sync::mpsc::channel(1);
        let (live_tx, mut live_rx) = tokio::sync::mpsc::channel(1);
        host.replace_stdio_runtime(
            "slack-blocked",
            Arc::new(StdioPluginRuntime::new("slack-blocked", blocked_tx)),
        );
        host.replace_stdio_runtime(
            "slack-live",
            Arc::new(StdioPluginRuntime::new("slack-live", live_tx)),
        );

        host.send_output(ChannelOutput::SystemText {
            route: RouteKey::with_actor("slack", "slack-blocked", "chat-a", "bot-a", None),
            text: "fills buffer".to_string(),
            reply_to: None,
        });
        let (permission_tx, permission_rx) = tokio::sync::oneshot::channel();
        host.register_pending_permission(
            "req-full".to_string(),
            vec!["slack-blocked".to_string()],
            permission_tx,
        );
        host.send_output(ChannelOutput::PermissionRequest {
            route: RouteKey::with_actor("slack", "slack-blocked", "chat-a", "bot-a", None),
            reply_to: None,
            request_id: "req-full".to_string(),
            payload: serde_json::json!({}),
        });

        let live_route = RouteKey::with_actor("slack", "slack-live", "chat-b", "bot-b", None);
        host.send_output(ChannelOutput::SystemText {
            route: live_route.clone(),
            text: "still delivered".to_string(),
            reply_to: None,
        });

        assert!(permission_rx.await.is_err());
        assert!(blocked_rx.try_recv().is_ok());
        assert_eq!(live_rx.try_recv().unwrap().route_key(), &live_route);
    }

    #[test]
    fn remove_stdio_runtime_only_removes_current_runtime() {
        let (input_tx, _input_rx) = tokio::sync::mpsc::unbounded_channel();
        let host = PluginHost::new(input_tx);
        let (output_tx, _output_rx) = tokio::sync::mpsc::channel(8);
        let old_runtime = Arc::new(StdioPluginRuntime::new("feishu", output_tx));
        host.replace_stdio_runtime("feishu", Arc::clone(&old_runtime));

        let (output_tx, _output_rx) = tokio::sync::mpsc::channel(8);
        let new_runtime = Arc::new(StdioPluginRuntime::new("feishu", output_tx));
        host.replace_stdio_runtime("feishu", Arc::clone(&new_runtime));

        assert!(!host.remove_stdio_runtime_if_current("feishu", &old_runtime));
        assert!(matches!(
            host.runtime_for_instance("feishu"),
            Some(PluginRuntime::Stdio(runtime)) if Arc::ptr_eq(&runtime, &new_runtime)
        ));

        assert!(host.remove_stdio_runtime_if_current("feishu", &new_runtime));
        assert!(host.runtime_for_instance("feishu").is_none());
    }

    #[tokio::test]
    async fn same_kind_instances_route_to_distinct_runtimes() {
        let (input_tx, _input_rx) = tokio::sync::mpsc::unbounded_channel();
        let host = PluginHost::new(input_tx);
        let (work_tx, mut work_rx) = tokio::sync::mpsc::channel(8);
        let (personal_tx, mut personal_rx) = tokio::sync::mpsc::channel(8);
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

        assert_eq!(work_rx.recv().await.unwrap().route_key(), &work_route);
        assert!(personal_rx.try_recv().is_err());
    }

    #[tokio::test]
    async fn cancel_channel_permissions_keeps_other_surfaces_alive() {
        let (input_tx, _input_rx) = tokio::sync::mpsc::unbounded_channel();
        let host = PluginHost::new(input_tx);
        let (tx, mut rx) = tokio::sync::oneshot::channel();
        host.register_pending_permission(
            "req-1".to_string(),
            vec!["feishu".to_string(), "web".to_string()],
            tx,
        );

        host.cancel_channel_permissions("feishu");

        assert!(host.pending_permissions.contains_key("req-1"));
        assert!(matches!(
            rx.try_recv(),
            Err(tokio::sync::oneshot::error::TryRecvError::Empty)
        ));
        host.respond_permission("web", "req-1", permission_response())
            .unwrap();
        assert!(rx.await.is_ok());

        let (tx, rx) = tokio::sync::oneshot::channel();
        host.register_pending_permission("req-2".to_string(), vec!["feishu".to_string()], tx);

        host.cancel_channel_permissions("feishu");

        assert!(!host.pending_permissions.contains_key("req-2"));
        assert!(rx.await.is_err());
    }
}
