//! `PluginHost` — the per-daemon **routing table** for outbound channel
//! traffic, plus a small amount of bridge-adjacent bookkeeping.
//!
//! Three tables, one job each:
//!
//! 1. **`runtimes`** (`DashMap<ChannelKind, PluginRuntime>`) — "which live
//!    sender does a `ChannelOutput` for channel X go through?". The
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
use parking_lot::{Mutex as ParkingMutex, RwLock};
use tokio::sync::{mpsc, oneshot};

use crate::proc_log;
use crate::process::registry::ProcessKind;
use crate::routing::ChannelKind;

use super::monitor::ChannelMonitor;
use super::outbox::ChannelOutbox;
use super::plugin_runtime::PluginRuntime;
use super::transport_stdio::StdioPluginRuntime;
use super::transport_websocket::WebSocketPluginRuntime;
use super::{ChannelInput, ChannelOutput};

pub struct PluginHost {
    runtimes: DashMap<ChannelKind, PluginRuntime>,
    outbox: Arc<ChannelOutbox>,
    input_tx: mpsc::UnboundedSender<ChannelInput>,
    send_locks: DashMap<ChannelKind, Arc<ParkingMutex<()>>>,
    /// Pending `requestPermission` replies keyed by a fresh request_id.
    /// Each entry records every channel kind that received the permission card.
    /// The first valid answer consumes the sender; plugin shutdown removes only
    /// that channel kind and drops the sender once no visible surface can answer.
    pending_permissions: DashMap<String, PendingPermission>,
    /// Back-pointer to the ChannelMonitor. Weak to avoid a reference cycle
    /// (ChannelMonitor holds `Arc<PluginHost>`). Used by the plugin bridge
    /// to call `touch` on `_va/heartbeat`. `mark_crashed` is no longer
    /// needed here — the supervisor observes `BridgeExit` directly.
    monitor: RwLock<Weak<ChannelMonitor>>,
}

impl PluginHost {
    pub fn new(input_tx: mpsc::UnboundedSender<ChannelInput>) -> Self {
        Self {
            runtimes: DashMap::new(),
            outbox: Arc::new(ChannelOutbox::new_default()),
            input_tx,
            send_locks: DashMap::new(),
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

    /// Insert or replace the stdio runtime for a channel kind. Called by
    /// the supervisor's bridge factory on every (re)spawn so `send_output`
    /// always routes to the live process.
    pub fn replace_stdio_runtime(&self, channel_kind: &str, runtime: Arc<StdioPluginRuntime>) {
        let send_lock = self.send_lock(channel_kind);
        let _send_guard = send_lock.lock();
        let plugin_runtime = PluginRuntime::Stdio(Arc::clone(&runtime));
        self.runtimes
            .insert(channel_kind.to_string(), PluginRuntime::Stdio(runtime));
        self.flush_pending(channel_kind, &plugin_runtime);
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

    pub async fn send_output(&self, output: ChannelOutput) {
        let route = output.route_key().clone();
        let runtime = self.runtime_for_channel(&route.channel_kind);

        if matches!(&runtime, Some(PluginRuntime::WebSocket(_))) || route.channel_kind == "web" {
            self.send_direct(output, route, runtime).await;
            return;
        }

        let send_lock = self.send_lock(&route.channel_kind);
        let _send_guard = send_lock.lock();
        let runtime = self.runtime_for_channel(&route.channel_kind);

        let output_id = if should_replay_output(&output) {
            match self.outbox.enqueue_now(output.clone()) {
                Ok(output_id) => output_id,
                Err(error) => {
                    proc_log!(
                        warn,
                        kind = ProcessKind::ChannelPlugin,
                        label = route.channel_kind,
                        event = "outbox_enqueue_failed",
                        route = %route,
                        error = %error
                    );
                    String::new()
                }
            }
        } else {
            String::new()
        };
        proc_log!(
            debug,
            kind = ProcessKind::ChannelPlugin,
            label = route.channel_kind,
            event = "send_output",
            route = %route
        );

        if let Some(runtime) = runtime {
            match runtime.send_output_now(output) {
                Ok(()) if !output_id.is_empty() => {
                    if let Err(error) = self.outbox.mark_sent_now(&output_id) {
                        proc_log!(
                            warn,
                            kind = ProcessKind::ChannelPlugin,
                            label = route.channel_kind,
                            event = "outbox_mark_sent_failed",
                            route = %route,
                            error = %error
                        );
                    }
                }
                Ok(()) => {}
                Err(error) if !output_id.is_empty() => {
                    if let Err(mark_error) = self.outbox.mark_nacked_now(&output_id) {
                        proc_log!(
                            warn,
                            kind = ProcessKind::ChannelPlugin,
                            label = route.channel_kind,
                            event = "outbox_mark_nacked_failed",
                            route = %route,
                            output_id = %output_id,
                            error = %mark_error
                        );
                    }
                    proc_log!(
                        warn,
                        kind = ProcessKind::ChannelPlugin,
                        label = route.channel_kind,
                        event = "outbox_send_failed",
                        route = %route,
                        error = %error
                    );
                }
                Err(error) => {
                    proc_log!(
                        warn,
                        kind = ProcessKind::ChannelPlugin,
                        label = route.channel_kind,
                        event = "send_output_failed",
                        route = %route,
                        error = %error
                    );
                }
            }
        } else {
            let known: Vec<String> = self
                .runtimes
                .iter()
                .map(|e| format!("{:?}", e.key()))
                .collect();
            proc_log!(
                warn,
                kind = ProcessKind::ChannelPlugin,
                label = route.channel_kind,
                event = "no_runtime_for_route",
                route = %route,
                known = ?known
            );
        }
    }

    fn runtime_for_channel(&self, channel_kind: &str) -> Option<PluginRuntime> {
        self.runtimes
            .get(channel_kind)
            .map(|entry| match entry.value() {
                PluginRuntime::Stdio(runtime) => PluginRuntime::Stdio(Arc::clone(runtime)),
                PluginRuntime::WebSocket(runtime) => PluginRuntime::WebSocket(Arc::clone(runtime)),
            })
    }

    fn send_lock(&self, channel_kind: &str) -> Arc<ParkingMutex<()>> {
        self.send_locks
            .entry(channel_kind.to_string())
            .or_insert_with(|| Arc::new(ParkingMutex::new(())))
            .clone()
    }

    async fn send_direct(
        &self,
        output: ChannelOutput,
        route: crate::routing::RouteKey,
        runtime: Option<PluginRuntime>,
    ) {
        proc_log!(
            debug,
            kind = ProcessKind::ChannelPlugin,
            label = route.channel_kind,
            event = "send_output_direct",
            route = %route
        );

        let Some(runtime) = runtime else {
            let known: Vec<String> = self
                .runtimes
                .iter()
                .map(|e| format!("{:?}", e.key()))
                .collect();
            proc_log!(
                warn,
                kind = ProcessKind::ChannelPlugin,
                label = route.channel_kind,
                event = "no_runtime_for_route",
                route = %route,
                known = ?known
            );
            return;
        };

        if let Err(error) = runtime.send_output(output).await {
            proc_log!(
                warn,
                kind = ProcessKind::ChannelPlugin,
                label = route.channel_kind,
                event = "send_output_failed",
                route = %route,
                error = %error
            );
        }
    }

    fn flush_pending(&self, channel_kind: &str, runtime: &PluginRuntime) {
        let pending = self.outbox.pending_for_channel(channel_kind);
        if pending.is_empty() {
            return;
        }
        let channel_kind = channel_kind.to_string();
        for pending in pending {
            match runtime.send_output_now(pending.output) {
                Ok(()) => {
                    if let Err(error) = self.outbox.mark_sent_now(&pending.output_id) {
                        proc_log!(
                            warn,
                            kind = ProcessKind::ChannelPlugin,
                            label = channel_kind,
                            event = "outbox_flush_mark_sent_failed",
                            output_id = %pending.output_id,
                            error = %error
                        );
                    }
                }
                Err(error) => {
                    if let Err(mark_error) = self.outbox.mark_nacked_now(&pending.output_id) {
                        proc_log!(
                            warn,
                            kind = ProcessKind::ChannelPlugin,
                            label = channel_kind,
                            event = "outbox_flush_mark_nacked_failed",
                            output_id = %pending.output_id,
                            error = %mark_error
                        );
                    }
                    proc_log!(
                        warn,
                        kind = ProcessKind::ChannelPlugin,
                        label = channel_kind,
                        event = "outbox_flush_send_failed",
                        output_id = %pending.output_id,
                        error = %error
                    );
                    break;
                }
            }
        }
    }

    pub async fn shutdown_all(&self) {
        let runtimes: Vec<PluginRuntime> = self
            .runtimes
            .iter()
            .map(|entry| match entry.value() {
                PluginRuntime::Stdio(runtime) => PluginRuntime::Stdio(Arc::clone(runtime)),
                PluginRuntime::WebSocket(runtime) => PluginRuntime::WebSocket(Arc::clone(runtime)),
            })
            .collect();

        self.runtimes.clear();
        // Drop every pending oneshot sender so waiting `request_permission`
        // callers in `ChannelBridgeHandler` see `rx.await -> Err` and fall
        // through to `Cancelled` instead of stalling forever.
        self.pending_permissions.clear();

        for runtime in runtimes {
            runtime.shutdown().await;
        }
    }

    /// Drop every pending permission request belonging to `channel_kind`.
    /// Called from `run_acp_plugin_bridge` right before it returns its
    /// `BridgeExit` — guaranteed to fire exactly once per bridge death.
    /// Without this drain, oneshot senders waiting on a reply from the
    /// dying plugin would stall `ChannelBridgeHandler::request_permission`
    /// callers indefinitely.
    pub fn cancel_channel_permissions(&self, channel_kind: &str) {
        let request_ids: Vec<String> = self
            .pending_permissions
            .iter()
            .filter(|entry| entry.value().channel_kinds.contains(channel_kind))
            .map(|entry| entry.key().clone())
            .collect();
        for id in request_ids {
            let mut should_remove = false;
            if let Some(mut pending) = self.pending_permissions.get_mut(&id) {
                pending.channel_kinds.remove(channel_kind);
                should_remove = pending.channel_kinds.is_empty();
            }
            if should_remove {
                self.pending_permissions.remove(&id);
            }
        }
    }

    pub fn register_pending_permission<I>(
        &self,
        request_id: String,
        channel_kinds: I,
        tx: oneshot::Sender<acp::RequestPermissionResponse>,
    ) where
        I: IntoIterator<Item = ChannelKind>,
    {
        self.pending_permissions.insert(
            request_id,
            PendingPermission {
                channel_kinds: channel_kinds.into_iter().collect(),
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
        channel_kind: &str,
        request_id: &str,
        response: acp::RequestPermissionResponse,
    ) -> Result<(), String> {
        let Some((_, pending)) = self
            .pending_permissions
            .remove_if(request_id, |_, pending| {
                pending.channel_kinds.contains(channel_kind)
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
    channel_kinds: HashSet<ChannelKind>,
    tx: oneshot::Sender<acp::RequestPermissionResponse>,
}

fn should_replay_output(output: &ChannelOutput) -> bool {
    matches!(
        output,
        ChannelOutput::SystemText { .. } | ChannelOutput::PermissionRequest { .. }
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::routing::RouteKey;

    fn permission_response() -> acp::RequestPermissionResponse {
        acp::RequestPermissionResponse::new(acp::RequestPermissionOutcome::Cancelled)
    }

    #[test]
    fn replay_outbox_only_keeps_durable_outputs() {
        let route = RouteKey::new("feishu", "chat-a");

        assert!(should_replay_output(&ChannelOutput::SystemText {
            route: route.clone(),
            text: "still useful".to_string(),
            reply_to: None,
        }));
        assert!(should_replay_output(&ChannelOutput::PermissionRequest {
            route: route.clone(),
            request_id: "req-1".to_string(),
            payload: serde_json::json!({}),
        }));
        assert!(!should_replay_output(&ChannelOutput::PromptDone {
            route: route.clone(),
            message_id: None,
        }));
        assert!(!should_replay_output(&ChannelOutput::TurnStatus {
            route,
            active: true,
        }));
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
