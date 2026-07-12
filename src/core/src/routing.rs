use std::fmt;
use std::sync::Arc;

use serde::{Deserialize, Deserializer, Serialize};
use tokio::sync::watch;

/// Channel kind identifier (e.g. "web", "telegram").
pub type ChannelKind = String;

/// One configured channel/Bot process instance (e.g. "slack-work").
pub type ChannelInstanceId = String;

/// Persisted compatibility name for `ChannelInstanceId` in `RouteKey::bot_id`.
/// New code should use `RouteKey::channel_instance_id()`.
pub type BotId = String;

/// Logical VibeAround actor addressed within a channel instance.
pub type ActorId = String;

/// Chat identifier within a channel.
pub type ChatId = String;

/// Optional platform topic/thread identifier within a chat.
pub type TopicId = String;

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DefaultWorkspaceKind {
    General,
    ChannelDefault,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChannelTraits {
    pub rehydratable_runtime: bool,
    pub startup_replay: bool,
    pub default_workspace: DefaultWorkspaceKind,
    pub rich_agent_events: bool,
}

pub fn channel_traits(channel_kind: &str) -> ChannelTraits {
    match channel_kind {
        "web" => ChannelTraits {
            rehydratable_runtime: true,
            startup_replay: true,
            default_workspace: DefaultWorkspaceKind::General,
            rich_agent_events: true,
        },
        "tui" => ChannelTraits {
            rehydratable_runtime: true,
            startup_replay: true,
            default_workspace: DefaultWorkspaceKind::ChannelDefault,
            rich_agent_events: true,
        },
        _ => ChannelTraits {
            // IM routes keep their WorkspaceThread attachment across host
            // idle shutdown. Rehydrate the runtime on the next message so
            // agent/profile/session continuity is preserved without replaying
            // old output into the chat.
            rehydratable_runtime: true,
            startup_replay: false,
            default_workspace: DefaultWorkspaceKind::ChannelDefault,
            rich_agent_events: false,
        },
    }
}

/// Stable route key for a conversation path through a channel.
///
/// `(channel_kind, bot_id, chat_id, actor_id, topic_id)` identifies a logical
/// actor conversation. Legacy routes omit `actor_id` and `topic_id`.
///
/// The persisted `bot_id` field carries the stable channel instance ID and
/// defaults to `channel_kind` for legacy routes.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
pub struct RouteKey {
    pub channel_kind: ChannelKind,
    /// Stable channel process/config instance. The field name is retained for
    /// persisted-route compatibility; new code should use
    /// [`RouteKey::channel_instance_id`].
    #[serde(default)]
    pub bot_id: BotId,
    pub chat_id: ChatId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actor_id: Option<ActorId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub topic_id: Option<TopicId>,
}

impl<'de> Deserialize<'de> for RouteKey {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct RouteKeyWire {
            channel_kind: ChannelKind,
            #[serde(default)]
            bot_id: Option<BotId>,
            chat_id: ChatId,
            #[serde(default)]
            actor_id: Option<ActorId>,
            #[serde(default)]
            topic_id: Option<TopicId>,
        }

        let wire = RouteKeyWire::deserialize(deserializer)?;
        let bot_id = wire
            .bot_id
            .filter(|bot_id| !bot_id.trim().is_empty())
            .unwrap_or_else(|| wire.channel_kind.clone());
        let actor_id = wire
            .actor_id
            .filter(|actor_id| !actor_id.trim().is_empty() && actor_id != &bot_id);
        let topic_id = wire.topic_id.filter(|topic_id| !topic_id.trim().is_empty());

        Ok(Self {
            channel_kind: wire.channel_kind,
            bot_id,
            chat_id: wire.chat_id,
            actor_id,
            topic_id,
        })
    }
}

impl RouteKey {
    pub fn new(channel_kind: impl Into<ChannelKind>, chat_id: impl Into<ChatId>) -> Self {
        let ck: ChannelKind = channel_kind.into();
        Self {
            bot_id: ck.clone(),
            channel_kind: ck,
            chat_id: chat_id.into(),
            actor_id: None,
            topic_id: None,
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
            actor_id: None,
            topic_id: None,
        }
    }

    pub fn with_actor(
        channel_kind: impl Into<ChannelKind>,
        channel_instance_id: impl Into<BotId>,
        chat_id: impl Into<ChatId>,
        actor_id: impl Into<ActorId>,
        topic_id: Option<TopicId>,
    ) -> Self {
        let channel_instance_id = channel_instance_id.into();
        let actor_id = actor_id.into();
        Self {
            channel_kind: channel_kind.into(),
            bot_id: channel_instance_id.clone(),
            chat_id: chat_id.into(),
            actor_id: (actor_id != channel_instance_id).then_some(actor_id),
            topic_id: topic_id.filter(|topic_id| !topic_id.trim().is_empty()),
        }
    }

    pub fn channel_instance_id(&self) -> &str {
        &self.bot_id
    }

    pub fn actor_id(&self) -> Option<&str> {
        self.actor_id.as_deref()
    }

    pub fn topic_id(&self) -> Option<&str> {
        self.topic_id.as_deref()
    }

    /// Lossy display/API key: `channel_kind:chat_id` (backward compat).
    ///
    /// This intentionally does NOT include `bot_id`, `actor_id`, or `topic_id`.
    /// Do not use this as a persisted route identity; serialize the full
    /// `RouteKey` instead.
    pub fn as_key(&self) -> String {
        if self.bot_id != self.channel_kind || self.actor_id.is_some() || self.topic_id.is_some() {
            tracing::warn!(
                channel_kind = %self.channel_kind,
                bot_id = %self.bot_id,
                chat_id = %self.chat_id,
                actor_id = ?self.actor_id,
                topic_id = ?self.topic_id,
                "RouteKey::as_key is lossy for an extended route"
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

/// Ephemeral delivery target for one inbound turn.
///
/// `reply_to` belongs to one platform message and must never be persisted in
/// [`RouteKey`] or used to select a workspace thread.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChannelTarget {
    pub route: RouteKey,
    pub reply_to: Option<MessageId>,
}

impl ChannelTarget {
    pub fn new(route: RouteKey, reply_to: Option<MessageId>) -> Self {
        Self { route, reply_to }
    }

    pub fn for_route(route: RouteKey) -> Self {
        Self::new(route, None)
    }
}

/// Shared, cancellation-safe pointer to the target of the currently running
/// turn on one workspace thread.
#[derive(Clone)]
pub struct ActiveTurnTarget {
    state: Arc<ActiveTurnTargetState>,
}

struct ActiveTurnTargetState {
    current: watch::Sender<Option<ActiveTurn>>,
}

#[derive(Clone)]
struct ActiveTurn {
    token: Arc<()>,
    target: ChannelTarget,
    cancel_tx: watch::Sender<bool>,
}

impl Default for ActiveTurnTarget {
    fn default() -> Self {
        let (current, _) = watch::channel(None);
        Self {
            state: Arc::new(ActiveTurnTargetState { current }),
        }
    }
}

impl ActiveTurnTarget {
    pub fn install(&self, target: ChannelTarget) -> ActiveTurnTargetGuard {
        let (cancel_tx, _) = watch::channel(false);
        let previous_cancel = self
            .state
            .current
            .borrow()
            .as_ref()
            .map(|previous| previous.cancel_tx.clone());
        if let Some(previous_cancel) = previous_cancel {
            previous_cancel.send_replace(true);
        }
        let token = Arc::new(());
        self.state.current.send_replace(Some(ActiveTurn {
            token: Arc::clone(&token),
            target,
            cancel_tx: cancel_tx.clone(),
        }));
        ActiveTurnTargetGuard {
            owner: self.clone(),
            token,
            cancel_tx,
        }
    }

    pub fn current(&self) -> Option<ChannelTarget> {
        self.state
            .current
            .borrow()
            .as_ref()
            .map(|current| current.target.clone())
    }

    pub(crate) fn current_with_cancellation(
        &self,
    ) -> Option<(ChannelTarget, watch::Receiver<bool>)> {
        self.state
            .current
            .borrow()
            .as_ref()
            .map(|current| (current.target.clone(), current.cancel_tx.subscribe()))
    }

    pub(crate) fn cancel_current(&self) {
        if let Some(current) = self.state.current.borrow().as_ref() {
            current.cancel_tx.send_replace(true);
        }
    }
}

pub struct ActiveTurnTargetGuard {
    owner: ActiveTurnTarget,
    token: Arc<()>,
    cancel_tx: watch::Sender<bool>,
}

impl ActiveTurnTargetGuard {
    pub(crate) fn cancellation(&self) -> watch::Receiver<bool> {
        self.cancel_tx.subscribe()
    }
}

pub(crate) async fn wait_for_signal(signal: &mut watch::Receiver<bool>) {
    while !*signal.borrow_and_update() {
        if signal.changed().await.is_err() {
            return;
        }
    }
}

impl Drop for ActiveTurnTargetGuard {
    fn drop(&mut self) {
        self.cancel_tx.send_replace(true);
        self.owner.state.current.send_if_modified(|current| {
            if current
                .as_ref()
                .is_some_and(|active| Arc::ptr_eq(&active.token, &self.token))
            {
                *current = None;
                true
            } else {
                false
            }
        });
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
    fn channel_traits_capture_current_surface_capabilities() {
        let web = channel_traits("web");
        assert!(web.rehydratable_runtime);
        assert!(web.startup_replay);
        assert_eq!(web.default_workspace, DefaultWorkspaceKind::General);
        assert!(web.rich_agent_events);

        let tui = channel_traits("tui");
        assert!(tui.rehydratable_runtime);
        assert!(tui.startup_replay);
        assert_eq!(tui.default_workspace, DefaultWorkspaceKind::ChannelDefault);
        assert!(tui.rich_agent_events);

        let im = channel_traits("feishu");
        assert!(im.rehydratable_runtime);
        assert!(!im.startup_replay);
        assert_eq!(im.default_workspace, DefaultWorkspaceKind::ChannelDefault);
        assert!(!im.rich_agent_events);
    }

    #[test]
    fn route_key_legacy_key_round_trips_default_bot_id() {
        let route = RouteKey::new("feishu", "chat-1");

        assert_eq!(RouteKey::from_key(&route.as_key()), Some(route));
    }

    #[test]
    fn active_turn_target_guard_clears_on_drop() {
        let active = ActiveTurnTarget::default();
        let target = ChannelTarget::new(
            RouteKey::with_actor("slack", "slack-work", "chat-1", "U1", None),
            Some("message-1".to_string()),
        );
        {
            let _guard = active.install(target.clone());
            assert_eq!(active.current(), Some(target));
        }
        assert_eq!(active.current(), None);
    }

    #[test]
    fn older_active_turn_guard_cannot_clear_a_new_generation() {
        let active = ActiveTurnTarget::default();
        let first = active.install(ChannelTarget::for_route(RouteKey::new("web", "one")));
        let (_, first_cancelled) = active
            .current_with_cancellation()
            .expect("first active target");
        let second_target = ChannelTarget::for_route(RouteKey::new("web", "two"));
        let second = active.install(second_target.clone());

        assert!(*first_cancelled.borrow());
        drop(first);
        assert_eq!(active.current(), Some(second_target));
        drop(second);
        assert_eq!(active.current(), None);
    }

    #[tokio::test]
    async fn active_turn_cancel_signal_survives_until_waiters_observe_it() {
        let active = ActiveTurnTarget::default();
        let _guard = active.install(ChannelTarget::for_route(RouteKey::new("web", "one")));
        let (_, mut cancelled) = active.current_with_cancellation().expect("active target");

        active.cancel_current();

        cancelled
            .wait_for(|is_cancelled| *is_cancelled)
            .await
            .expect("turn cancellation sender remains live");
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

    #[test]
    fn route_key_deserialization_defaults_missing_or_empty_bot_id() {
        let missing: RouteKey = serde_json::from_value(serde_json::json!({
            "channel_kind": "feishu",
            "chat_id": "chat-1"
        }))
        .expect("legacy route without bot_id parses");
        let empty: RouteKey = serde_json::from_value(serde_json::json!({
            "channel_kind": "feishu",
            "bot_id": "",
            "chat_id": "chat-1"
        }))
        .expect("legacy route with empty bot_id parses");

        assert_eq!(missing.bot_id, "feishu");
        assert_eq!(empty.bot_id, "feishu");
    }

    #[test]
    fn route_key_distinguishes_actor_and_topic() {
        let reviewer = RouteKey::with_actor(
            "feishu",
            "feishu-primary",
            "chat-1",
            "codex-reviewer",
            Some("topic-1".to_string()),
        );
        let builder = RouteKey::with_actor(
            "feishu",
            "feishu-primary",
            "chat-1",
            "codex-builder",
            Some("topic-1".to_string()),
        );
        let other_topic = RouteKey::with_actor(
            "feishu",
            "feishu-primary",
            "chat-1",
            "codex-reviewer",
            Some("topic-2".to_string()),
        );

        assert_ne!(reviewer, builder);
        assert_ne!(reviewer, other_topic);
        assert_eq!(reviewer.channel_instance_id(), "feishu-primary");
        assert_eq!(reviewer.actor_id(), Some("codex-reviewer"));
        assert_eq!(reviewer.topic_id(), Some("topic-1"));
    }

    #[test]
    fn default_actor_context_preserves_legacy_route_identity() {
        let legacy = RouteKey::new("feishu", "chat-1");
        let routed = RouteKey::with_actor("feishu", "feishu", "chat-1", "feishu", None);

        assert_eq!(routed, legacy);
        assert_eq!(routed.actor_id(), None);
    }
}
