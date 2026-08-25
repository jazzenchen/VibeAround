use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::{mpsc, oneshot};

use crate::routing::ChannelKind;
use crate::routing::RouteKey;

use super::ChannelOutput;

/// Outbound sink to a single web chat connection.
pub type WebChatSink = mpsc::UnboundedSender<ChannelOutput>;

const MAX_ROUTE_HISTORY: usize = 4000;

#[derive(Debug, Clone)]
struct PendingUserMessage {
    message_id: String,
    content: Vec<serde_json::Value>,
}

struct RouteSession {
    agent_id: String,
    session_id: String,
}

/// Internal websocket-backed channel manager used by the browser chat UI.
pub struct WebChannelManager {
    command_tx: mpsc::UnboundedSender<WebChannelCommand>,
}

type WebChannelCommand = Box<dyn FnOnce(&mut WebChannelState) + Send>;

#[derive(Default)]
struct WebChannelState {
    connections: HashMap<RouteKey, HashMap<String, WebChatSink>>,
    route_agents: HashMap<RouteKey, String>,
    route_sessions: HashMap<RouteKey, RouteSession>,
    route_history: HashMap<RouteKey, Vec<ChannelOutput>>,
    route_pending_permissions: HashMap<RouteKey, HashMap<String, ChannelOutput>>,
    route_pending_user_messages: HashMap<RouteKey, Vec<PendingUserMessage>>,
    route_activity: HashMap<RouteKey, bool>,
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

    pub async fn unregister_connection(&self, route: &RouteKey, connection_id: &str) {
        let route = route.clone();
        let connection_id = connection_id.to_string();
        self.request(move |state| state.unregister_connection(&route, &connection_id))
            .await;
    }

    pub async fn forget_route(&self, route: &RouteKey) {
        let route = route.clone();
        self.request(move |state| state.forget_route(&route)).await;
    }

    pub async fn set_route_agent(&self, route: &RouteKey, agent_id: String) {
        let route = route.clone();
        self.request(move |state| state.set_route_agent(&route, agent_id))
            .await;
    }

    pub async fn route_for_session(&self, agent_id: &str, session_id: &str) -> Option<RouteKey> {
        let agent_id = agent_id.to_string();
        let session_id = session_id.to_string();
        self.request(move |state| state.route_for_session(&agent_id, &session_id))
            .await
    }

    pub async fn route_has_session(&self, route: &RouteKey) -> bool {
        let route = route.clone();
        self.request(move |state| state.route_has_session(&route))
            .await
    }

    pub async fn route_session_id(&self, route: &RouteKey) -> Option<String> {
        let route = route.clone();
        self.request(move |state| state.route_session_id(&route))
            .await
    }

    pub async fn route_is_active(&self, route: &RouteKey) -> bool {
        let route = route.clone();
        self.request(move |state| state.route_is_active(&route))
            .await
    }

    /// Whether any browser or TUI is still listening on this route.
    pub async fn route_has_connections(&self, route: &RouteKey) -> bool {
        let route = route.clone();
        self.request(move |state| state.route_has_connections(&route))
            .await
    }

    pub async fn session_is_active(&self, agent_id: &str, session_id: &str) -> bool {
        let agent_id = agent_id.to_string();
        let session_id = session_id.to_string();
        self.request(move |state| state.session_is_active(&agent_id, &session_id))
            .await
    }

    pub async fn clear_pending_permission(&self, route: &RouteKey, request_id: &str) -> bool {
        let route = route.clone();
        let request_id = request_id.to_string();
        self.request(move |state| state.clear_pending_permission(&route, &request_id))
            .await
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

    pub async fn dispatch_output(&self, output: ChannelOutput) {
        self.request(move |state| state.dispatch_output(output))
            .await;
    }

    pub fn sender(
        &self,
    ) -> (
        mpsc::UnboundedSender<ChannelOutput>,
        mpsc::UnboundedReceiver<ChannelOutput>,
    ) {
        mpsc::unbounded_channel()
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
        self.connections
            .entry(route.clone())
            .or_default()
            .insert(connection_id, sink.clone());

        if replay_history {
            let history = self.route_history.get(route).cloned().unwrap_or_default();
            for output in history {
                let _ = sink.send(output);
            }
            let pending_permissions = self
                .route_pending_permissions
                .get(route)
                .map(|items| items.values().cloned().collect::<Vec<_>>())
                .unwrap_or_default();
            for output in pending_permissions {
                let _ = sink.send(output);
            }
        }
        if let Some(active) = self.route_activity.get(route).copied() {
            let _ = sink.send(ChannelOutput::TurnStatus {
                route: route.clone(),
                active,
            });
        }
    }

    pub fn unregister_connection(&mut self, route: &RouteKey, connection_id: &str) {
        let Some(route_connections) = self.connections.get_mut(route) else {
            return;
        };
        route_connections.remove(connection_id);
        if route_connections.is_empty() {
            self.connections.remove(route);
        }
    }

    pub fn forget_route(&mut self, route: &RouteKey) {
        self.route_history.remove(route);
        self.route_agents.remove(route);
        self.route_sessions.remove(route);
        self.clear_route_pending_permissions(route);
        self.clear_route_pending_user_messages(route);
        self.route_activity.remove(route);
    }

    pub fn set_route_agent(&mut self, route: &RouteKey, agent_id: String) {
        self.route_agents.insert(route.clone(), agent_id);
    }

    pub fn route_for_session(&self, agent_id: &str, session_id: &str) -> Option<RouteKey> {
        self.route_sessions
            .iter()
            .find(|(_, session)| session.agent_id == agent_id && session.session_id == session_id)
            .map(|(route, _)| route.clone())
    }

    pub fn route_has_session(&self, route: &RouteKey) -> bool {
        self.route_sessions.contains_key(route)
    }

    pub fn route_session_id(&self, route: &RouteKey) -> Option<String> {
        self.route_sessions
            .get(route)
            .map(|session| session.session_id.clone())
    }

    pub fn route_is_active(&self, route: &RouteKey) -> bool {
        self.route_activity.get(route).copied().unwrap_or(false)
    }

    pub fn route_has_connections(&self, route: &RouteKey) -> bool {
        self.connections
            .get(route)
            .is_some_and(|connections| !connections.is_empty())
    }

    pub fn session_is_active(&self, agent_id: &str, session_id: &str) -> bool {
        self.route_for_session(agent_id, session_id)
            .as_ref()
            .is_some_and(|route| self.route_is_active(route))
    }

    pub fn clear_pending_permission(&mut self, route: &RouteKey, request_id: &str) -> bool {
        let Some(pending) = self.route_pending_permissions.get_mut(route) else {
            return false;
        };
        let removed = pending.remove(request_id).is_some();
        if pending.is_empty() {
            self.route_pending_permissions.remove(route);
        }
        removed
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
            if let Some(session_id) = self.route_session_id(route) {
                for output in user_message_outputs(route, &session_id, message) {
                    self.broadcast_output(route, output);
                }
                return;
            }
        }
        self.route_pending_user_messages
            .entry(route.clone())
            .or_default()
            .push(message);
    }

    pub fn dispatch_output(&mut self, output: ChannelOutput) {
        let route = output.route_key().clone();
        let mut follow_up_outputs = Vec::new();
        if let ChannelOutput::SessionReady { session_id, .. } = &output {
            self.bind_route_session(&route, session_id);
            follow_up_outputs = self.take_pending_user_message_outputs(&route, session_id);
        }
        if let ChannelOutput::PermissionRequest { request_id, .. } = &output {
            self.remember_pending_permission(&route, request_id, &output);
        }
        if let ChannelOutput::TurnStatus { active, .. } = &output {
            self.route_activity.insert(route.clone(), *active);
            if !*active {
                self.clear_route_pending_permissions(&route);
                self.clear_route_pending_user_messages(&route);
            }
        }
        self.broadcast_output(&route, output);
        for output in follow_up_outputs {
            self.broadcast_output(&route, output);
        }
    }

    fn bind_route_session(&mut self, route: &RouteKey, session_id: &str) {
        let Some(agent_id) = self.route_agents.get(route).cloned() else {
            return;
        };
        self.route_sessions.insert(
            route.clone(),
            RouteSession {
                agent_id,
                session_id: session_id.to_string(),
            },
        );
    }

    fn broadcast_output(&mut self, route: &RouteKey, output: ChannelOutput) {
        self.push_route_history(route, output.clone());

        let sinks = self
            .connections
            .get(route)
            .map(|connections| connections.values().cloned().collect::<Vec<_>>())
            .unwrap_or_default();
        tracing::info!(
            "[WebChannelManager] dispatch_output route={} subscribers={}",
            route,
            sinks.len()
        );
        for sink in sinks {
            let _ = sink.send(output.clone());
        }
    }

    fn push_route_history(&mut self, route: &RouteKey, output: ChannelOutput) {
        if matches!(
            output,
            ChannelOutput::PermissionRequest { .. } | ChannelOutput::TurnStatus { .. }
        ) {
            return;
        }
        let items = self.route_history.entry(route.clone()).or_default();
        items.push(output);
        if items.len() > MAX_ROUTE_HISTORY {
            let overflow = items.len() - MAX_ROUTE_HISTORY;
            items.drain(0..overflow);
        }
    }

    fn remember_pending_permission(
        &mut self,
        route: &RouteKey,
        request_id: &str,
        output: &ChannelOutput,
    ) {
        self.route_pending_permissions
            .entry(route.clone())
            .or_default()
            .insert(request_id.to_string(), output.clone());
    }

    fn clear_route_pending_permissions(&mut self, route: &RouteKey) {
        self.route_pending_permissions.remove(route);
    }

    fn clear_route_pending_user_messages(&mut self, route: &RouteKey) {
        self.route_pending_user_messages.remove(route);
    }

    fn take_pending_user_message_outputs(
        &mut self,
        route: &RouteKey,
        session_id: &str,
    ) -> Vec<ChannelOutput> {
        self.route_pending_user_messages
            .remove(route)
            .unwrap_or_default()
            .into_iter()
            .flat_map(|message| user_message_outputs(route, session_id, message))
            .collect()
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
        manager
            .dispatch_output(ChannelOutput::TurnStatus {
                route: route.clone(),
                active: true,
            })
            .await;

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
    async fn a_route_has_connections_only_while_one_is_registered() {
        let manager = WebChannelManager::new();
        let route = RouteKey::new("web", "chat-1");
        assert!(!manager.route_has_connections(&route).await);

        let (first, _first_rx) = manager.sender();
        manager
            .register_connection(&route, "conn-1".to_string(), first, false)
            .await;
        let (second, _second_rx) = manager.sender();
        manager
            .register_connection(&route, "conn-2".to_string(), second, false)
            .await;
        assert!(manager.route_has_connections(&route).await);

        // One tab closing leaves the other one listening.
        manager.unregister_connection(&route, "conn-1").await;
        assert!(manager.route_has_connections(&route).await);

        manager.unregister_connection(&route, "conn-2").await;
        assert!(!manager.route_has_connections(&route).await);
    }

    #[tokio::test]
    async fn session_info_reaches_the_browser() {
        let manager = WebChannelManager::new();
        let route = RouteKey::new("web", "chat-1");
        let (tx, mut rx) = manager.sender();
        manager
            .register_connection(&route, "conn-1".to_string(), tx, true)
            .await;
        let info = crate::channels::types::ChannelSessionInfo {
            workspace_id: "ws_general".to_string(),
            workspace_path: "/tmp/project".to_string(),
            thread_id: "wt_1".to_string(),
            agent: crate::channels::types::ChannelSessionAgent {
                id: "codex".to_string(),
                name: "Codex".to_string(),
                version: String::new(),
                profile_id: Some("deepseek".to_string()),
            },
            session_id: "sid-1".to_string(),
            start: crate::channels::types::ChannelSessionStart::New,
        };

        manager
            .dispatch_output(ChannelOutput::SessionInfo {
                route: route.clone(),
                reply_to: None,
                info: info.clone(),
            })
            .await;

        assert_eq!(
            rx.try_recv().expect("session info"),
            ChannelOutput::SessionInfo {
                route,
                reply_to: None,
                info,
            }
        );
    }

    #[tokio::test]
    async fn turn_status_is_stored_and_forwarded_without_derived_events() {
        let manager = WebChannelManager::new();
        let route = RouteKey::new("web", "chat-1");
        let (tx, mut rx) = manager.sender();
        manager
            .register_connection(&route, "conn-1".to_string(), tx, true)
            .await;
        manager
            .dispatch_output(ChannelOutput::TurnStatus {
                route: route.clone(),
                active: true,
            })
            .await;
        assert_eq!(
            rx.try_recv().expect("active turn status"),
            ChannelOutput::TurnStatus {
                route: route.clone(),
                active: true,
            }
        );
        assert!(manager.route_is_active(&route).await);

        manager
            .dispatch_output(ChannelOutput::TurnStatus {
                route: route.clone(),
                active: false,
            })
            .await;
        assert_eq!(
            rx.try_recv().expect("idle turn status"),
            ChannelOutput::TurnStatus {
                route: route.clone(),
                active: false,
            }
        );
        assert!(!manager.route_is_active(&route).await);
        assert!(rx.try_recv().is_err());
    }

    #[tokio::test]
    async fn pending_user_message_replays_after_session_ready() {
        let manager = WebChannelManager::new();
        let route = RouteKey::new("web", "chat-1");
        manager.set_route_agent(&route, "codex".to_string()).await;

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
        assert!(replay_rx.try_recv().is_err());
    }

    #[tokio::test]
    async fn session_routes_keep_original_channel_kind() {
        let manager = WebChannelManager::new();
        let route = RouteKey::new("tui", "chat-1");
        manager.set_route_agent(&route, "codex".to_string()).await;
        manager
            .dispatch_output(ChannelOutput::TurnStatus {
                route: route.clone(),
                active: true,
            })
            .await;

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
    async fn idle_turn_drops_pending_permission_and_unbound_user_message() {
        let manager = WebChannelManager::new();
        let route = RouteKey::new("web", "chat-1");
        manager.set_route_agent(&route, "codex".to_string()).await;

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
            .dispatch_output(ChannelOutput::PermissionRequest {
                route: route.clone(),
                reply_to: None,
                request_id: "permission-1".to_string(),
                payload: serde_json::json!({}),
            })
            .await;
        manager
            .dispatch_output(ChannelOutput::TurnStatus {
                route: route.clone(),
                active: false,
            })
            .await;
        while rx.try_recv().is_ok() {}

        let (tx, mut replay_rx) = manager.sender();
        manager
            .register_connection(&route, "conn-2".to_string(), tx, true)
            .await;
        assert!(matches!(
            replay_rx.try_recv().expect("stored idle status"),
            ChannelOutput::TurnStatus { active: false, .. }
        ));
        assert!(replay_rx.try_recv().is_err());

        manager
            .dispatch_output(ChannelOutput::SessionReady {
                route: route.clone(),
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
    async fn same_chat_id_routes_keep_independent_state_and_connections() {
        let manager = WebChannelManager::new();
        let web_route = RouteKey::new("web", "chat-1");
        let tui_route = RouteKey::new("tui", "chat-1");
        let (web_tx, mut web_rx) = manager.sender();
        let (tui_tx, mut tui_rx) = manager.sender();
        manager
            .register_connection(&web_route, "web-conn".to_string(), web_tx, true)
            .await;
        manager
            .register_connection(&tui_route, "tui-conn".to_string(), tui_tx, true)
            .await;

        manager
            .dispatch_output(ChannelOutput::TurnStatus {
                route: web_route.clone(),
                active: true,
            })
            .await;

        assert_eq!(
            web_rx.try_recv().expect("web turn status"),
            ChannelOutput::TurnStatus {
                route: web_route.clone(),
                active: true,
            }
        );
        assert!(tui_rx.try_recv().is_err());
        assert!(manager.route_is_active(&web_route).await);
        assert!(!manager.route_is_active(&tui_route).await);
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
        assert!(
            manager
                .clear_pending_permission(&route, "permission-1")
                .await
        );

        let (tx, mut replay_rx) = manager.sender();
        manager
            .register_connection(&route, "conn-1".to_string(), tx, true)
            .await;

        while let Ok(output) = replay_rx.try_recv() {
            assert!(!matches!(output, ChannelOutput::PermissionRequest { .. }));
        }
    }

    #[tokio::test]
    async fn permission_response_cannot_clear_another_route() {
        let manager = WebChannelManager::new();
        let owning_route = RouteKey::new("web", "chat-1");
        let other_route = RouteKey::new("web", "chat-2");
        manager
            .dispatch_output(ChannelOutput::PermissionRequest {
                route: owning_route.clone(),
                reply_to: None,
                request_id: "permission-1".to_string(),
                payload: serde_json::json!({}),
            })
            .await;

        assert!(
            !manager
                .clear_pending_permission(&other_route, "permission-1")
                .await
        );
        assert!(
            manager
                .clear_pending_permission(&owning_route, "permission-1")
                .await
        );
    }

    #[tokio::test]
    async fn forget_route_drops_replay_and_session_state() {
        let manager = WebChannelManager::new();
        let route = RouteKey::new("web", "chat-1");
        manager.set_route_agent(&route, "codex".to_string()).await;
        manager
            .dispatch_output(ChannelOutput::TurnStatus {
                route: route.clone(),
                active: true,
            })
            .await;
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

        manager.forget_route(&route).await;
        assert!(!manager.route_has_session(&route).await);
        assert_eq!(manager.route_for_session("codex", "sid-1").await, None);
        assert!(!manager.route_is_active(&route).await);

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
