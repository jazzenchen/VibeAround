use std::fmt;

use serde::{Deserialize, Serialize};

/// Channel kind identifier (e.g. "web", "telegram").
pub type ChannelKind = String;

/// Bot identity on the IM platform (e.g. Feishu botOpenId, Telegram bot username).
pub type BotId = String;

/// Chat identifier within a channel.
pub type ChatId = String;

/// Platform envelope identifier.
pub type MessageId = String;

/// ACP/provider session identifier.
pub type SessionId = String;

/// External CLI session identifier.
pub type CliSessionId = String;

/// Runtime instance identifier for a route.
pub type RuntimeId = String;

/// Logical turn identifier on a route.
pub type TurnId = String;

/// Stable route key for a conversation path through a channel.
///
/// The triple `(channel_kind, bot_id, chat_id)` uniquely identifies a bot
/// instance in a chat. This supports group chats with multiple bots — each
/// bot has its own route.
///
/// `bot_id` defaults to `channel_kind` for backward compat with plugins
/// that haven't reported their IM identity yet.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct RouteKey {
    pub channel_kind: ChannelKind,
    /// Bot identity on the IM platform. Defaults to `channel_kind`.
    /// Each plugin process represents one bot; future multi-bot support
    /// would use separate plugin processes with distinct bot_id values.
    #[serde(default)]
    pub bot_id: BotId,
    pub chat_id: ChatId,
}

impl RouteKey {
    pub fn new(channel_kind: impl Into<ChannelKind>, chat_id: impl Into<ChatId>) -> Self {
        let ck: ChannelKind = channel_kind.into();
        Self {
            bot_id: ck.clone(),
            channel_kind: ck,
            chat_id: chat_id.into(),
        }
    }

    pub fn with_bot_id(
        channel_kind: impl Into<ChannelKind>,
        bot_id: impl Into<BotId>,
        chat_id: impl Into<ChatId>,
    ) -> Self {
        Self {
            channel_kind: channel_kind.into(),
            bot_id: bot_id.into(),
            chat_id: chat_id.into(),
        }
    }

    /// Lossy display/API key: `channel_kind:chat_id` (backward compat).
    ///
    /// This intentionally does NOT include `bot_id`. Do not use this as a
    /// persisted identity for multi-bot routes; serialize the full `RouteKey`
    /// instead.
    pub fn as_key(&self) -> String {
        if self.bot_id != self.channel_kind {
            tracing::warn!(
                channel_kind = %self.channel_kind,
                bot_id = %self.bot_id,
                chat_id = %self.chat_id,
                "RouteKey::as_key is lossy for non-default bot_id"
            );
        }
        format!("{}:{}", self.channel_kind, self.chat_id)
    }

    /// Parse the legacy lossy `channel_kind:chat_id` key.
    ///
    /// `bot_id` is restored to its default (`channel_kind`) because this key
    /// format has never carried a bot identity.
    pub fn from_key(key: &str) -> Option<Self> {
        let (channel_kind, chat_id) = key.split_once(':')?;
        Some(Self::new(channel_kind, chat_id))
    }
}

impl fmt::Display for RouteKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.channel_kind, self.chat_id)
    }
}

/// Attachment metadata carried on channel envelopes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Attachment {
    pub message_id: String,
    pub file_key: String,
    pub file_name: String,
    pub resource_type: String,
    #[serde(default)]
    pub size: Option<i64>,
}

pub fn is_external_attachment_uri(file_key: &str) -> bool {
    file_key.starts_with("file://")
        || file_key.starts_with("http://")
        || file_key.starts_with("https://")
}

pub fn is_safe_attachment_file_key(file_key: &str) -> bool {
    if is_external_attachment_uri(file_key) {
        return true;
    }
    !file_key.is_empty()
        && file_key != "."
        && file_key != ".."
        && !file_key.contains('/')
        && !file_key.contains('\\')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn attachment_file_key_rejects_path_traversal_segments() {
        assert!(is_safe_attachment_file_key("upload_123"));
        assert!(is_safe_attachment_file_key("file:///tmp/report.md"));
        assert!(is_safe_attachment_file_key("https://example.com/report.md"));

        assert!(!is_safe_attachment_file_key(""));
        assert!(!is_safe_attachment_file_key("."));
        assert!(!is_safe_attachment_file_key(".."));
        assert!(!is_safe_attachment_file_key("../secret"));
        assert!(!is_safe_attachment_file_key("nested/file"));
        assert!(!is_safe_attachment_file_key(r"nested\file"));
    }

    #[test]
    fn route_key_legacy_key_round_trips_default_bot_id() {
        let route = RouteKey::new("feishu", "chat-1");

        assert_eq!(RouteKey::from_key(&route.as_key()), Some(route));
    }

    #[test]
    fn route_key_legacy_key_is_lossy_for_custom_bot_id() {
        let route = RouteKey::with_bot_id("feishu", "bot-a", "chat-1");
        let parsed = RouteKey::from_key(&route.as_key()).expect("legacy key parses");

        assert_eq!(parsed.channel_kind, "feishu");
        assert_eq!(parsed.bot_id, "feishu");
        assert_eq!(parsed.chat_id, "chat-1");
        assert_ne!(parsed, route);
    }
}
