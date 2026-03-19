//! Unified IM log format: [VibeAround][im][channel] key=value ...
//! Channel name is derived from channel_id (e.g. "telegram:123" -> "telegram").

const CONTENT_LOG_MAX_LEN: usize = 120;

/// Log prefix for IM: [VibeAround][im][{channel}]. Channel is parsed from channel_id (prefix before ':').
#[inline]
pub fn prefix(channel_id: &str) -> String {
    let channel = channel_id.split(':').next().unwrap_or("?");
    format!("[VibeAround][im][{}]", channel)
}

/// Same as prefix but with explicit channel name (e.g. for webhook where we know "feishu" before we have channel_id).
#[inline]
pub fn prefix_channel(channel: &str) -> String {
    format!("[VibeAround][im][{}]", channel)
}

/// Truncate message content for logging (avoid huge dumps).
#[inline]
pub fn truncate_content(content: &str, _max_len: usize) -> std::borrow::Cow<'_, str> {
    std::borrow::Cow::Borrowed(content)
}

#[inline]
pub fn truncate_content_default(content: &str) -> std::borrow::Cow<'_, str> {
    truncate_content(content, CONTENT_LOG_MAX_LEN)
}
