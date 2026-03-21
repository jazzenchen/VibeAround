use std::sync::Arc;

use dashmap::DashMap;
use tokio::sync::mpsc;

use crate::hub::types::ChannelNotification;

/// Outbound sink to a single web chat connection.
pub type WebChatSink = mpsc::UnboundedSender<ChannelNotification>;

/// Internal web channel manager.
///
/// One manager owns many chat connections keyed by chat_id.
pub struct WebChannelManager {
    connections: DashMap<String, WebChatSink>,
}

impl WebChannelManager {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            connections: DashMap::new(),
        })
    }

    pub fn register_connection(&self, chat_id: String, sink: WebChatSink) {
        self.connections.insert(chat_id, sink);
    }

    pub fn unregister_connection(&self, chat_id: &str) {
        self.connections.remove(chat_id);
    }

    pub fn sender(&self) -> (mpsc::UnboundedSender<ChannelNotification>, mpsc::UnboundedReceiver<ChannelNotification>) {
        mpsc::unbounded_channel()
    }

    pub fn dispatch_notification(&self, notif: ChannelNotification) {
        let chat_id = chat_id_of_notification(&notif);
        if let Some(entry) = self.connections.get(chat_id) {
            let _ = entry.send(notif);
        }
    }
}

fn chat_id_of_notification(notif: &ChannelNotification) -> &str {
    match notif {
        ChannelNotification::AgentStart { chat_id, .. } => chat_id,
        ChannelNotification::AgentThinking { chat_id, .. } => chat_id,
        ChannelNotification::AgentToken { chat_id, .. } => chat_id,
        ChannelNotification::AgentToolUse { chat_id, .. } => chat_id,
        ChannelNotification::AgentToolResult { chat_id, .. } => chat_id,
        ChannelNotification::AgentEnd { chat_id, .. } => chat_id,
        ChannelNotification::AgentError { chat_id, .. } => chat_id,
        ChannelNotification::SendText { chat_id, .. } => chat_id,
    }
}
