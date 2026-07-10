//! `PluginAgentHandler` — host-side behavior for ACP channel plugins.
//! Drives the prompt lifecycle and routes extension notifications back into
//! the host.

use std::sync::Arc;

use tokio::sync::mpsc;

use agent_client_protocol::schema::v1 as acp;
use agent_client_protocol::schema::ProtocolVersion;

use super::super::plugin_host::PluginHost;
use super::super::types::{ChannelInboundContext, CHANNEL_CONTEXT_META_KEY};
use super::super::{ChannelEnvelope, ChannelInput, ConversationIngress};
use crate::proc_log;
use crate::process::registry::ProcessKind;
use crate::routing::RouteKey;

fn route_for_prompt(
    channel_kind: &str,
    session_chat_id: &str,
    meta: Option<&serde_json::Map<String, serde_json::Value>>,
) -> Result<RouteKey, String> {
    let Some(value) = meta.and_then(|meta| meta.get(CHANNEL_CONTEXT_META_KEY)) else {
        return Ok(RouteKey::new(channel_kind, session_chat_id));
    };

    let context: ChannelInboundContext = serde_json::from_value(value.clone())
        .map_err(|error| format!("invalid {CHANNEL_CONTEXT_META_KEY} metadata: {error}"))?;
    if context.channel_instance_id.trim().is_empty() {
        return Err("channelInstanceId must not be empty".into());
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

    Ok(RouteKey::with_actor(
        channel_kind,
        context.channel_instance_id,
        context.chat_id,
        context.actor_id,
        context.topic_id,
    ))
}

/// ACP Agent handler for a channel plugin. `prompt()` calls through the
/// shared conversation ingress — blocks until the turn completes and
/// returns the real `PromptResponse` with `StopReason`.
pub(super) struct PluginAgentHandler {
    channel_kind: String,
    config: serde_json::Value,
    /// Used for fire-and-forget callback notifications.
    input_tx: mpsc::UnboundedSender<ChannelInput>,
    ingress: Arc<ConversationIngress>,
    plugin_host: Arc<PluginHost>,
}

impl PluginAgentHandler {
    pub(super) fn new(
        channel_kind: String,
        config: serde_json::Value,
        input_tx: mpsc::UnboundedSender<ChannelInput>,
        ingress: Arc<ConversationIngress>,
        plugin_host: Arc<PluginHost>,
    ) -> Self {
        Self {
            channel_kind,
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
        meta.insert("channelInstanceId".into(), self.channel_kind.clone().into());
        meta.insert("actorId".into(), self.channel_kind.clone().into());
        meta.insert("config".into(), self.config.clone());
        meta.insert("hostVersion".into(), env!("CARGO_PKG_VERSION").into());
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
        let route = route_for_prompt(&self.channel_kind, &chat_id, args.meta.as_ref()).map_err(
            |error| {
                tracing::warn!(
                    channel_kind = %self.channel_kind,
                    chat_id = %chat_id,
                    error = %error,
                    "rejected channel prompt metadata"
                );
                acp::Error::invalid_params()
            },
        )?;

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
        self.ingress.prompt(route, content_blocks).await
    }

    pub(super) async fn cancel(&self, args: acp::CancelNotification) -> acp::Result<()> {
        let chat_id = args.session_id.to_string();
        let route = RouteKey::new(&self.channel_kind, &chat_id);

        proc_log!(
            info,
            kind = ProcessKind::ChannelPlugin,
            label = self.channel_kind,
            event = "acp_cancel",
            chat_id = %chat_id
        );

        self.ingress.dispatch(ChannelInput::Stop { route }).await;
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
                    &self.channel_kind,
                );
            }
            "va/callback" => {
                // Accept both chatId (new) and channelId (legacy, "kind:chatId") for compat.
                let chat_id = params_obj
                    .get("chatId")
                    .and_then(|v| v.as_str())
                    .or_else(|| {
                        params_obj
                            .get("channelId")
                            .and_then(|v| v.as_str())
                            .map(|cid| {
                                cid.strip_prefix(&format!("{}:", self.channel_kind))
                                    .unwrap_or(cid)
                            })
                    })
                    .unwrap_or("");
                let route = RouteKey::new(&self.channel_kind, chat_id);
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn channel_meta(value: serde_json::Value) -> serde_json::Map<String, serde_json::Value> {
        serde_json::Map::from_iter([(CHANNEL_CONTEXT_META_KEY.to_string(), value)])
    }

    #[test]
    fn legacy_prompt_route_uses_session_id_and_default_bot() {
        let route = route_for_prompt("feishu", "chat-1", None).expect("legacy route parses");

        assert_eq!(route, RouteKey::new("feishu", "chat-1"));
    }

    #[test]
    fn routed_prompt_uses_full_channel_context() {
        let meta = channel_meta(json!({
            "channelInstanceId": "feishu-primary",
            "actorId": "codex-reviewer",
            "chatId": "chat-1",
            "topicId": "topic-1",
            "scope": "group",
            "addressedBy": "mention"
        }));

        let route = route_for_prompt("feishu", "chat-1", Some(&meta))
            .expect("routed prompt metadata parses");

        assert_eq!(route.channel_instance_id(), "feishu-primary");
        assert_eq!(route.chat_id, "chat-1");
        assert_eq!(route.actor_id(), Some("codex-reviewer"));
        assert_eq!(route.topic_id(), Some("topic-1"));
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

        let error = route_for_prompt("feishu", "chat-1", Some(&meta))
            .expect_err("unaddressed group message must be rejected");

        assert_eq!(error, "group prompts must explicitly address the actor");
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

        let route = route_for_prompt("feishu", "chat-1", Some(&meta))
            .expect("direct messages do not require an explicit mention");

        assert_eq!(route.actor_id(), Some("codex-reviewer"));
    }
}
