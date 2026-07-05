//! `PluginAgentHandler` — host-side behavior for ACP channel plugins.
//! Drives the prompt lifecycle and routes extension notifications back into
//! the host.

use std::sync::Arc;

use tokio::sync::mpsc;

use agent_client_protocol::schema::v1 as acp;
use agent_client_protocol::schema::ProtocolVersion;

use crate::proc_log;
use crate::process::registry::ProcessKind;
use crate::routing::RouteKey;
use crate::workspace::WorkspaceThreadManager;

use super::super::plugin_host::PluginHost;
use super::super::prompt::handle_prompt;
use super::super::{ChannelEnvelope, ChannelInput};

/// ACP Agent handler for a channel plugin. `prompt()` calls through to
/// `handle_prompt()` directly — blocks until the turn completes and
/// returns the real `PromptResponse` with `StopReason`.
pub(super) struct PluginAgentHandler {
    channel_kind: String,
    config: serde_json::Value,
    /// Still used for fire-and-forget operations: cancel, callback.
    input_tx: mpsc::UnboundedSender<ChannelInput>,
    workspace_thread_manager: Arc<WorkspaceThreadManager>,
    plugin_host: Arc<PluginHost>,
}

impl PluginAgentHandler {
    pub(super) fn new(
        channel_kind: String,
        config: serde_json::Value,
        input_tx: mpsc::UnboundedSender<ChannelInput>,
        workspace_thread_manager: Arc<WorkspaceThreadManager>,
        plugin_host: Arc<PluginHost>,
    ) -> Self {
        Self {
            channel_kind,
            config,
            input_tx,
            workspace_thread_manager,
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
        let route = RouteKey::new(&self.channel_kind, &chat_id);

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

        // Call through to handle_prompt — blocks until the turn completes.
        // Session notifications stream to the plugin via ChannelBridgeHandler
        // → PluginHost → output_tx → output forwarder → conn.session_notification().
        let result = handle_prompt(
            &self.workspace_thread_manager,
            &self.plugin_host,
            route.clone(),
            content_blocks,
        )
        .await;

        result
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

        let _ = self.workspace_thread_manager.cancel_route(&route).await;
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
