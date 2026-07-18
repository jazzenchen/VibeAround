//! `PluginAgentHandler` — host-side behavior for ACP channel plugins.
//! Drives the prompt lifecycle and routes extension notifications back into
//! the host.

use std::sync::Arc;

use tokio::sync::mpsc;

use agent_client_protocol::schema::v1 as acp;
use agent_client_protocol::schema::ProtocolVersion;

use super::super::manifest::ChannelPluginManifest;
use super::super::plugin_host::PluginHost;
use super::super::types::{ChannelInboundContext, CHANNEL_CONTEXT_META_KEY};
use super::super::{ChannelEnvelope, ChannelInput, ConversationIngress};
use crate::plugins::TopicConversationScope;
use crate::proc_log;
use crate::process::registry::ProcessKind;
use crate::routing::{ChannelTarget, RouteKey};

fn route_for_prompt(
    channel_kind: &str,
    channel_instance_id: &str,
    session_chat_id: &str,
    meta: Option<&serde_json::Map<String, serde_json::Value>>,
) -> Result<RouteKey, String> {
    target_for_prompt(channel_kind, channel_instance_id, session_chat_id, meta)
        .map(|target| target.route)
}

fn target_for_prompt(
    channel_kind: &str,
    channel_instance_id: &str,
    session_chat_id: &str,
    meta: Option<&serde_json::Map<String, serde_json::Value>>,
) -> Result<ChannelTarget, String> {
    let value = meta
        .and_then(|meta| meta.get(CHANNEL_CONTEXT_META_KEY))
        .ok_or_else(|| format!("missing {CHANNEL_CONTEXT_META_KEY} metadata"))?;

    let context: ChannelInboundContext = serde_json::from_value(value.clone())
        .map_err(|error| format!("invalid {CHANNEL_CONTEXT_META_KEY} metadata: {error}"))?;
    if context.channel_instance_id.trim().is_empty() {
        return Err("channelInstanceId must not be empty".into());
    }
    if context.channel_instance_id != channel_instance_id {
        return Err("channelInstanceId must match the supervised plugin instance".into());
    }
    if context.actor_id.trim().is_empty() {
        return Err("actorId must not be empty".into());
    }
    if context.chat_id.trim().is_empty() {
        return Err("chatId must not be empty".into());
    }
    if context.chat_id != session_chat_id {
        return Err("va.channel chatId must match the ACP sessionId".into());
    }
    if !context.is_prompt_allowed() {
        return Err("group prompts must explicitly address the actor".into());
    }

    let reply_to = context
        .platform_message_id
        .as_deref()
        .map(str::trim)
        .filter(|message_id| !message_id.is_empty())
        .map(ToOwned::to_owned);
    Ok(ChannelTarget::new(
        RouteKey::with_actor(
            channel_kind,
            context.channel_instance_id,
            context.chat_id,
            context.actor_id,
            context.topic_id,
        ),
        reply_to,
    ))
}

fn route_for_callback(
    channel_kind: &str,
    channel_instance_id: &str,
    chat_id: &str,
    params: &serde_json::Map<String, serde_json::Value>,
) -> Result<RouteKey, String> {
    let context = params
        .get(CHANNEL_CONTEXT_META_KEY)
        .or_else(|| params.get("context"))
        .ok_or_else(|| format!("missing {CHANNEL_CONTEXT_META_KEY} metadata"))?;
    let meta =
        serde_json::Map::from_iter([(CHANNEL_CONTEXT_META_KEY.to_string(), context.clone())]);
    route_for_prompt(channel_kind, channel_instance_id, chat_id, Some(&meta))
}

fn route_for_cancel(
    channel_kind: &str,
    channel_instance_id: &str,
    session_chat_id: &str,
    meta: Option<&serde_json::Map<String, serde_json::Value>>,
) -> Result<RouteKey, String> {
    route_for_prompt(channel_kind, channel_instance_id, session_chat_id, meta)
}

/// ACP Agent handler for a channel plugin. `prompt()` calls through the
/// shared conversation ingress — blocks until the turn completes and
/// returns the real `PromptResponse` with `StopReason`.
pub(super) struct PluginAgentHandler {
    channel_kind: String,
    channel_instance_id: String,
    default_actor_id: String,
    topic_scope: TopicConversationScope,
    config: serde_json::Value,
    /// Used for fire-and-forget callback notifications.
    input_tx: mpsc::UnboundedSender<ChannelInput>,
    ingress: Arc<ConversationIngress>,
    plugin_host: Arc<PluginHost>,
}

impl PluginAgentHandler {
    pub(super) fn new(
        manifest: ChannelPluginManifest,
        input_tx: mpsc::UnboundedSender<ChannelInput>,
        ingress: Arc<ConversationIngress>,
        plugin_host: Arc<PluginHost>,
    ) -> Self {
        let ChannelPluginManifest {
            channel_kind,
            instance_id: channel_instance_id,
            actor_id: default_actor_id,
            topic_scope,
            raw_config: config,
            ..
        } = manifest;
        Self {
            channel_kind,
            channel_instance_id,
            default_actor_id,
            topic_scope,
            config,
            input_tx,
            ingress,
            plugin_host,
        }
    }
    pub(super) async fn initialize(
        &self,
        _args: acp::InitializeRequest,
    ) -> acp::Result<acp::InitializeResponse> {
        proc_log!(
            info,
            kind = ProcessKind::ChannelPlugin,
            label = self.channel_kind,
            event = "acp_initialize"
        );

        let mut meta = serde_json::Map::new();
        meta.insert("channelKind".into(), self.channel_kind.clone().into());
        meta.insert(
            "channelInstanceId".into(),
            self.channel_instance_id.clone().into(),
        );
        meta.insert("actorId".into(), self.default_actor_id.clone().into());
        meta.insert("config".into(), self.config.clone());
        meta.insert("hostVersion".into(), env!("CARGO_PKG_VERSION").into());
        meta.insert("promptDone".into(), true.into());
        meta.insert(
            "cacheDir".into(),
            crate::config::data_dir()
                .join(".cache")
                .to_string_lossy()
                .into(),
        );

        Ok(acp::InitializeResponse::new(ProtocolVersion::V1)
            .agent_info(
                acp::Implementation::new("vibearound-host", env!("CARGO_PKG_VERSION"))
                    .title("VibeAround"),
            )
            .meta(meta))
    }

    pub(super) async fn prompt(
        &self,
        args: acp::PromptRequest,
    ) -> acp::Result<acp::PromptResponse> {
        let chat_id = args.session_id.to_string();
        let mut target = target_for_prompt(
            &self.channel_kind,
            &self.channel_instance_id,
            &chat_id,
            args.meta.as_ref(),
        )
        .map_err(|error| {
            tracing::warn!(
                channel_kind = %self.channel_kind,
                chat_id = %chat_id,
                error = %error,
                "rejected channel prompt metadata"
            );
            acp::Error::invalid_params()
        })?;
        apply_topic_scope(self.topic_scope, &mut target.route);

        let content_blocks = args.prompt;

        if content_blocks.is_empty() {
            return Err(acp::Error::invalid_params());
        }

        // Extract text preview for logging
        let text_preview: String = content_blocks
            .iter()
            .find_map(|b| match b {
                acp::ContentBlock::Text(t) => Some(t.text.clone()),
                _ => None,
            })
            .unwrap_or_default();

        tracing::info!(
            "[{}] ACP prompt chat_id={} blocks={} text_preview={}",
            self.channel_kind,
            chat_id,
            content_blocks.len(),
            text_preview.chars().take(80).collect::<String>()
        );

        // The shared ingress blocks until the turn completes.
        // Session notifications stream to the plugin via ChannelBridgeHandler
        // → PluginHost → output_tx → output forwarder → conn.session_notification().
        self.ingress.prompt(target, content_blocks).await
    }

    pub(super) async fn cancel(&self, args: acp::CancelNotification) -> acp::Result<()> {
        let chat_id = args.session_id.to_string();
        let mut route = route_for_cancel(
            &self.channel_kind,
            &self.channel_instance_id,
            &chat_id,
            args.meta.as_ref(),
        )
        .map_err(|error| {
            tracing::warn!(
                channel_kind = %self.channel_kind,
                chat_id = %chat_id,
                error = %error,
                "rejected channel cancel metadata"
            );
            acp::Error::invalid_params()
        })?;
        apply_topic_scope(self.topic_scope, &mut route);

        proc_log!(
            info,
            kind = ProcessKind::ChannelPlugin,
            label = self.channel_kind,
            event = "acp_cancel",
            chat_id = %chat_id
        );

        self.ingress.dispatch(ChannelInput::Stop { route });
        Ok(())
    }

    pub(super) async fn ext_notification(&self, args: acp::ExtNotification) -> acp::Result<()> {
        // Rust ACP SDK already strips the "_" prefix before dispatching here.
        let method = args.method.to_string();
        let params: serde_json::Value = serde_json::from_str(args.params.get())
            .unwrap_or(serde_json::Value::Object(serde_json::Map::new()));
        let params_obj = params.as_object().cloned().unwrap_or_default();

        match method.as_str() {
            "va/heartbeat" => {
                super::super::monitor::touch_weak(
                    &self.plugin_host.monitor_weak(),
                    &self.channel_instance_id,
                );
            }
            "va/callback" => {
                let chat_id = params_obj
                    .get("chatId")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let mut route = route_for_callback(
                    &self.channel_kind,
                    &self.channel_instance_id,
                    chat_id,
                    &params_obj,
                )
                .map_err(|error| {
                    tracing::warn!(
                        channel_kind = %self.channel_kind,
                        chat_id,
                        error = %error,
                        "rejected channel callback metadata"
                    );
                    acp::Error::invalid_params()
                })?;
                apply_topic_scope(self.topic_scope, &mut route);
                let action_value = params_obj
                    .get("data")
                    .and_then(|v| v.as_str())
                    .map(String::from);

                let input = ChannelInput::Callback {
                    envelope: ChannelEnvelope {
                        route,
                        message_id: params_obj
                            .get("messageId")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string(),
                        turn_id: None,
                        text: String::new(),
                        sender_id: params_obj
                            .get("sender")
                            .and_then(|v| v.get("id"))
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string(),
                        attachments: vec![],
                        parent_id: None,
                        cli_kind: None,
                    },
                    action_value,
                };
                let _ = self.input_tx.send(input);
            }
            other => {
                tracing::info!(
                    "[{}] unhandled ext_notification: {}",
                    self.channel_kind,
                    other
                );
            }
        }
        Ok(())
    }

    pub(super) async fn ext_method(&self, args: acp::ExtRequest) -> acp::Result<acp::ExtResponse> {
        let method = args.method.to_string();
        proc_log!(
            info,
            kind = ProcessKind::ChannelPlugin,
            label = self.channel_kind,
            event = "unhandled_ext_method",
            method = %method
        );
        Err(acp::Error::method_not_found())
    }
}

fn apply_topic_scope(scope: TopicConversationScope, route: &mut RouteKey) {
    if scope == TopicConversationScope::Chat {
        route.topic_id = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn chat_topic_scope_collapses_topic_identity() {
        let mut route = RouteKey::with_actor(
            "slack",
            "slack-primary",
            "chat-1",
            "slack-primary",
            Some("thread-1".to_string()),
        );

        apply_topic_scope(TopicConversationScope::Chat, &mut route);

        assert_eq!(route.topic_id(), None);
    }

    #[test]
    fn topic_scope_preserves_topic_identity() {
        let mut route = RouteKey::with_actor(
            "slack",
            "slack-primary",
            "chat-1",
            "slack-primary",
            Some("thread-1".to_string()),
        );

        apply_topic_scope(TopicConversationScope::Topic, &mut route);

        assert_eq!(route.topic_id(), Some("thread-1"));
    }

    fn channel_meta(value: serde_json::Value) -> serde_json::Map<String, serde_json::Value> {
        serde_json::Map::from_iter([(CHANNEL_CONTEXT_META_KEY.to_string(), value)])
    }

    #[test]
    fn prompt_without_route_metadata_is_rejected() {
        assert!(route_for_prompt("feishu", "feishu", "chat-1", None).is_err());
    }

    #[test]
    fn routed_prompt_uses_full_channel_context() {
        let meta = channel_meta(json!({
            "channelInstanceId": "feishu-primary",
            "actorId": "codex-reviewer",
            "chatId": "chat-1",
            "topicId": "topic-1",
            "platformMessageId": "message-1",
            "scope": "group",
            "addressedBy": "mention"
        }));

        let target = target_for_prompt("feishu", "feishu-primary", "chat-1", Some(&meta))
            .expect("routed prompt metadata parses");
        let route = target.route;

        assert_eq!(route.channel_instance_id(), "feishu-primary");
        assert_eq!(route.chat_id, "chat-1");
        assert_eq!(route.actor_id(), Some("codex-reviewer"));
        assert_eq!(route.topic_id(), Some("topic-1"));
        assert_eq!(target.reply_to.as_deref(), Some("message-1"));
    }

    #[test]
    fn routed_prompt_ignores_an_empty_platform_message_id() {
        let meta = channel_meta(json!({
            "channelInstanceId": "feishu-primary",
            "actorId": "codex-reviewer",
            "chatId": "chat-1",
            "platformMessageId": "  ",
            "scope": "dm",
            "addressedBy": "dm"
        }));

        let target = target_for_prompt("feishu", "feishu-primary", "chat-1", Some(&meta))
            .expect("routed prompt metadata parses");

        assert_eq!(target.reply_to, None);
    }

    #[test]
    fn routed_group_prompt_rejects_unaddressed_message() {
        let meta = channel_meta(json!({
            "channelInstanceId": "feishu-primary",
            "actorId": "codex-reviewer",
            "chatId": "chat-1",
            "scope": "group",
            "addressedBy": "unaddressed"
        }));

        let error = route_for_prompt("feishu", "feishu-primary", "chat-1", Some(&meta))
            .expect_err("unaddressed group message must be rejected");

        assert_eq!(error, "group prompts must explicitly address the actor");
    }

    #[test]
    fn routed_group_prompt_rejects_bare_command() {
        let meta = channel_meta(json!({
            "channelInstanceId": "feishu-primary",
            "actorId": "codex-reviewer",
            "chatId": "chat-1",
            "scope": "group",
            "addressedBy": "command"
        }));

        let error = route_for_prompt("feishu", "feishu-primary", "chat-1", Some(&meta))
            .expect_err("a bare group command does not identify the target bot");

        assert_eq!(error, "group prompts must explicitly address the actor");
    }

    #[test]
    fn routed_group_callback_is_an_explicit_bot_interaction() {
        let meta = channel_meta(json!({
            "channelInstanceId": "feishu-primary",
            "actorId": "codex-reviewer",
            "chatId": "chat-1",
            "scope": "group",
            "addressedBy": "callback"
        }));

        let route = route_for_prompt("feishu", "feishu-primary", "chat-1", Some(&meta))
            .expect("a callback targets the bot that created the action");

        assert_eq!(route.actor_id(), Some("codex-reviewer"));
    }

    #[test]
    fn callback_metadata_preserves_extended_route() {
        let params = serde_json::Map::from_iter([(
            "context".to_string(),
            json!({
                "channelInstanceId": "feishu-primary",
                "actorId": "codex-reviewer",
                "chatId": "chat-1",
                "topicId": "topic-1",
                "scope": "group",
                "addressedBy": "callback"
            }),
        )]);

        let route = route_for_callback("feishu", "feishu-primary", "chat-1", &params)
            .expect("callback context should parse");

        assert_eq!(route.channel_instance_id(), "feishu-primary");
        assert_eq!(route.actor_id(), Some("codex-reviewer"));
        assert_eq!(route.topic_id(), Some("topic-1"));
    }

    #[test]
    fn cancel_metadata_targets_only_the_addressed_actor_route() {
        let meta = channel_meta(json!({
            "channelInstanceId": "slack-primary",
            "actorId": "codex-reviewer",
            "chatId": "group-1",
            "topicId": "thread-1",
            "scope": "group",
            "addressedBy": "mention"
        }));

        let route = route_for_cancel("slack", "slack-primary", "group-1", Some(&meta))
            .expect("targeted cancel metadata parses");

        assert_eq!(route.channel_instance_id(), "slack-primary");
        assert_eq!(route.actor_id(), Some("codex-reviewer"));
        assert_eq!(route.topic_id(), Some("thread-1"));
    }

    #[test]
    fn cancel_without_route_metadata_is_rejected() {
        assert!(route_for_cancel("slack", "slack", "group-1", None).is_err());
    }

    #[test]
    fn routed_dm_prompt_allows_unaddressed_message() {
        let meta = channel_meta(json!({
            "channelInstanceId": "feishu-primary",
            "actorId": "codex-reviewer",
            "chatId": "chat-1",
            "scope": "dm",
            "addressedBy": "unaddressed"
        }));

        let route = route_for_prompt("feishu", "feishu-primary", "chat-1", Some(&meta))
            .expect("direct messages do not require an explicit mention");

        assert_eq!(route.actor_id(), Some("codex-reviewer"));
    }

    #[test]
    fn routed_prompt_rejects_a_spoofed_channel_instance() {
        let meta = channel_meta(json!({
            "channelInstanceId": "slack-other",
            "actorId": "U123",
            "chatId": "chat-1",
            "scope": "dm",
            "addressedBy": "dm"
        }));

        let error = route_for_prompt("slack", "slack-work", "chat-1", Some(&meta))
            .expect_err("a plugin connection cannot impersonate another instance");

        assert_eq!(
            error,
            "channelInstanceId must match the supervised plugin instance"
        );
    }
}
