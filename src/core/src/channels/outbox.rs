//! Process-local channel output outbox.
//!
//! This is a transport buffer for the current daemon process only. It lets
//! outputs wait while a channel plugin runtime is briefly unavailable and
//! flushes them when that runtime comes back. It intentionally does not
//! survive daemon restarts: pending permission responders and plugin
//! runtimes are also in-memory, so replaying old outputs would create
//! stale messages and unanswerable permission prompts.

use parking_lot::Mutex;

use crate::storage::jsonl;

use super::ChannelOutput;

const OUTBOX_FILE: &str = "channel-outbox.jsonl";

#[derive(Debug, Clone)]
pub struct PendingOutput {
    pub output_id: String,
    pub output: ChannelOutput,
}

pub struct ChannelOutbox {
    pending: Mutex<Vec<PendingOutput>>,
}

impl ChannelOutbox {
    pub fn new_default() -> Self {
        discard_persisted_outbox_files();
        Self::new()
    }

    pub fn new() -> Self {
        Self {
            pending: Mutex::new(Vec::new()),
        }
    }

    pub async fn enqueue(&self, output: ChannelOutput) -> jsonl::Result<String> {
        self.enqueue_now(output)
    }

    pub fn enqueue_now(&self, output: ChannelOutput) -> jsonl::Result<String> {
        let output_id = format!("out_{}", uuid::Uuid::new_v4().simple());
        self.pending.lock().push(PendingOutput {
            output_id: output_id.clone(),
            output,
        });
        Ok(output_id)
    }

    pub async fn mark_sent(&self, output_id: &str) -> jsonl::Result<()> {
        self.mark_sent_now(output_id)
    }

    pub async fn mark_nacked(&self, output_id: &str, _reason: Option<String>) -> jsonl::Result<()> {
        self.mark_nacked_now(output_id)
    }

    pub fn mark_sent_now(&self, output_id: &str) -> jsonl::Result<()> {
        self.pending
            .lock()
            .retain(|pending| pending.output_id != output_id);
        Ok(())
    }

    pub fn mark_nacked_now(&self, _output_id: &str) -> jsonl::Result<()> {
        Ok(())
    }

    pub fn pending_for_channel(&self, channel_kind: &str) -> Vec<PendingOutput> {
        self.pending
            .lock()
            .iter()
            .filter(|pending| pending.output.route_key().channel_kind == channel_kind)
            .cloned()
            .collect()
    }
}

fn discard_persisted_outbox_files() {
    for path in [
        crate::config::legacy_state_file(OUTBOX_FILE),
        crate::config::state_file(OUTBOX_FILE),
    ] {
        match std::fs::remove_file(&path) {
            Ok(()) => tracing::info!(path = ?path, "discarded persisted channel outbox"),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                tracing::warn!(path = ?path, error = %error, "failed to discard persisted channel outbox")
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::routing::RouteKey;

    use super::*;

    #[tokio::test]
    async fn pending_outputs_filter_by_channel() {
        let outbox = ChannelOutbox::new();
        let feishu = ChannelOutput::SystemText {
            route: RouteKey::new("feishu", "chat-a"),
            text: "hello".to_string(),
            reply_to: None,
        };
        let web = ChannelOutput::SystemText {
            route: RouteKey::new("web", "chat-a"),
            text: "hello".to_string(),
            reply_to: None,
        };

        let feishu_id = outbox.enqueue(feishu).await.unwrap();
        let _ = outbox.enqueue(web).await.unwrap();

        let pending = outbox.pending_for_channel("feishu");

        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].output_id, feishu_id);
    }

    #[tokio::test]
    async fn pending_outputs_keep_enqueue_order() {
        let outbox = ChannelOutbox::new();
        let first = ChannelOutput::SystemText {
            route: RouteKey::new("feishu", "chat-a"),
            text: "first".to_string(),
            reply_to: None,
        };
        let second = ChannelOutput::SystemText {
            route: RouteKey::new("feishu", "chat-a"),
            text: "second".to_string(),
            reply_to: None,
        };
        let first_id = outbox.enqueue(first).await.unwrap();
        let second_id = outbox.enqueue(second).await.unwrap();

        let pending = outbox.pending_for_channel("feishu");

        assert_eq!(
            pending
                .iter()
                .map(|item| item.output_id.as_str())
                .collect::<Vec<_>>(),
            vec![first_id.as_str(), second_id.as_str()]
        );
    }

    #[tokio::test]
    async fn new_does_not_share_pending_outputs() {
        let outbox = ChannelOutbox::new();
        let output = ChannelOutput::SystemText {
            route: RouteKey::new("feishu", "chat-a"),
            text: "hello".to_string(),
            reply_to: None,
        };
        let _ = outbox.enqueue(output).await.unwrap();

        let fresh = ChannelOutbox::new();
        assert!(fresh.pending_for_channel("feishu").is_empty());
    }

    #[tokio::test]
    async fn mark_sent_removes_pending_output() {
        let outbox = ChannelOutbox::new();
        let output = ChannelOutput::SystemText {
            route: RouteKey::new("feishu", "chat-a"),
            text: "hello".to_string(),
            reply_to: None,
        };
        let output_id = outbox.enqueue(output).await.unwrap();
        outbox.mark_sent(&output_id).await.unwrap();

        assert!(outbox.pending_for_channel("feishu").is_empty());
    }

    #[tokio::test]
    async fn mark_nacked_keeps_pending_output() {
        let outbox = ChannelOutbox::new();
        let output = ChannelOutput::SystemText {
            route: RouteKey::new("feishu", "chat-a"),
            text: "hello".to_string(),
            reply_to: None,
        };
        let output_id = outbox.enqueue(output).await.unwrap();
        outbox
            .mark_nacked(&output_id, Some("failed".to_string()))
            .await
            .unwrap();

        assert_eq!(outbox.pending_for_channel("feishu").len(), 1);
    }
}
