use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use parking_lot::RwLock;
use tokio::sync::mpsc;

use crate::routing::ChannelKind;
use crate::routing::RouteKey;
use crate::workspace::WorkspaceThreadManager;

use super::ChannelOutput;

/// Outbound sink to a single web chat connection.
pub type WebChatSink = mpsc::UnboundedSender<ChannelOutput>;

const MAX_ROUTE_HISTORY: usize = 4000;
const SESSION_KEY_SEPARATOR: char = '\u{1f}';
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

/// Internal websocket-backed channel manager used by the browser chat UI.
pub struct WebChannelManager {
    connections: RwLock<HashMap<String, HashMap<String, WebChatSink>>>,
    route_agents: RwLock<HashMap<String, String>>,
    route_sessions: RwLock<HashMap<String, String>>,
    session_routes: RwLock<HashMap<String, RouteKey>>,
    route_history: RwLock<HashMap<String, Vec<ChannelOutput>>>,
    route_pending_permissions: RwLock<HashMap<String, HashMap<String, ChannelOutput>>>,
    route_pending_user_messages: RwLock<HashMap<String, Vec<PendingUserMessage>>>,
    permission_routes: RwLock<HashMap<String, String>>,
    route_activity: RwLock<HashMap<String, RouteActivity>>,
}

impl WebChannelManager {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            connections: RwLock::new(HashMap::new()),
            route_agents: RwLock::new(HashMap::new()),
            route_sessions: RwLock::new(HashMap::new()),
            session_routes: RwLock::new(HashMap::new()),
            route_history: RwLock::new(HashMap::new()),
            route_pending_permissions: RwLock::new(HashMap::new()),
            route_pending_user_messages: RwLock::new(HashMap::new()),
            permission_routes: RwLock::new(HashMap::new()),
            route_activity: RwLock::new(HashMap::new()),
        })
    }

    pub fn register_connection(
        &self,
        route: &RouteKey,
        connection_id: String,
        sink: WebChatSink,
        replay_history: bool,
    ) {
        let route_chat_id = route.chat_id.clone();
        self.connections
            .write()
            .entry(route_chat_id.clone())
            .or_default()
            .insert(connection_id, sink.clone());

        if replay_history {
            let history = self
                .route_history
                .read()
                .get(&route_chat_id)
                .cloned()
                .unwrap_or_default();
            for output in history {
                let _ = sink.send(output);
            }
            let pending_permissions = self
                .route_pending_permissions
                .read()
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

    pub fn unregister_connection(&self, route_chat_id: &str, connection_id: &str) {
        let mut connections = self.connections.write();
        let Some(route_connections) = connections.get_mut(route_chat_id) else {
            return;
        };
        route_connections.remove(connection_id);
        if route_connections.is_empty() {
            connections.remove(route_chat_id);
        }
    }

    pub fn set_route_agent(&self, route_chat_id: &str, agent_id: String) {
        self.route_agents
            .write()
            .insert(route_chat_id.to_string(), agent_id);
    }

    pub fn route_for_session(&self, agent_id: &str, session_id: &str) -> Option<RouteKey> {
        self.session_routes
            .read()
            .get(&session_key(agent_id, session_id))
            .cloned()
    }

    pub fn route_has_session(&self, route_chat_id: &str) -> bool {
        self.route_sessions.read().contains_key(route_chat_id)
    }

    pub fn route_session_id(&self, route_chat_id: &str) -> Option<String> {
        self.route_sessions
            .read()
            .get(route_chat_id)
            .and_then(|key| key.split_once(SESSION_KEY_SEPARATOR))
            .map(|(_, session_id)| session_id.to_string())
    }

    pub fn route_is_active(&self, route_chat_id: &str) -> bool {
        self.route_activity
            .read()
            .get(route_chat_id)
            .is_some_and(|activity| activity.active)
    }

    pub fn session_is_active(&self, agent_id: &str, session_id: &str) -> bool {
        self.route_for_session(agent_id, session_id)
            .as_ref()
            .is_some_and(|route| self.route_is_active(&route.chat_id))
    }

    pub fn mark_route_active(&self, route: &RouteKey) {
        let mut activity = self.route_activity.write();
        let entry = activity.entry(route.chat_id.clone()).or_default();
        entry.active = true;
        entry.generation = entry.generation.wrapping_add(1);
        drop(activity);
        self.send_turn_status(route, true);
    }

    pub fn mark_route_idle(&self, route: &RouteKey) -> WebRouteIdleDeadline {
        let mut activity = self.route_activity.write();
        let entry = activity.entry(route.chat_id.clone()).or_default();
        entry.active = false;
        entry.generation = entry.generation.wrapping_add(1);
        let generation = entry.generation;
        drop(activity);
        self.send_turn_status(route, false);
        WebRouteIdleDeadline {
            route: route.clone(),
            generation,
        }
    }

    pub fn bump_idle_route(&self, route: &RouteKey) -> Option<WebRouteIdleDeadline> {
        let mut activity = self.route_activity.write();
        let entry = activity.entry(route.chat_id.clone()).or_default();
        if entry.active {
            return None;
        }
        entry.generation = entry.generation.wrapping_add(1);
        Some(WebRouteIdleDeadline {
            route: route.clone(),
            generation: entry.generation,
        })
    }

    pub fn clear_pending_permission(&self, request_id: &str) {
        let Some(route_chat_id) = self.permission_routes.write().remove(request_id) else {
            return;
        };
        let mut pending = self.route_pending_permissions.write();
        let Some(route_pending) = pending.get_mut(&route_chat_id) else {
            return;
        };
        route_pending.remove(request_id);
        if route_pending.is_empty() {
            pending.remove(&route_chat_id);
        }
    }

    pub fn record_user_message(
        &self,
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
            .write()
            .entry(route.chat_id.clone())
            .or_default()
            .push(message);
    }

    pub fn schedule_idle_close(
        self: &Arc<Self>,
        workspace_threads: Arc<WorkspaceThreadManager>,
        deadline: WebRouteIdleDeadline,
    ) {
        let manager = Arc::clone(self);
        tokio::spawn(async move {
            tokio::time::sleep(WEB_ROUTE_IDLE_CLOSE_DELAY).await;
            if !manager.is_idle_deadline_current(&deadline) {
                return;
            }
            let _ = workspace_threads.detach_route(&deadline.route).await;
        });
    }

    pub fn sender(
        &self,
    ) -> (
        mpsc::UnboundedSender<ChannelOutput>,
        mpsc::UnboundedReceiver<ChannelOutput>,
    ) {
        mpsc::unbounded_channel()
    }

    pub fn dispatch_output(&self, output: ChannelOutput) -> Option<WebRouteIdleDeadline> {
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

    fn bind_route_session(&self, route: &RouteKey, session_id: &str) {
        let Some(agent_id) = self.route_agents.read().get(&route.chat_id).cloned() else {
            return;
        };
        let key = session_key(&agent_id, session_id);
        self.route_sessions
            .write()
            .insert(route.chat_id.clone(), key.clone());
        self.session_routes.write().insert(key, route.clone());
    }

    fn broadcast_output(&self, route_chat_id: &str, output: ChannelOutput) {
        self.push_route_history(route_chat_id, output.clone());

        let sinks = self
            .connections
            .read()
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

    fn push_route_history(&self, route_chat_id: &str, output: ChannelOutput) {
        if matches!(
            output,
            ChannelOutput::PermissionRequest { .. } | ChannelOutput::TurnStatus { .. }
        ) {
            return;
        }
        let mut history = self.route_history.write();
        let items = history.entry(route_chat_id.to_string()).or_default();
        items.push(output);
        if items.len() > MAX_ROUTE_HISTORY {
            let overflow = items.len() - MAX_ROUTE_HISTORY;
            items.drain(0..overflow);
        }
    }

    fn remember_pending_permission(
        &self,
        route_chat_id: &str,
        request_id: &str,
        output: &ChannelOutput,
    ) {
        self.permission_routes
            .write()
            .insert(request_id.to_string(), route_chat_id.to_string());
        self.route_pending_permissions
            .write()
            .entry(route_chat_id.to_string())
            .or_default()
            .insert(request_id.to_string(), output.clone());
    }

    fn clear_route_pending_permissions(&self, route_chat_id: &str) {
        let request_ids = {
            let Some(pending) = self.route_pending_permissions.write().remove(route_chat_id) else {
                return;
            };
            pending.keys().cloned().collect::<Vec<_>>()
        };
        let mut permission_routes = self.permission_routes.write();
        for request_id in request_ids {
            permission_routes.remove(&request_id);
        }
    }

    fn clear_route_pending_user_messages(&self, route_chat_id: &str) {
        self.route_pending_user_messages
            .write()
            .remove(route_chat_id);
    }

    fn take_pending_user_message_outputs(
        &self,
        route: &RouteKey,
        session_id: &str,
    ) -> Vec<ChannelOutput> {
        self.route_pending_user_messages
            .write()
            .remove(&route.chat_id)
            .unwrap_or_default()
            .into_iter()
            .flat_map(|message| user_message_outputs(route, session_id, message))
            .collect()
    }

    fn is_idle_deadline_current(&self, deadline: &WebRouteIdleDeadline) -> bool {
        self.route_activity
            .read()
            .get(&deadline.route.chat_id)
            .is_some_and(|activity| !activity.active && activity.generation == deadline.generation)
    }

    fn route_activity_state(&self, route_chat_id: &str) -> Option<bool> {
        self.route_activity
            .read()
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
            .read()
            .get(&route.chat_id)
            .map(|connections| connections.values().cloned().collect::<Vec<_>>())
            .unwrap_or_default();
        for sink in sinks {
            let _ = sink.send(output.clone());
        }
    }
}

fn session_key(agent_id: &str, session_id: &str) -> String {
    format!("{agent_id}{SESSION_KEY_SEPARATOR}{session_id}")
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

    #[test]
    fn register_connection_replays_active_turn_status_without_session() {
        let manager = WebChannelManager::new();
        let route = RouteKey::new("web", "chat-1");
        manager.mark_route_active(&route);

        let (tx, mut rx) = manager.sender();
        manager.register_connection(&route, "conn-1".to_string(), tx, true);

        assert_eq!(
            rx.try_recv().expect("turn status"),
            ChannelOutput::TurnStatus {
                route,
                active: true,
            }
        );
    }

    #[test]
    fn pending_user_message_replays_after_session_ready() {
        let manager = WebChannelManager::new();
        let route = RouteKey::new("web", "chat-1");
        manager.set_route_agent("chat-1", "codex".to_string());

        let (tx, mut rx) = manager.sender();
        manager.register_connection(&route, "conn-1".to_string(), tx, true);
        manager.record_user_message(
            &route,
            "msg-1".to_string(),
            vec![serde_json::json!({"type": "text", "text": "hello"})],
            true,
        );
        assert!(rx.try_recv().is_err());

        manager.dispatch_output(ChannelOutput::SessionReady {
            route: route.clone(),
            session_id: "sid-1".to_string(),
        });

        assert_eq!(
            rx.try_recv().expect("session ready"),
            ChannelOutput::SessionReady {
                route: route.clone(),
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
        manager.register_connection(&route, "conn-2".to_string(), tx, true);
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

    #[test]
    fn session_routes_keep_original_channel_kind() {
        let manager = WebChannelManager::new();
        let route = RouteKey::new("tui", "chat-1");
        manager.set_route_agent("chat-1", "codex".to_string());
        manager.mark_route_active(&route);

        manager.dispatch_output(ChannelOutput::SessionReady {
            route: route.clone(),
            session_id: "sid-1".to_string(),
        });

        assert_eq!(
            manager.route_for_session("codex", "sid-1"),
            Some(route.clone())
        );
        assert!(manager.session_is_active("codex", "sid-1"));
    }

    #[test]
    fn prompt_done_drops_unbound_pending_user_message() {
        let manager = WebChannelManager::new();
        let route = RouteKey::new("web", "chat-1");
        manager.set_route_agent("chat-1", "codex".to_string());

        let (tx, mut rx) = manager.sender();
        manager.register_connection(&route, "conn-1".to_string(), tx, true);
        manager.record_user_message(
            &route,
            "msg-1".to_string(),
            vec![serde_json::json!({"type": "text", "text": "hello"})],
            true,
        );
        manager.dispatch_output(ChannelOutput::PromptDone {
            route: route.clone(),
            message_id: Some("msg-1".to_string()),
        });
        while rx.try_recv().is_ok() {}

        manager.dispatch_output(ChannelOutput::SessionReady {
            route,
            session_id: "sid-1".to_string(),
        });

        assert!(matches!(
            rx.try_recv().expect("session ready"),
            ChannelOutput::SessionReady { .. }
        ));
        assert!(rx.try_recv().is_err());
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

    pub async fn send_output(&self, output: ChannelOutput) -> Result<(), String> {
        self.send_output_now(output)
    }

    pub async fn shutdown(&self) {}
}
