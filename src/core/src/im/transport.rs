//! IM transport abstraction: "send one message" and "edit message" for the unified send daemon.
//! Each channel declares unified capabilities at IM level; daemon branches on these when processing.

use async_trait::async_trait;

/// Error from sending/editing a message. Rate-limited responses can be retried after a delay.
#[derive(Debug, Clone)]
pub enum SendError {
    /// API returned 429; retry after this many seconds.
    #[allow(dead_code)]
    RateLimited { retry_after_secs: f64 },
    /// Other error (network, auth, etc.).
    Other(String),
}

/// Result of sending a message. If the channel supports stream-edit, returns Some(message_id).
pub type SendResult = Option<i32>;

/// Unified channel capabilities defined at IM level. Each channel (Telegram, Feishu, etc.)
/// declares these so the daemon can branch: e.g. only use edit_message when supports_stream_edit.
#[derive(Clone, Debug)]
pub struct ImChannelCapabilities {
    /// Whether this channel supports updating a message in place (send then edit for stream).
    pub supports_stream_edit: bool,
    /// Max length for a single message (truncation and chunking).
    pub max_message_len: usize,
    /// Prefix for channel_id (e.g. "telegram", "feishu") for routing and logging.
    pub channel_id_prefix: &'static str,
}

/// Transport that can send and optionally edit messages. Implemented per IM channel.
/// Must return capabilities(); daemon uses them to decide send vs edit flow.
#[async_trait]
pub trait ImTransport: Send + Sync {
    /// Unified capabilities for this channel. Daemon branches on supports_stream_edit, etc.
    fn capabilities(&self) -> ImChannelCapabilities;

    /// Max length for a single message (convenience; equals capabilities().max_message_len).
    fn max_message_len(&self) -> usize {
        self.capabilities().max_message_len
    }

    /// Send `text` to the channel identified by `channel_id` (e.g. "telegram:123").
    /// Returns `Ok(Some(message_id))` only when capabilities().supports_stream_edit is true;
    /// otherwise returns `Ok(None)`. Caller must truncate to max_message_len.
    async fn send(&self, channel_id: &str, text: &str) -> Result<SendResult, SendError>;

    /// Edit an existing message. Only called when supports_stream_edit and send returned Some(message_id).
    async fn edit_message(&self, channel_id: &str, message_id: i32, text: &str) -> Result<(), SendError>;
}

/// Split text into chunks of at most `max_len` characters, trying to break at newlines.
/// Used by the worker when pushing long replies to the outbound queue.
pub fn chunk_message(text: &str, max_len: usize) -> Vec<String> {
    if text.len() <= max_len {
        return vec![text.to_string()];
    }
    let mut chunks = Vec::new();
    let mut rest = text;
    while !rest.is_empty() {
        let (chunk, next) = if rest.len() <= max_len {
            (rest, "")
        } else {
            let slice = &rest[..max_len];
            let break_at = slice.rfind('\n').map(|i| i + 1).unwrap_or(max_len);
            (&rest[..break_at], &rest[break_at..])
        };
        chunks.push(chunk.to_string());
        rest = next;
    }
    chunks
}
