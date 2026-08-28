//! `ChannelBridgeHandler` — the downstream handler wired to the upstream
//! `Agent`. Its two jobs:
//!
//! 1. **`session_notification`** — wrap every `acp::SessionNotification`
//!    from the agent as a workspace-thread reply, then fan it out to attached
//!    channel routes.
//! 2. **`request_permission`** — turn an ACP `requestPermission` call from
//!    the upstream agent into a `ChannelOutput::PermissionRequest` to the
//!    plugin, then await the plugin's reply via a per-request oneshot
//!    registered in `PluginHost::pending_permissions`. No timeout — the UX
//!    is "user takes as long as they need".

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, Weak};

use agent_client_protocol::schema::v1 as acp;
use tokio::sync::{mpsc, oneshot};

use crate::agent::AgentClientHandler;
use crate::routing::{channel_traits, ActiveTurnTarget, ChannelTarget, RouteKey};
use crate::workspace::registry::WorkspaceId;
use crate::workspace::threads::store::{HostBinding, WorkspaceThreadId};
use crate::workspace::threads::{ThreadAgent, ThreadAgentId};
use crate::workspace::WorkspaceThreadManager;

use super::agent_protocol::{
    notification_payload, notification_payload_with_text, session_update_text,
    synthetic_agent_message_payload, synthetic_user_message_payload, AgentProtocolFilter,
};
#[cfg(test)]
use super::plugin_host::PendingPermissionRegistration;
use super::plugin_host::PluginHost;
use super::types::{ChannelOutput, ThreadReply, ThreadReplyAgent, ThreadReplyPayload};

/// Collects the transcript a `session/load` startup replays, so the start
/// path can hand it to the one connection that asked for it instead of
/// broadcasting it. Deactivated (and drained) once the start completes;
/// afterwards notifications flow through the normal fan-out again.
pub(crate) struct StartupReplayCapture {
    active: AtomicBool,
    frames: Mutex<Vec<ThreadReply>>,
}

impl StartupReplayCapture {
    pub(crate) fn new() -> Self {
        Self {
            active: AtomicBool::new(true),
            frames: Mutex::new(Vec::new()),
        }
    }

    /// Store a frame while the capture is live. Returns whether it was taken.
    fn push(&self, reply: ThreadReply) -> bool {
        if !self.active.load(Ordering::SeqCst) {
            return false;
        }
        let mut frames = self.frames.lock().unwrap_or_else(|poisoned| {
            self.active.store(false, Ordering::SeqCst);
            poisoned.into_inner()
        });
        frames.push(reply);
        true
    }

    /// Stop capturing and hand back everything collected so far.
    pub(crate) fn finish(&self) -> Vec<ThreadReply> {
        self.active.store(false, Ordering::SeqCst);
        std::mem::take(
            &mut *self
                .frames
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner()),
        )
    }
}

pub(crate) struct ChannelBridgeHandler {
    plugin_host: Arc<PluginHost>,
    workspace_threads: Weak<WorkspaceThreadManager>,
    workspace_id: WorkspaceId,
    thread_id: WorkspaceThreadId,
    host_binding: HostBinding,
    active_turn_target: ActiveTurnTarget,
    host_protocol: HostProtocolOwner,
    startup_capture: Option<Arc<StartupReplayCapture>>,
}

impl ChannelBridgeHandler {
    pub(crate) fn for_thread_with_capture(
        plugin_host: Arc<PluginHost>,
        workspace_threads: &Arc<WorkspaceThreadManager>,
        workspace_id: WorkspaceId,
        thread_id: WorkspaceThreadId,
        host_binding: HostBinding,
        active_turn_target: ActiveTurnTarget,
        startup_capture: Option<Arc<StartupReplayCapture>>,
    ) -> Self {
        Self {
            plugin_host,
            workspace_threads: Arc::downgrade(workspace_threads),
            workspace_id,
            thread_id,
            host_binding,
            active_turn_target,
            host_protocol: HostProtocolOwner::new(),
            startup_capture,
        }
    }

    /// Divert a transcript frame into the startup capture while one is live.
    fn capture_startup_frame(&self, reply: &ThreadReply) -> bool {
        self.startup_capture
            .as_ref()
            .is_some_and(|capture| capture.push(reply.clone()))
    }

    async fn attached_routes(&self) -> Vec<RouteKey> {
        let Some(workspace_threads) = self.workspace_threads.upgrade() else {
            return Vec::new();
        };
        match workspace_threads
            .attached_routes_for_thread(&self.thread_id)
            .await
        {
            Ok(routes) => routes,
            Err(error) => {
                tracing::warn!(
                    "[ChannelBridgeHandler] failed to resolve attached routes thread={}: {:#}",
                    self.thread_id,
                    error
                );
                Vec::new()
            }
        }
    }

    async fn attached_rich_agent_event_routes(&self) -> Vec<RouteKey> {
        self.attached_routes()
            .await
            .into_iter()
            .filter(|route| channel_traits(&route.channel_kind).rich_agent_events)
            .collect()
    }

    fn active_turn_target(&self) -> Option<ChannelTarget> {
        self.active_turn_target.current()
    }

    async fn attached_delivery_targets(&self) -> Vec<ChannelTarget> {
        let origin = self.active_turn_target();
        delivery_targets(self.attached_routes().await, origin)
    }

    async fn filter_host_protocol_notification(
        &self,
        args: &acp::SessionNotification,
    ) -> acp::Result<Option<serde_json::Value>> {
        let Some(text) = session_update_text(&args.update) else {
            return notification_payload(args).map(Some);
        };
        let visible = self
            .host_protocol
            .feed(args.session_id.to_string(), text.to_string())
            .await;
        if visible.is_empty() {
            return Ok(None);
        }
        notification_payload_with_text(args, visible).map(Some)
    }

    async fn finish_host_protocol(&self, success: bool) {
        let (session_id, finished) = self.host_protocol.finish().await;

        if let Some(session_id) = session_id.as_deref() {
            self.send_host_visible_text_chunk(session_id, &finished.visible_tail)
                .await;
        }
        if !success {
            return;
        }
        let Some(frame) = finished.frame else {
            return;
        };
        let envelope = match frame {
            Ok(envelope) => envelope,
            Err(error) => {
                self.send_system_text(&format!("Subagent assignment ignored: {}", error))
                    .await;
                return;
            }
        };
        self.dispatch_host_protocol_envelope(&envelope).await;
    }

    async fn dispatch_host_protocol_envelope(&self, envelope: &str) {
        let assignment = match HostAssignment::parse(envelope) {
            Ok(Some(assignment)) => assignment,
            Ok(None) => return,
            Err(error) => {
                self.send_system_text(&format!("Subagent assignment ignored: {}", error))
                    .await;
                return;
            }
        };
        let Some(workspace_threads) = self.workspace_threads.upgrade() else {
            self.send_system_text("Subagent assignment ignored: thread manager is unavailable.")
                .await;
            return;
        };
        let runtime = match workspace_threads
            .runtime_for_thread_id(&self.thread_id)
            .await
        {
            Ok(runtime) => runtime,
            Err(error) => {
                self.send_system_text(&format!(
                    "Subagent assignment ignored: failed to load thread runtime: {:#}",
                    error
                ))
                .await;
                return;
            }
        };
        let status_tx = self.spawn_subagent_status_forwarder();
        let to_agent_id = assignment.to_agent_id.clone();
        let task = assignment.task.clone();
        let Some(target) = self.active_turn_target() else {
            self.send_system_text("Subagent assignment ignored: host turn is no longer active.")
                .await;
            return;
        };
        if let Err(error) = runtime
            .prompt_subagent_assignment(&to_agent_id, assignment.payload, target, status_tx)
            .await
        {
            self.send_system_text(&format!(
                "Subagent assignment ignored for {}: {}",
                to_agent_id, error.message
            ))
            .await;
        } else {
            self.send_subagent_assignment_visible(&to_agent_id, &task)
                .await;
        }
    }

    async fn send_host_visible_text_chunk(&self, session_id: &str, text: &str) {
        if text.is_empty() {
            return;
        }
        let payload = synthetic_agent_message_payload(session_id, text.to_string());
        let reply = ThreadReply {
            workspace_id: self.workspace_id.to_string(),
            thread_id: self.thread_id.to_string(),
            agent: ThreadReplyAgent {
                id: self.host_binding.agent_id.clone(),
                profile: self.host_binding.profile_id.clone(),
                session_id: session_id.to_string(),
            },
            payload: ThreadReplyPayload::AcpSessionNotification {
                notification: payload,
            },
        };
        if self.capture_startup_frame(&reply) {
            return;
        }

        for target in self.attached_delivery_targets().await {
            self.plugin_host.send_output(ChannelOutput::ThreadReply {
                route: target.route,
                reply_to: target.reply_to,
                reply: reply.clone(),
            });
        }
    }

    async fn send_subagent_assignment_visible(&self, agent_id: &ThreadAgentId, task: &str) {
        let Some(workspace_threads) = self.workspace_threads.upgrade() else {
            return;
        };
        let Ok(runtime) = workspace_threads
            .runtime_for_thread_id(&self.thread_id)
            .await
        else {
            return;
        };
        let Some(agent) = runtime
            .state()
            .await
            .agents
            .into_iter()
            .find(|agent| &agent.id == agent_id)
        else {
            return;
        };
        let text = if task.trim().is_empty() {
            "Host assignment received.".to_string()
        } else {
            format!("Host assignment:\n\n{}", task.trim())
        };
        let payload = synthetic_user_message_payload(&format!("subagent:{}", agent.id), text);
        for route in self.attached_rich_agent_event_routes().await {
            self.plugin_host.send_output(ChannelOutput::SubagentAcp {
                route,
                agent: agent.clone(),
                payload: payload.clone(),
            });
        }
    }

    fn spawn_subagent_status_forwarder(&self) -> mpsc::UnboundedSender<ThreadAgent> {
        let (tx, mut rx) = mpsc::unbounded_channel::<ThreadAgent>();
        let plugin_host = Arc::clone(&self.plugin_host);
        let workspace_threads = self.workspace_threads.clone();
        let thread_id = self.thread_id.clone();
        tokio::spawn(async move {
            while let Some(agent) = rx.recv().await {
                let Some(workspace_threads) = workspace_threads.upgrade() else {
                    continue;
                };
                let routes = match workspace_threads
                    .attached_routes_for_thread(&thread_id)
                    .await
                {
                    Ok(routes) => routes,
                    Err(error) => {
                        tracing::warn!(
                            thread_id = %thread_id,
                            error = %error,
                            "failed to resolve web routes for subagent status"
                        );
                        continue;
                    }
                };
                for route in routes
                    .into_iter()
                    .filter(|route| channel_traits(&route.channel_kind).rich_agent_events)
                {
                    plugin_host.send_output(ChannelOutput::SubagentStatus {
                        route,
                        agent: agent.clone(),
                    });
                }
            }
        });
        tx
    }

    async fn send_system_text(&self, text: &str) {
        let origin = self.active_turn_target();
        for route in self.attached_rich_agent_event_routes().await {
            let reply_to = origin
                .as_ref()
                .filter(|origin| origin.route == route)
                .and_then(|origin| origin.reply_to.clone());
            self.plugin_host.send_output(ChannelOutput::SystemText {
                route,
                text: text.to_string(),
                reply_to,
            });
        }
    }
}

fn delivery_targets(
    mut routes: Vec<RouteKey>,
    origin: Option<ChannelTarget>,
) -> Vec<ChannelTarget> {
    if let Some(origin) = &origin {
        if !routes.contains(&origin.route) {
            routes.push(origin.route.clone());
        }
    }
    routes
        .into_iter()
        .map(|route| {
            let reply_to = origin
                .as_ref()
                .filter(|origin| origin.route == route)
                .and_then(|origin| origin.reply_to.clone());
            ChannelTarget::new(route, reply_to)
        })
        .collect()
}

#[async_trait::async_trait]
impl AgentClientHandler for ChannelBridgeHandler {
    fn mcp_server(&self) -> Option<Arc<dyn crate::agent::AcpMcpServer>> {
        self.workspace_threads.upgrade()?.mcp_over_acp()
    }

    async fn session_notification(&self, args: acp::SessionNotification) -> acp::Result<()> {
        let Some(payload) = self.filter_host_protocol_notification(&args).await? else {
            return Ok(());
        };

        // Log only the update variant. Message content must not be persisted
        // in daemon logs.
        let update_kind = payload
            .get("update")
            .and_then(|u| u.get("sessionUpdate"))
            .and_then(|v| v.as_str())
            .unwrap_or("<none>");
        tracing::info!(
            "[ChannelBridgeHandler] session_notification thread={} session={} kind={}",
            self.thread_id,
            args.session_id,
            update_kind
        );

        let reply = ThreadReply {
            workspace_id: self.workspace_id.to_string(),
            thread_id: self.thread_id.to_string(),
            agent: ThreadReplyAgent {
                id: self.host_binding.agent_id.clone(),
                profile: self.host_binding.profile_id.clone(),
                session_id: args.session_id.to_string(),
            },
            payload: ThreadReplyPayload::AcpSessionNotification {
                notification: payload,
            },
        };
        if self.capture_startup_frame(&reply) {
            return Ok(());
        }

        for target in self.attached_delivery_targets().await {
            self.plugin_host.send_output(ChannelOutput::ThreadReply {
                route: target.route,
                reply_to: target.reply_to,
                reply: reply.clone(),
            });
        }
        Ok(())
    }

    async fn prompt_finished(&self, success: bool) -> acp::Result<()> {
        self.finish_host_protocol(success).await;
        Ok(())
    }

    async fn request_permission(
        &self,
        args: acp::RequestPermissionRequest,
    ) -> acp::Result<acp::RequestPermissionResponse> {
        super::permission::forward_permission_request(
            &self.plugin_host,
            &self.active_turn_target,
            args,
            None,
        )
        .await
    }
}

#[derive(Default)]
struct HostProtocolState {
    filter: AgentProtocolFilter,
    last_session_id: Option<String>,
}

struct HostProtocolOwner {
    command_tx: mpsc::UnboundedSender<HostProtocolCommand>,
}

enum HostProtocolCommand {
    Feed {
        session_id: String,
        text: String,
        reply: oneshot::Sender<String>,
    },
    Finish(oneshot::Sender<(Option<String>, super::agent_protocol::AgentProtocolFinish)>),
}

impl HostProtocolOwner {
    fn new() -> Self {
        let (command_tx, mut command_rx) = mpsc::unbounded_channel();
        tokio::spawn(async move {
            let mut state = HostProtocolState::default();
            while let Some(command) = command_rx.recv().await {
                match command {
                    HostProtocolCommand::Feed {
                        session_id,
                        text,
                        reply,
                    } => {
                        state.last_session_id = Some(session_id);
                        let _ = reply.send(state.filter.feed_text(&text));
                    }
                    HostProtocolCommand::Finish(reply) => {
                        let _ = reply.send((state.last_session_id.clone(), state.filter.finish()));
                    }
                }
            }
        });
        Self { command_tx }
    }

    async fn feed(&self, session_id: String, text: String) -> String {
        let (reply, result) = oneshot::channel();
        self.command_tx
            .send(HostProtocolCommand::Feed {
                session_id,
                text,
                reply,
            })
            .expect("host protocol owner must outlive its handler");
        result.await.expect("host protocol owner must reply")
    }

    async fn finish(&self) -> (Option<String>, super::agent_protocol::AgentProtocolFinish) {
        let (reply, result) = oneshot::channel();
        self.command_tx
            .send(HostProtocolCommand::Finish(reply))
            .expect("host protocol owner must outlive its handler");
        result.await.expect("host protocol owner must reply")
    }
}

struct HostAssignment {
    to_agent_id: ThreadAgentId,
    payload: serde_json::Value,
    task: String,
}

impl HostAssignment {
    fn parse(envelope: &str) -> Result<Option<Self>, String> {
        let payload: serde_json::Value = serde_json::from_str(envelope)
            .map_err(|error| format!("invalid va-agent-protocol JSON: {}", error))?;
        let object = payload
            .as_object()
            .ok_or_else(|| "va-agent-protocol payload must be a JSON object".to_string())?;
        let protocol = string_field(object, "protocol")?;
        if protocol != "va-agent-protocol" {
            return Err(format!(
                "protocol field expected `va-agent-protocol`, got `{}`",
                protocol
            ));
        }
        let kind = string_field(object, "kind")?;
        if kind != "assignment" {
            return Ok(None);
        }
        let to_agent_id = string_field(object, "to_agent_id")?;
        if to_agent_id.trim().is_empty() {
            return Err("assignment field `to_agent_id` must not be empty".to_string());
        }
        Ok(Some(Self {
            to_agent_id: ThreadAgentId::from(to_agent_id),
            task: object
                .get("task")
                .and_then(|value| value.as_str())
                .unwrap_or_default()
                .to_string(),
            payload,
        }))
    }
}

fn string_field(
    object: &serde_json::Map<String, serde_json::Value>,
    field: &str,
) -> Result<String, String> {
    object
        .get(field)
        .and_then(|value| value.as_str())
        .map(ToOwned::to_owned)
        .ok_or_else(|| format!("va-agent-protocol payload missing string field `{}`", field))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn permission_handler(
        plugin_host: Arc<PluginHost>,
        active_turn_target: ActiveTurnTarget,
    ) -> ChannelBridgeHandler {
        ChannelBridgeHandler {
            plugin_host,
            workspace_threads: Weak::new(),
            workspace_id: WorkspaceId::general(),
            thread_id: WorkspaceThreadId::from("thread-1"),
            host_binding: HostBinding::new("codex", None),
            active_turn_target,
            host_protocol: HostProtocolOwner::new(),
            startup_capture: None,
        }
    }

    #[test]
    fn startup_capture_takes_frames_until_finished() {
        let capture = StartupReplayCapture::new();
        let reply = ThreadReply {
            workspace_id: "ws_a".to_string(),
            thread_id: "wt_a".to_string(),
            agent: ThreadReplyAgent {
                id: "codex".to_string(),
                profile: None,
                session_id: "sid-1".to_string(),
            },
            payload: ThreadReplyPayload::AcpSessionNotification {
                notification: serde_json::json!({"sessionId": "sid-1"}),
            },
        };

        assert!(capture.push(reply.clone()));
        assert_eq!(capture.finish(), vec![reply.clone()]);
        // Finished captures let frames flow to the normal fan-out again.
        assert!(!capture.push(reply));
        assert!(capture.finish().is_empty());
    }

    fn target_for(route: RouteKey, reply_to: &str) -> ChannelTarget {
        ChannelTarget::new(route, Some(reply_to.to_string()))
    }

    #[test]
    fn delivery_targets_keep_reply_to_on_origin_only() {
        let origin = RouteKey::with_actor(
            "slack",
            "slack-work",
            "channel-1",
            "codex-reviewer",
            Some("thread-1".to_string()),
        );
        let handover = RouteKey::new("web", "socket-1");

        let targets = delivery_targets(
            vec![origin.clone(), handover.clone()],
            Some(target_for(origin.clone(), "message-1")),
        );

        assert_eq!(targets.len(), 2);
        assert_eq!(targets[0], target_for(origin, "message-1"));
        assert_eq!(targets[1], ChannelTarget::for_route(handover));
    }

    #[test]
    fn delivery_targets_keep_detached_origin_for_the_active_turn() {
        let origin =
            RouteKey::with_actor("slack", "slack-work", "channel-1", "codex-reviewer", None);
        let handover = RouteKey::new("web", "socket-1");

        let targets = delivery_targets(
            vec![handover.clone()],
            Some(target_for(origin.clone(), "message-1")),
        );

        assert_eq!(targets.len(), 2);
        assert_eq!(targets[0], ChannelTarget::for_route(handover));
        assert_eq!(targets[1], target_for(origin, "message-1"));
    }

    #[tokio::test]
    async fn dropping_a_permission_registration_cancels_the_waiter() {
        let (input_tx, _input_rx) = tokio::sync::mpsc::unbounded_channel();
        let plugin_host = Arc::new(PluginHost::new(input_tx));
        let (tx, rx) = tokio::sync::oneshot::channel();

        let registration = PendingPermissionRegistration::register(
            &plugin_host,
            "request-1".to_string(),
            "slack-work".to_string(),
            tx,
        );
        drop(registration);

        assert!(rx.await.is_err());
        assert!(plugin_host
            .respond_permission(
                "slack-work",
                "request-1",
                acp::RequestPermissionResponse::new(acp::RequestPermissionOutcome::Cancelled,),
            )
            .await
            .is_err());
    }

    #[tokio::test]
    async fn cancelling_host_turn_clears_permission_and_rejects_late_response() {
        let (input_tx, _input_rx) = tokio::sync::mpsc::unbounded_channel();
        let plugin_host = Arc::new(PluginHost::new(input_tx));
        let (output_tx, mut output_rx) = tokio::sync::mpsc::unbounded_channel();
        plugin_host.register_websocket_plugin("web", output_tx);
        let active_turn_target = ActiveTurnTarget::default();
        let _target_guard =
            active_turn_target.install(ChannelTarget::for_route(RouteKey::new("web", "chat-1")));
        let handler = Arc::new(permission_handler(
            Arc::clone(&plugin_host),
            active_turn_target.clone(),
        ));

        let permission_task = tokio::spawn({
            let handler = Arc::clone(&handler);
            async move {
                handler
                    .request_permission(super::super::permission::test_permission_request())
                    .await
            }
        });
        let output = output_rx.recv().await.expect("permission output");
        let ChannelOutput::PermissionRequest { request_id, .. } = output else {
            panic!("expected permission request");
        };

        active_turn_target.cancel_current();

        let response = permission_task.await.unwrap().unwrap();
        assert_eq!(response.outcome, acp::RequestPermissionOutcome::Cancelled);
        assert!(plugin_host
            .respond_permission(
                "web",
                &request_id,
                acp::RequestPermissionResponse::new(acp::RequestPermissionOutcome::Cancelled,),
            )
            .await
            .is_err());
    }

    #[test]
    fn host_assignment_parses_target_agent_id() {
        let assignment = HostAssignment::parse(
            r#"{"protocol":"va-agent-protocol","kind":"assignment","to_agent_id":"00000000-0000-0000-0000-000000000001","task":"continue"}"#,
        )
        .unwrap()
        .unwrap();

        assert_eq!(
            assignment.to_agent_id.as_str(),
            "00000000-0000-0000-0000-000000000001"
        );
    }

    #[test]
    fn host_assignment_ignores_non_assignment_protocol_payloads() {
        let parsed = HostAssignment::parse(
            r#"{"protocol":"va-agent-protocol","kind":"report","from_agent_id":"a"}"#,
        )
        .unwrap();

        assert!(parsed.is_none());
    }
}
