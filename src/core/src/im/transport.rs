//! IM transport abstraction: "send one message" and "edit message" for the unified send daemon.
//! Each channel declares unified capabilities at IM level; daemon branches on these when processing.

use std::time::Duration;

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

/// Result of sending a message. Returns Some(message_id) if the platform provides one.
/// message_id is a String to accommodate different platforms (Feishu: "om_xxx", Telegram: "123").
pub type SendResult = Option<String>;

/// Style for interactive buttons.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ButtonStyle {
    Primary,
    Danger,
    Default,
}

/// An interactive option (button) for user input.
#[derive(Debug, Clone)]
pub struct InteractiveOption {
    pub label: String,
    pub value: String,
    pub style: ButtonStyle,
    /// Row group index. Buttons with the same group are placed on the same row.
    /// Different groups are separated visually (e.g. hr in Feishu cards).
    pub group: u8,
}

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
    /// Reaction to add when processing starts (platform-specific identifier).
    pub processing_reaction: &'static str,
    /// Minimum interval between edit_message calls (throttle). Platforms have different rate limits.
    pub min_edit_interval: Duration,
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
    async fn edit_message(&self, channel_id: &str, message_id: &str, text: &str) -> Result<(), SendError>;

    /// Send a message as a reply to another message. Default: just send without quoting.
    async fn reply(&self, channel_id: &str, reply_to_message_id: &str, text: &str) -> Result<SendResult, SendError> {
        let _ = reply_to_message_id;
        self.send(channel_id, text).await
    }

    /// Add a reaction emoji to a message (e.g. 👀 when processing starts).
    /// Returns a reaction_id that can be used to remove it later.
    async fn add_reaction(&self, _channel_id: &str, _message_id: &str, _emoji: &str) -> Result<Option<String>, SendError> {
        Ok(None) // default no-op
    }

    /// Remove a reaction from a message (e.g. when processing completes).
    async fn remove_reaction(&self, _channel_id: &str, _message_id: &str, _reaction_id: &str) -> Result<(), SendError> {
        Ok(()) // default no-op
    }

    /// Finalize a streaming session: convert draft/streaming preview into a permanent message.
    /// Called by daemon at StreamEnd/StreamDone after the last edit_message.
    /// - Telegram: materialize draft via sendMessage
    /// - Feishu: disable streaming_mode on the card
    /// Default: no-op (for channels that don't need finalization).
    async fn finalize_stream(&self, _channel_id: &str, _message_id: &str, _final_text: &str) -> Result<(), SendError> {
        Ok(())
    }

    /// Send an interactive card/inline keyboard for user input (e.g. tool approval).
    /// Returns the message_id of the interactive message.
    async fn send_interactive(
        &self,
        _channel_id: &str,
        _prompt: &str,
        _options: &[InteractiveOption],
        _reply_to: Option<&str>,
    ) -> Result<SendResult, SendError> {
        // Default: fall back to plain text with numbered options
        Ok(None)
    }

    /// Update an existing interactive card in place (e.g. highlight the selected agent).
    /// Only works if the platform supports editing interactive messages.
    async fn update_interactive(
        &self,
        _channel_id: &str,
        _message_id: &str,
        _prompt: &str,
        _options: &[InteractiveOption],
    ) -> Result<(), SendError> {
        Ok(()) // default no-op
    }
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
        if rest.len() <= max_len {
            chunks.push(rest.to_string());
            break;
        }
        // Try to break at a newline within the last 20% of the chunk
        let search_start = max_len * 4 / 5;
        let break_at = rest[search_start..max_len]
            .rfind('\n')
            .map(|i| search_start + i + 1)
            .unwrap_or(max_len);
        chunks.push(rest[..break_at].to_string());
        rest = &rest[break_at..];
    }
    chunks
}
