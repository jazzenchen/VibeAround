use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::{mpsc, oneshot};

use crate::routing::ChannelKind;
use crate::routing::RouteKey;
use crate::workspace::WorkspaceThreadManager;

use super::ChannelOutput;

/// Outbound sink to a single web chat connection.
pub type WebChatSink = mpsc::UnboundedSender<ChannelOutput>;

const MAX_ROUTE_HISTORY: usize = 4000;
const WEB_ROUTE_IDLE_CLOSE_DELAY: Duration = Duration::from_secs(60);

#[derive(Debug, Clone)]
pub struct WebRouteIdleDeadline {
    route: RouteKey,
    generation: u64,
}

#[derive(Debug, Clone, Copy, Default)]
struct RouteActivity {
    active: bool,
    generation: u64,
}

#[derive(Debug, Clone)]
struct PendingUserMessage {
    message_id: String,
    content: Vec<serde_json::Value>,
}

struct RouteSession {
    agent_id: String,
    session_id: String,
    route: RouteKey,
}

/// Internal websocket-backed channel manager used by the browser chat UI.
pub struct WebChannelManager {
    command_tx: mpsc::UnboundedSender<WebChannelCommand>,
}

type WebChannelCommand = Box<dyn FnOnce(&mut WebChannelState) + Send>;

#[derive(Default)]
struct WebChannelState {
    connections: HashMap<String, HashMap<String, WebChatSink>>,
    route_agents: HashMap<String, String>,
    route_sessions: HashMap<String, RouteSession>,
    route_history: HashMap<String, Vec<ChannelOutput>>,
    route_pending_permissions: HashMap<String, HashMap<String, ChannelOutput>>,
    route_pending_user_messages: HashMap<String, Vec<PendingUserMessage>>,
    route_activity: HashMap<String, RouteActivity>,
}

impl WebChannelManager {
    pub fn new() -> Arc<Self> {
        let (command_tx, mut command_rx) = mpsc::unbounded_channel::<WebChannelCommand>();
        tokio::spawn(async move {
            let mut state = WebChannelState::default();
            while let Some(command) = command_rx.recv().await {
                command(&mut state);
            }
        });
        Arc::new(Self { command_tx })
    }

    async fn request<T: Send + 'static>(
        &self,
        command: impl FnOnce(&mut WebChannelState) -> T + Send + 'static,
    ) -> T {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.command_tx
            .send(Box::new(move |state| {
                let _ = reply_tx.send(command(state));
            }))
            .expect("web channel owner must outlive its manager");
        reply_rx
            .await
            .expect("web channel owner must reply while its manager is alive")
    }

    pub async fn register_connection(
        &self,
        route: &RouteKey,
        connection_id: String,
        sink: WebChatSink,
        replay_history: bool,
    ) {
        let route = route.clone();
        self.request(move |state| {
            state.register_connection(&route, connection_id, sink, replay_history)
        })
        .await;
    }

    pub async fn unregister_connection(&self, route_chat_id: &str, connection_id: &str) {
        let route_chat_id = route_chat_id.to_string();
        let connection_id = connection_id.to_string();
        self.request(move |state| state.unregister_connection(&route_chat_id, &connection_id))
            .await;
    }

    pub async fn forget_route(&self, route_chat_id: &str) {
        let route_chat_id = route_chat_id.to_string();
        self.request(move |state| state.forget_route(&route_chat_id))
            .await;
    }

    pub async fn set_route_agent(&self, route_chat_id: &str, agent_id: String) {
        let route_chat_id = route_chat_id.to_string();
        self.request(move |state| state.set_route_agent(&route_chat_id, agent_id))
            .await;
    }

    pub async fn route_for_session(&self, agent_id: &str, session_id: &str) -> Option<RouteKey> {
        let agent_id = agent_id.to_string();
        let session_id = session_id.to_string();
        self.request(move |state| state.route_for_session(&agent_id, &session_id))
            .await
    }

    pub async fn route_has_session(&self, route_chat_id: &str) -> bool {
        let route_chat_id = route_chat_id.to_string();
        self.request(move |state| state.route_has_session(&route_chat_id))
            .await
    }

    pub async fn route_session_id(&self, route_chat_id: &str) -> Option<String> {
        let route_chat_id = route_chat_id.to_string();
        self.request(move |state| state.route_session_id(&route_chat_id))
            .await
    }

    pub async fn route_is_active(&self, route_chat_id: &str) -> bool {
        let route_chat_id = route_chat_id.to_string();
        self.request(move |state| state.route_is_active(&route_chat_id))
            .await
    }

    pub async fn session_is_active(&self, agent_id: &str, session_id: &str) -> bool {
        let agent_id = agent_id.to_string();
        let session_id = session_id.to_string();
        self.request(move |state| state.session_is_active(&agent_id, &session_id))
            .await
    }

    pub async fn mark_route_active(&self, route: &RouteKey) {
        let route = route.clone();
        self.request(move |state| state.mark_route_active(&route))
            .await;
    }

    pub async fn mark_route_idle(&self, route: &RouteKey) -> WebRouteIdleDeadline {
        let route = route.clone();
        self.request(move |state| state.mark_route_idle(&route))
            .await
    }

    pub async fn bump_idle_route(&self, route: &RouteKey) -> Option<WebRouteIdleDeadline> {
        let route = route.clone();
        self.request(move |state| state.bump_idle_route(&route))
            .await
    }

    pub async fn clear_pending_permission(&self, request_id: &str) {
        let request_id = request_id.to_string();
        self.request(move |state| state.clear_pending_permission(&request_id))
            .await;
    }

    pub async fn record_user_message(
        &self,
        route: &RouteKey,
        message_id: String,
        content: Vec<serde_json::Value>,
        wait_for_session_ready: bool,
    ) {
        let route = route.clone();
        self.request(move |state| {
            state.record_user_message(&route, message_id, content, wait_for_session_ready)
        })
        .await;
    }

    pub async fn dispatch_output(&self, output: ChannelOutput) -> Option<WebRouteIdleDeadline> {
        self.request(move |state| state.dispatch_output(output))
            .await
    }

    pub fn sender(
        &self,
    ) -> (
        mpsc::UnboundedSender<ChannelOutput>,
        mpsc::UnboundedReceiver<ChannelOutput>,
    ) {
        mpsc::unbounded_channel()
    }

    pub fn schedule_idle_close(
        self: &Arc<Self>,
        workspace_threads: Arc<WorkspaceThreadManager>,
        deadline: WebRouteIdleDeadline,
    ) {
        let manager = Arc::clone(self);
        tokio::spawn(async move {
            tokio::time::sleep(WEB_ROUTE_IDLE_CLOSE_DELAY).await;
            let current = manager
                .request({
                    let deadline = deadline.clone();
                    move |state| state.is_idle_deadline_current(&deadline)
                })
                .await;
            if !current {
                return;
            }
            let _ = workspace_threads.shutdown_route_host(&deadline.route).await;
        });
    }
}

impl WebChannelState {
    pub fn register_connection(
        &mut self,
        route: &RouteKey,
        connection_id: String,
        sink: WebChatSink,
        replay_history: bool,
    ) {
        let route_chat_id = route.chat_id.clone();
        self.connections
            .entry(route_chat_id.clone())
            .or_default()
            .insert(connection_id, sink.clone());

        if replay_history {
            let history = self
                .route_history
                .get(&route_chat_id)
                .cloned()
                .unwrap_or_default();
            for output in history {
                let _ = sink.send(output);
            }
            let pending_permissions = self
                .route_pending_permissions
                .get(&route_chat_id)
                .map(|items| items.values().cloned().collect::<Vec<_>>())
                .unwrap_or_default();
            for output in pending_permissions {
                let _ = sink.send(output);
            }
        }
        let active = self.route_activity_state(&route_chat_id);
        if active.is_some() || self.route_has_session(&route_chat_id) {
            let _ = sink.send(ChannelOutput::TurnStatus {
                route: route.clone(),
                active: active.unwrap_or(false),
            });
        }
    }

    pub fn unregister_connection(&mut self, route_chat_id: &str, connection_id: &str) {
        let Some(route_connections) = self.connections.get_mut(route_chat_id) else {
            return;
        };
        route_connections.remove(connection_id);
        if route_connections.is_empty() {
            self.connections.remove(route_chat_id);
        }
    }

    pub fn forget_route(&mut self, route_chat_id: &str) {
        self.route_history.remove(route_chat_id);
        self.route_agents.remove(route_chat_id);
        self.route_sessions.remove(route_chat_id);
        self.clear_route_pending_permissions(route_chat_id);
        self.clear_route_pending_user_messages(route_chat_id);
        self.route_activity.remove(route_chat_id);
    }

    pub fn set_route_agent(&mut self, route_chat_id: &str, agent_id: String) {
        self.route_agents
            .insert(route_chat_id.to_string(), agent_id);
    }

    pub fn route_for_session(&self, agent_id: &str, session_id: &str) -> Option<RouteKey> {
        self.route_sessions
            .values()
            .find(|session| session.agent_id == agent_id && session.session_id == session_id)
            .map(|session| session.route.clone())
    }

    pub fn route_has_session(&self, route_chat_id: &str) -> bool {
        self.route_sessions.contains_key(route_chat_id)
    }

    pub fn route_session_id(&self, route_chat_id: &str) -> Option<String> {
        self.route_sessions
            .get(route_chat_id)
            .map(|session| session.session_id.clone())
    }

    pub fn route_is_active(&self, route_chat_id: &str) -> bool {
        self.route_activity
            .get(route_chat_id)
            .is_some_and(|activity| activity.active)
    }

    pub fn session_is_active(&self, agent_id: &str, session_id: &str) -> bool {
        self.route_for_session(agent_id, session_id)
            .as_ref()
            .is_some_and(|route| self.route_is_active(&route.chat_id))
    }

    pub fn mark_route_active(&mut self, route: &RouteKey) {
        let entry = self
            .route_activity
            .entry(route.chat_id.clone())
            .or_default();
        entry.active = true;
        entry.generation = entry.generation.wrapping_add(1);
        self.send_turn_status(route, true);
    }

    pub fn mark_route_idle(&mut self, route: &RouteKey) -> WebRouteIdleDeadline {
        let entry = self
            .route_activity
            .entry(route.chat_id.clone())
            .or_default();
        entry.active = false;
        entry.generation = entry.generation.wrapping_add(1);
        let generation = entry.generation;
        self.send_turn_status(route, false);
        WebRouteIdleDeadline {
            route: route.clone(),
            generation,
        }
    }

    pub fn bump_idle_route(&mut self, route: &RouteKey) -> Option<WebRouteIdleDeadline> {
        let entry = self
            .route_activity
            .entry(route.chat_id.clone())
            .or_default();
        if entry.active {
            return None;
        }
        entry.generation = entry.generation.wrapping_add(1);
        Some(WebRouteIdleDeadline {
            route: route.clone(),
            generation: entry.generation,
        })
    }

    pub fn clear_pending_permission(&mut self, request_id: &str) {
        let mut empty_route = None;
        for (route_chat_id, pending) in &mut self.route_pending_permissions {
            if pending.remove(request_id).is_some() {
                if pending.is_empty() {
                    empty_route = Some(route_chat_id.clone());
                }
                break;
            }
        }
        if let Some(route_chat_id) = empty_route {
            self.route_pending_permissions.remove(&route_chat_id);
        }
    }

    pub fn record_user_message(
        &mut self,
        route: &RouteKey,
        message_id: String,
        content: Vec<serde_json::Value>,
        wait_for_session_ready: bool,
    ) {
        if content.is_empty() {
            return;
        }
        let message = PendingUserMessage {
            message_id,
            content,
        };
        if !wait_for_session_ready {
            if let Some(session_id) = self.route_session_id(&route.chat_id) {
                for output in user_message_outputs(route, &session_id, message) {
                    self.broadcast_output(&route.chat_id, output);
                }
                return;
            }
        }
        self.route_pending_user_messages
            .entry(route.chat_id.clone())
            .or_default()
            .push(message);
    }

    pub fn dispatch_output(&mut self, output: ChannelOutput) -> Option<WebRouteIdleDeadline> {
        let route = output.route_key().clone();
        if matches!(output, ChannelOutput::SessionInfo { .. }) {
            return self.bump_idle_route(&route);
        }
        let chat_id = route.chat_id.clone();
        let mut follow_up_outputs = Vec::new();
        if let ChannelOutput::SessionReady { session_id, .. } = &output {
            self.bind_route_session(&route, session_id);
            follow_up_outputs = self.take_pending_user_message_outputs(&route, session_id);
        }
        if let ChannelOutput::PermissionRequest { request_id, .. } = &output {
            self.remember_pending_permission(&chat_id, request_id, &output);
        }
        if matches!(output, ChannelOutput::PromptDone { .. }) {
            self.clear_route_pending_permissions(&chat_id);
            self.clear_route_pending_user_messages(&chat_id);
        }
        self.broadcast_output(&chat_id, output.clone());
        for output in follow_up_outputs {
            self.broadcast_output(&chat_id, output);
        }

        if matches!(output, ChannelOutput::PromptDone { .. }) {
            Some(self.mark_route_idle(&route))
        } else {
            self.bump_idle_route(&route)
        }
    }

    fn bind_route_session(&mut self, route: &RouteKey, session_id: &str) {
        let Some(agent_id) = self.route_agents.get(&route.chat_id).cloned() else {
            return;
        };
        self.route_sessions.insert(
            route.chat_id.clone(),
            RouteSession {
                agent_id,
                session_id: session_id.to_string(),
                route: route.clone(),
            },
        );
    }

    fn broadcast_output(&mut self, route_chat_id: &str, output: ChannelOutput) {
        self.push_route_history(route_chat_id, output.clone());

        let sinks = self
            .connections
            .get(route_chat_id)
            .map(|connections| connections.values().cloned().collect::<Vec<_>>())
            .unwrap_or_default();
        tracing::info!(
            "[WebChannelManager] dispatch_output chat_id={} subscribers={}",
            route_chat_id,
            sinks.len()
        );
        for sink in sinks {
            let _ = sink.send(output.clone());
        }
    }

    fn push_route_history(&mut self, route_chat_id: &str, output: ChannelOutput) {
        if matches!(
            output,
            ChannelOutput::PermissionRequest { .. } | ChannelOutput::TurnStatus { .. }
        ) {
            return;
        }
        let items = self
            .route_history
            .entry(route_chat_id.to_string())
            .or_default();
        items.push(output);
        if items.len() > MAX_ROUTE_HISTORY {
            let overflow = items.len() - MAX_ROUTE_HISTORY;
            items.drain(0..overflow);
        }
    }

    fn remember_pending_permission(
        &mut self,
        route_chat_id: &str,
        request_id: &str,
        output: &ChannelOutput,
    ) {
        self.route_pending_permissions
            .entry(route_chat_id.to_string())
            .or_default()
            .insert(request_id.to_string(), output.clone());
    }

    fn clear_route_pending_permissions(&mut self, route_chat_id: &str) {
        self.route_pending_permissions.remove(route_chat_id);
    }

    fn clear_route_pending_user_messages(&mut self, route_chat_id: &str) {
        self.route_pending_user_messages.remove(route_chat_id);
    }

    fn take_pending_user_message_outputs(
        &mut self,
        route: &RouteKey,
        session_id: &str,
    ) -> Vec<ChannelOutput> {
        self.route_pending_user_messages
            .remove(&route.chat_id)
            .unwrap_or_default()
            .into_iter()
            .flat_map(|message| user_message_outputs(route, session_id, message))
            .collect()
    }

    fn is_idle_deadline_current(&self, deadline: &WebRouteIdleDeadline) -> bool {
        self.route_activity
            .get(&deadline.route.chat_id)
            .is_some_and(|activity| !activity.active && activity.generation == deadline.generation)
    }

    fn route_activity_state(&self, route_chat_id: &str) -> Option<bool> {
        self.route_activity
            .get(route_chat_id)
            .map(|activity| activity.active)
    }

    fn send_turn_status(&self, route: &RouteKey, active: bool) {
        let output = ChannelOutput::TurnStatus {
            route: route.clone(),
            active,
        };
        let sinks = self
            .connections
            .get(&route.chat_id)
            .map(|connections| connections.values().cloned().collect::<Vec<_>>())
            .unwrap_or_default();
        for sink in sinks {
            let _ = sink.send(output.clone());
        }
    }
}

fn user_message_outputs(
    route: &RouteKey,
    session_id: &str,
    message: PendingUserMessage,
) -> Vec<ChannelOutput> {
    let PendingUserMessage {
        message_id,
        content,
    } = message;
    content
        .into_iter()
        .map(|block| ChannelOutput::RawAcp {
            route: route.clone(),
            payload: serde_json::json!({
                "sessionId": session_id,
                "update": {
                    "sessionUpdate": "user_message_chunk",
                    "messageId": message_id.clone(),
                    "content": block,
                },
            }),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn register_connection_replays_active_turn_status_without_session() {
        let manager = WebChannelManager::new();
        let route = RouteKey::new("web", "chat-1");
        manager.mark_route_active(&route).await;

        let (tx, mut rx) = manager.sender();
        manager
            .register_connection(&route, "conn-1".to_string(), tx, true)
            .await;

        assert_eq!(
            rx.try_recv().expect("turn status"),
            ChannelOutput::TurnStatus {
                route,
                active: true,
            }
        );
    }

    #[tokio::test]
    async fn pending_user_message_replays_after_session_ready() {
        let manager = WebChannelManager::new();
        let route = RouteKey::new("web", "chat-1");
        manager.set_route_agent("chat-1", "codex".to_string()).await;

        let (tx, mut rx) = manager.sender();
        manager
            .register_connection(&route, "conn-1".to_string(), tx, true)
            .await;
        manager
            .record_user_message(
                &route,
                "msg-1".to_string(),
                vec![serde_json::json!({"type": "text", "text": "hello"})],
                true,
            )
            .await;
        assert!(rx.try_recv().is_err());

        manager
            .dispatch_output(ChannelOutput::SessionReady {
                route: route.clone(),
                reply_to: None,
                session_id: "sid-1".to_string(),
            })
            .await;

        assert_eq!(
            rx.try_recv().expect("session ready"),
            ChannelOutput::SessionReady {
                route: route.clone(),
                reply_to: None,
                session_id: "sid-1".to_string(),
            }
        );
        let output = rx.try_recv().expect("user message replay");
        let ChannelOutput::RawAcp { payload, .. } = output else {
            panic!("expected raw acp user message replay");
        };
        assert_eq!(payload["sessionId"].as_str(), Some("sid-1"));
        assert_eq!(
            payload["update"]["sessionUpdate"].as_str(),
            Some("user_message_chunk")
        );
        assert_eq!(payload["update"]["messageId"].as_str(), Some("msg-1"));
        assert_eq!(payload["update"]["content"]["text"].as_str(), Some("hello"));

        let (tx, mut replay_rx) = manager.sender();
        manager
            .register_connection(&route, "conn-2".to_string(), tx, true)
            .await;
        assert!(matches!(
            replay_rx.try_recv().expect("replayed session ready"),
            ChannelOutput::SessionReady { .. }
        ));
        assert!(matches!(
            replay_rx.try_recv().expect("replayed user message"),
            ChannelOutput::RawAcp { .. }
        ));
        assert_eq!(
            replay_rx.try_recv().expect("replayed turn status"),
            ChannelOutput::TurnStatus {
                route,
                active: false,
            }
        );
    }

    #[tokio::test]
    async fn session_routes_keep_original_channel_kind() {
        let manager = WebChannelManager::new();
        let route = RouteKey::new("tui", "chat-1");
        manager.set_route_agent("chat-1", "codex".to_string()).await;
        manager.mark_route_active(&route).await;

        manager
            .dispatch_output(ChannelOutput::SessionReady {
                route: route.clone(),
                reply_to: None,
                session_id: "sid-1".to_string(),
            })
            .await;

        assert_eq!(
            manager.route_for_session("codex", "sid-1").await,
            Some(route.clone())
        );
        assert!(manager.session_is_active("codex", "sid-1").await);
    }

    #[tokio::test]
    async fn prompt_done_drops_unbound_pending_user_message() {
        let manager = WebChannelManager::new();
        let route = RouteKey::new("web", "chat-1");
        manager.set_route_agent("chat-1", "codex".to_string()).await;

        let (tx, mut rx) = manager.sender();
        manager
            .register_connection(&route, "conn-1".to_string(), tx, true)
            .await;
        manager
            .record_user_message(
                &route,
                "msg-1".to_string(),
                vec![serde_json::json!({"type": "text", "text": "hello"})],
                true,
            )
            .await;
        manager
            .dispatch_output(ChannelOutput::PromptDone {
                route: route.clone(),
                message_id: Some("msg-1".to_string()),
            })
            .await;
        while rx.try_recv().is_ok() {}

        manager
            .dispatch_output(ChannelOutput::SessionReady {
                route,
                reply_to: None,
                session_id: "sid-1".to_string(),
            })
            .await;

        assert!(matches!(
            rx.try_recv().expect("session ready"),
            ChannelOutput::SessionReady { .. }
        ));
        assert!(rx.try_recv().is_err());
    }

    #[tokio::test]
    async fn cleared_permission_is_not_replayed() {
        let manager = WebChannelManager::new();
        let route = RouteKey::new("web", "chat-1");
        manager
            .dispatch_output(ChannelOutput::PermissionRequest {
                route: route.clone(),
                reply_to: None,
                request_id: "permission-1".to_string(),
                payload: serde_json::json!({}),
            })
            .await;
        manager.clear_pending_permission("permission-1").await;

        let (tx, mut replay_rx) = manager.sender();
        manager
            .register_connection(&route, "conn-1".to_string(), tx, true)
            .await;

        while let Ok(output) = replay_rx.try_recv() {
            assert!(!matches!(output, ChannelOutput::PermissionRequest { .. }));
        }
    }

    #[tokio::test]
    async fn forget_route_drops_replay_and_session_state() {
        let manager = WebChannelManager::new();
        let route = RouteKey::new("web", "chat-1");
        manager.set_route_agent("chat-1", "codex".to_string()).await;
        manager.mark_route_active(&route).await;
        manager
            .dispatch_output(ChannelOutput::SessionReady {
                route: route.clone(),
                reply_to: None,
                session_id: "sid-1".to_string(),
            })
            .await;
        manager
            .dispatch_output(ChannelOutput::SystemText {
                route: route.clone(),
                text: "hello".to_string(),
                reply_to: None,
            })
            .await;

        let (tx, mut replay_rx) = manager.sender();
        manager
            .register_connection(&route, "conn-1".to_string(), tx, true)
            .await;
        assert!(matches!(
            replay_rx.try_recv().expect("replayed session ready"),
            ChannelOutput::SessionReady { .. }
        ));
        assert!(matches!(
            replay_rx.try_recv().expect("replayed system text"),
            ChannelOutput::SystemText { .. }
        ));

        manager.forget_route(&route.chat_id).await;
        assert!(!manager.route_has_session(&route.chat_id).await);
        assert_eq!(manager.route_for_session("codex", "sid-1").await, None);
        assert!(!manager.route_is_active(&route.chat_id).await);

        let (tx, mut replay_rx) = manager.sender();
        manager
            .register_connection(&route, "conn-2".to_string(), tx, true)
            .await;
        assert!(replay_rx.try_recv().is_err());
    }
}

#[derive(Debug)]
pub struct WebSocketPluginRuntime {
    channel_kind: ChannelKind,
    outbound_tx: mpsc::UnboundedSender<ChannelOutput>,
}

impl WebSocketPluginRuntime {
    pub fn new(
        channel_kind: impl Into<ChannelKind>,
        outbound_tx: mpsc::UnboundedSender<ChannelOutput>,
    ) -> Arc<Self> {
        Arc::new(Self {
            channel_kind: channel_kind.into(),
            outbound_tx,
        })
    }

    pub fn send_output_now(&self, output: ChannelOutput) -> Result<(), String> {
        tracing::info!(
            "[WebSocketPluginRuntime] send_output channel_kind={} route={}",
            self.channel_kind,
            output.route_key()
        );
        self.outbound_tx.send(output).map_err(|error| {
            let message = format!("failed to deliver websocket output: {error}");
            tracing::info!("[{}] {}", self.channel_kind, message);
            message
        })
    }
}
