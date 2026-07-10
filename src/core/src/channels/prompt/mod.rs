//! Channel-input dispatch.
//!
//! [`ConversationIngress`] is the single business-logic entry point for
//! inbound prompts. Stdio plugins use its request/response [`prompt`](ConversationIngress::prompt)
//! method, while web/TUI inputs use fire-and-forget [`dispatch`](ConversationIngress::dispatch).
//! `dispatch` routes by variant:
//!
//! - `Message` / `Callback` → [`handler::handle_prompt`] (workspace thread
//!   slash command parse → thread runtime prompt).
//! - `Stop` / `Close` / `SwitchAgent` → workspace thread control.
//! - `Log` → forward to the daemon log stream.
//!
//! Sub-modules:
//! - [`handler`] — `handle_prompt` + workspace-thread command dispatch.

mod handler;

use std::sync::Arc;

use agent_client_protocol::schema::v1 as acp;

use crate::routing::{
    is_external_attachment_uri, is_safe_attachment_file_key, Attachment, RouteKey,
};
use crate::workspace::WorkspaceThreadManager;

use super::plugin_host::PluginHost;
use super::types::{ChannelEnvelope, ChannelInput, ChannelOutput};

pub use handler::{send_runtime_multi_agent_state_and_replay, start_runtime_and_notify};

/// Concrete conversation entry point shared by every channel transport.
///
/// This type deliberately owns no queue or lifecycle state. The server keeps
/// responsibility for per-route dispatch ordering, while the stdio transport
/// can await the real ACP [`PromptResponse`](acp::PromptResponse).
pub struct ConversationIngress {
    workspace_threads: Arc<WorkspaceThreadManager>,
    plugin_host: Arc<PluginHost>,
}

impl ConversationIngress {
    pub(crate) fn new(
        workspace_threads: Arc<WorkspaceThreadManager>,
        plugin_host: Arc<PluginHost>,
    ) -> Self {
        Self {
            workspace_threads,
            plugin_host,
        }
    }

    /// Run one prompt to completion and return its actual ACP stop reason.
    pub async fn prompt(
        &self,
        route: RouteKey,
        content_blocks: Vec<acp::ContentBlock>,
    ) -> acp::Result<acp::PromptResponse> {
        handler::handle_prompt(
            &self.workspace_threads,
            &self.plugin_host,
            route,
            content_blocks,
        )
        .await
    }

    /// Dispatch a fire-and-forget channel command on the caller's route lane.
    pub async fn dispatch(&self, input: ChannelInput) {
        match input {
            ChannelInput::Message { envelope } => {
                self.handle_prompt_input(envelope, None).await;
            }
            ChannelInput::Callback {
                envelope,
                action_value,
            } => {
                self.handle_prompt_input(envelope, action_value).await;
            }
            ChannelInput::Stop { route } => {
                let _ = self.workspace_threads.cancel_route(&route).await;
            }
            ChannelInput::Close { route, reason } => {
                let _ = self.workspace_threads.close_route(&route, reason).await;
            }
            ChannelInput::SwitchAgent { route, agent_kind } => {
                send_system_text(
                    &self.plugin_host,
                    &route,
                    &format!("Use /switch host {} with workspace threads.", agent_kind),
                )
                .await;
            }
            ChannelInput::Log { level, message } => {
                tracing::info!(
                    level = %level.unwrap_or_else(|| "info".to_string()),
                    message = %message,
                    "channel log"
                );
            }
        }
    }

    async fn handle_prompt_input(&self, envelope: ChannelEnvelope, action_value: Option<String>) {
        let route = envelope.route.clone();
        let cli_kind = envelope.cli_kind.clone();
        let text = effective_input_text(&envelope, action_value);
        let message_id = if envelope.message_id.is_empty() {
            None
        } else {
            Some(envelope.message_id.clone())
        };
        tracing::debug!(
            route = %route,
            cli_kind = ?cli_kind,
            text = %text,
            "channel input"
        );

        let content_blocks = envelope_content_blocks(&text, &envelope.attachments);

        match self.prompt(route.clone(), content_blocks).await {
            Ok(_resp) => {
                tracing::debug!(route = %route, "prompt ok");
            }
            Err(e) => {
                tracing::warn!(route = %route, error = %e, "prompt failed");
                send_system_text(&self.plugin_host, &route, &format!("❌ {}", e)).await;
                if let Some(reason) = auto_close_reason_for_prompt_error(&e) {
                    if let Err(close_error) = self
                        .workspace_threads
                        .close_route(&route, Some(reason))
                        .await
                    {
                        tracing::warn!(
                            route = %route,
                            error = %close_error,
                            "failed to auto-close failed workspace thread"
                        );
                    }
                }
            }
        }
        send_prompt_done(&self.plugin_host, &route, message_id).await;
        if let Err(error) = self
            .workspace_threads
            .schedule_route_host_idle_shutdown(&route)
            .await
        {
            tracing::debug!(
                route = %route,
                error = %error,
                "failed to schedule agent host idle shutdown"
            );
        }
    }
}

fn effective_input_text(envelope: &ChannelEnvelope, action_value: Option<String>) -> String {
    if envelope.text.is_empty() {
        action_value.unwrap_or_default()
    } else {
        envelope.text.clone()
    }
}

fn envelope_content_blocks(text: &str, attachments: &[Attachment]) -> Vec<acp::ContentBlock> {
    let mut blocks = Vec::with_capacity(usize::from(!text.is_empty()) + attachments.len());
    if !text.is_empty() {
        blocks.push(acp::ContentBlock::Text(acp::TextContent::new(text)));
    }
    blocks.extend(attachments.iter().filter_map(attachment_content_block));
    blocks
}

fn attachment_content_block(attachment: &Attachment) -> Option<acp::ContentBlock> {
    let uri = match attachment_uri(&attachment.file_key) {
        Some(uri) => uri,
        None => {
            tracing::warn!(
                file_key = %attachment.file_key,
                "dropping attachment with unsafe file key"
            );
            return None;
        }
    };
    let mut link = acp::ResourceLink::new(attachment.file_name.clone(), uri);
    if !attachment.resource_type.trim().is_empty() {
        link.mime_type = Some(attachment.resource_type.clone());
    }
    link.size = attachment.size;
    Some(acp::ContentBlock::ResourceLink(link))
}

fn attachment_uri(file_key: &str) -> Option<String> {
    if is_external_attachment_uri(file_key) {
        return Some(file_key.to_string());
    }
    if !is_safe_attachment_file_key(file_key) {
        return None;
    }
    Some(format!(
        "file://{}",
        crate::config::data_dir()
            .join(".cache")
            .join(file_key)
            .to_string_lossy()
    ))
}

/// Fire-and-forget helper: emit a `SystemText` to the plugin for this route.
/// Shared by every sub-module in this folder.
pub(super) async fn send_system_text(plugin_host: &Arc<PluginHost>, route: &RouteKey, text: &str) {
    plugin_host
        .send_output(ChannelOutput::SystemText {
            route: route.clone(),
            text: text.to_string(),
            reply_to: None,
        })
        .await;
}

async fn send_prompt_done(
    plugin_host: &Arc<PluginHost>,
    route: &RouteKey,
    message_id: Option<String>,
) {
    plugin_host
        .send_output(ChannelOutput::PromptDone {
            route: route.clone(),
            message_id,
        })
        .await;
}

fn auto_close_reason_for_prompt_error(error: &acp::Error) -> Option<String> {
    if error.code == acp::ErrorCode::AuthRequired {
        return Some("agent authentication required".to_string());
    }

    let message = error.message.trim().to_ascii_lowercase();
    if message == "workspace thread is closed" {
        return Some("workspace thread already closed".to_string());
    }
    if message.contains("authentication required") {
        return Some("agent authentication required".to_string());
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn envelope_with_text(text: &str) -> ChannelEnvelope {
        ChannelEnvelope {
            route: RouteKey::new("feishu", "chat-a"),
            message_id: String::new(),
            turn_id: None,
            text: text.to_string(),
            sender_id: String::new(),
            attachments: Vec::new(),
            parent_id: None,
            cli_kind: None,
        }
    }

    #[test]
    fn callback_action_value_becomes_prompt_text() {
        let envelope = envelope_with_text("");

        assert_eq!(
            effective_input_text(&envelope, Some("approve".to_string())),
            "approve"
        );
    }

    #[test]
    fn message_text_takes_precedence_over_callback_action_value() {
        let envelope = envelope_with_text("typed text");

        assert_eq!(
            effective_input_text(&envelope, Some("button".to_string())),
            "typed text"
        );
    }

    #[test]
    fn channel_envelope_builds_shared_prompt_content() {
        let attachments = vec![Attachment {
            message_id: "message-a".to_string(),
            file_key: "https://example.com/report.md".to_string(),
            file_name: "report.md".to_string(),
            resource_type: "text/markdown".to_string(),
            size: Some(42),
        }];

        let blocks = envelope_content_blocks("review this", &attachments);

        assert_eq!(blocks.len(), 2);
        let acp::ContentBlock::Text(text) = &blocks[0] else {
            panic!("first block must preserve the message text");
        };
        assert_eq!(text.text, "review this");
        assert!(matches!(blocks[1], acp::ContentBlock::ResourceLink(_)));
    }

    #[test]
    fn unsafe_relative_attachment_key_is_rejected() {
        assert!(attachment_uri("../../secret").is_none());
        assert!(attachment_uri(r"nested\secret").is_none());
        assert!(attachment_uri("safe_upload_key").is_some());
        assert_eq!(
            attachment_uri("file:///tmp/report.md").as_deref(),
            Some("file:///tmp/report.md")
        );
    }

    #[test]
    fn auto_close_only_for_unrecoverable_prompt_errors() {
        assert_eq!(
            auto_close_reason_for_prompt_error(&acp::Error::auth_required()).as_deref(),
            Some("agent authentication required")
        );
        assert_eq!(
            auto_close_reason_for_prompt_error(&acp::Error::new(
                -32603,
                "workspace thread is closed"
            ))
            .as_deref(),
            Some("workspace thread already closed")
        );
        assert_eq!(
            auto_close_reason_for_prompt_error(&acp::Error::new(
                -32603,
                "ACP initialize failed for claude: Authentication required"
            ))
            .as_deref(),
            Some("agent authentication required")
        );
        assert!(auto_close_reason_for_prompt_error(&acp::Error::new(
            -32603,
            "upstream request failed"
        ))
        .is_none());
    }
}
