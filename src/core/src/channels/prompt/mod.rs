//! Channel-input dispatch.
//!
//! [`ConversationIngress`] is the single business-logic and ordering entry
//! point for inbound prompts. Stdio plugins use its request/response
//! [`prompt`](ConversationIngress::prompt) method, while web/TUI inputs use
//! fire-and-forget [`dispatch`](ConversationIngress::dispatch). Both enter a
//! bounded lane keyed by the full [`RouteKey`]; unrelated routes run in
//! parallel, while one route remains FIFO. Stop bypasses the lane so it can
//! cancel a long-running turn immediately.
//!
//! - `Message` / `Callback` → [`handler::handle_prompt`] (workspace thread
//!   slash command parse → thread runtime prompt).
//! - `Stop` / `Close` / `SwitchAgent` → workspace thread control.
//! - `Log` → forward to the daemon log stream.
//!
//! Sub-modules:
//! - [`handler`] — `handle_prompt` + workspace-thread command dispatch.

mod handler;
mod ingress;

use std::sync::Arc;

use agent_client_protocol::schema::v1 as acp;
#[cfg(test)]
use tokio::sync::{mpsc, oneshot};

use crate::routing::{
    is_external_attachment_uri, is_safe_attachment_file_key, Attachment, RouteKey,
};
#[cfg(test)]
use crate::workspace::WorkspaceThreadManager;

use super::plugin_host::PluginHost;
use super::types::{ChannelEnvelope, ChannelInput, ChannelOutput};

pub use handler::{send_runtime_multi_agent_state_and_replay, start_runtime_and_notify};
pub use ingress::ConversationIngress;
#[cfg(test)]
use ingress::ROUTE_LANE_CAPACITY;

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
    use std::sync::atomic::{AtomicU64, Ordering};

    fn test_ingress() -> Arc<ConversationIngress> {
        static NEXT_ID: AtomicU64 = AtomicU64::new(1);
        let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        let base =
            std::env::temp_dir().join(format!("vibearound-route-lane-{}-{id}", std::process::id()));
        let workspace_threads = WorkspaceThreadManager::with_paths(
            base.join("workspaces.jsonl"),
            base.join("threads.jsonl"),
            base.join("attachments.jsonl"),
        );
        let (input_tx, _input_rx) = mpsc::unbounded_channel();
        let plugin_host = Arc::new(PluginHost::new(input_tx));
        Arc::new(ConversationIngress::new(workspace_threads, plugin_host))
    }

    async fn wait_for_lanes_to_drain(ingress: &ConversationIngress) {
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(1);
        while ingress.active_lane_count() != 0 && tokio::time::Instant::now() < deadline {
            tokio::task::yield_now().await;
        }
        assert_eq!(ingress.active_lane_count(), 0);
    }

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

    #[tokio::test]
    async fn same_route_lane_is_fifo_and_removed_when_drained() {
        let ingress = test_ingress();
        let route = RouteKey::new("web", "chat-a");
        let (first_started, first_started_rx) = oneshot::channel();
        let (release_first, release_first_rx) = oneshot::channel();
        let (second_started, mut second_started_rx) = oneshot::channel();

        let first_done = ingress
            .enqueue_probe(route.clone(), async move {
                let _ = first_started.send(());
                let _ = release_first_rx.await;
            })
            .unwrap();
        let second_done = ingress
            .enqueue_probe(route, async move {
                let _ = second_started.send(());
            })
            .unwrap();

        first_started_rx.await.unwrap();
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(50), &mut second_started_rx)
                .await
                .is_err()
        );
        release_first.send(()).unwrap();
        second_started_rx.await.unwrap();
        first_done.await.unwrap();
        second_done.await.unwrap();
        wait_for_lanes_to_drain(&ingress).await;
    }

    #[tokio::test]
    async fn different_routes_do_not_block_each_other() {
        let ingress = test_ingress();
        let (first_started, first_started_rx) = oneshot::channel();
        let (release_first, release_first_rx) = oneshot::channel();
        let (second_started, second_started_rx) = oneshot::channel();

        let first_done = ingress
            .enqueue_probe(RouteKey::new("web", "chat-a"), async move {
                let _ = first_started.send(());
                let _ = release_first_rx.await;
            })
            .unwrap();
        first_started_rx.await.unwrap();
        let second_done = ingress
            .enqueue_probe(RouteKey::new("web", "chat-b"), async move {
                let _ = second_started.send(());
            })
            .unwrap();

        tokio::time::timeout(std::time::Duration::from_millis(100), second_started_rx)
            .await
            .expect("unrelated route was blocked")
            .unwrap();
        release_first.send(()).unwrap();
        first_done.await.unwrap();
        second_done.await.unwrap();
        wait_for_lanes_to_drain(&ingress).await;
    }

    #[tokio::test]
    async fn stop_cancels_active_and_queued_route_work() {
        let ingress = test_ingress();
        let route = RouteKey::new("web", "chat-a");
        let (started, started_rx) = oneshot::channel();
        let (release, release_rx) = oneshot::channel();
        let active_done = ingress
            .enqueue_probe(route.clone(), async move {
                let _ = started.send(());
                let _ = release_rx.await;
            })
            .unwrap();
        started_rx.await.unwrap();
        let queued_done = ingress.enqueue_probe(route.clone(), async {}).unwrap();

        tokio::time::timeout(
            std::time::Duration::from_millis(100),
            ingress.dispatch(ChannelInput::Stop { route }),
        )
        .await
        .expect("stop waited behind the active turn");

        assert!(active_done.await.is_err(), "active work survived stop");
        assert!(queued_done.await.is_err(), "queued work survived stop");
        assert!(release.send(()).is_err(), "active work was not dropped");
        wait_for_lanes_to_drain(&ingress).await;
    }

    #[tokio::test]
    async fn shutdown_cancels_lanes_and_rejects_new_work() {
        let ingress = test_ingress();
        let route = RouteKey::new("web", "chat-a");
        let (started, started_rx) = oneshot::channel();
        let (_release, release_rx) = oneshot::channel::<()>();
        let active_done = ingress
            .enqueue_probe(route.clone(), async move {
                let _ = started.send(());
                let _ = release_rx.await;
            })
            .unwrap();
        started_rx.await.unwrap();

        tokio::time::timeout(std::time::Duration::from_millis(100), ingress.shutdown())
            .await
            .expect("ingress shutdown did not drain active lanes");

        assert!(active_done.await.is_err(), "active work survived shutdown");
        assert_eq!(ingress.active_lane_count(), 0);
        assert!(ingress.enqueue_probe(route, async {}).is_err());
    }

    #[tokio::test]
    async fn route_lane_rejects_work_at_capacity() {
        let ingress = test_ingress();
        let route = RouteKey::new("web", "chat-a");
        let (started, started_rx) = oneshot::channel();
        let (release, release_rx) = oneshot::channel();
        let first_done = ingress
            .enqueue_probe(route.clone(), async move {
                let _ = started.send(());
                let _ = release_rx.await;
            })
            .unwrap();
        started_rx.await.unwrap();

        let mut queued = Vec::new();
        for _ in 0..ROUTE_LANE_CAPACITY {
            queued.push(ingress.enqueue_probe(route.clone(), async {}).unwrap());
        }
        assert!(ingress.enqueue_probe(route, async {}).is_err());

        release.send(()).unwrap();
        first_done.await.unwrap();
        for done in queued {
            done.await.unwrap();
        }
        wait_for_lanes_to_drain(&ingress).await;
    }
}
