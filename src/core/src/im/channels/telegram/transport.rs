//! Telegram transport: send, edit (via sendMessageDraft), reply, reactions, inline keyboard.
//! Uses sendMessageDraft for streaming (typewriter effect), sendMessage for final commit.
//! teloxide is used for non-streaming operations; raw HTTP for sendMessageDraft (not in teloxide).

use std::sync::atomic::{AtomicI64, Ordering};

use crate::im::transport::{ImChannelCapabilities, ImTransport, InteractiveOption, SendError};

pub const TELEGRAM_MAX_MESSAGE_LEN: usize = 4096;

/// Telegram send implementation: parses channel_id as "telegram:CHAT_ID" and calls send_message.
/// Streaming uses sendMessageDraft API (raw HTTP) for typewriter effect.
pub struct TelegramTransport {
    pub(crate) bot: teloxide::Bot,
    client: reqwest::Client,
    token: String,
    /// Monotonically increasing draft_id counter for sendMessageDraft.
    draft_counter: AtomicI64,
}

impl TelegramTransport {
    pub fn new(bot: teloxide::Bot) -> Self {
        let token = bot.token().to_string();
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(15))
            .build()
            .expect("reqwest client");
        Self { bot, client, token, draft_counter: AtomicI64::new(1) }
    }

    fn next_draft_id(&self) -> i64 {
        self.draft_counter.fetch_add(1, Ordering::Relaxed)
    }

    /// Call sendMessageDraft via raw HTTP (not in teloxide).
    async fn send_message_draft(&self, chat_id: i64, draft_id: i64, text: &str) -> Result<(), SendError> {
        let url = format!("https://api.telegram.org/bot{}/sendMessageDraft", self.token);
        let body = serde_json::json!({
            "chat_id": chat_id,
            "draft_id": draft_id,
            "text": text,
        });
        let res = self.client.post(&url).json(&body).send().await
            .map_err(|e| SendError::Other(format!("sendMessageDraft: {}", e)))?;
        let status = res.status();
        if status.as_u16() == 429 {
            let text = res.text().await.unwrap_or_default();
            let retry: f64 = serde_json::from_str::<serde_json::Value>(&text).ok()
                .and_then(|v| v.pointer("/parameters/retry_after").and_then(|r| r.as_f64()))
                .unwrap_or(1.0);
            return Err(SendError::RateLimited { retry_after_secs: retry });
        }
        if !status.is_success() {
            let body = res.text().await.unwrap_or_default();
            return Err(SendError::Other(format!("sendMessageDraft status={} body={}", status, body)));
        }
        Ok(())
    }
}

#[async_trait::async_trait]
impl ImTransport for TelegramTransport {
    fn capabilities(&self) -> ImChannelCapabilities {
        ImChannelCapabilities {
            supports_stream_edit: true,
            max_message_len: TELEGRAM_MAX_MESSAGE_LEN,
            channel_id_prefix: "telegram",
            processing_reaction: "👀",
            min_edit_interval: std::time::Duration::ZERO,
        }
    }

    /// Start a new streaming draft. Returns "draft:{chat_id}:{draft_id}" as the message_id
    /// so edit_message can continue pushing updates via sendMessageDraft.
    async fn send(&self, channel_id: &str, text: &str) -> Result<Option<String>, SendError> {
        let chat_id = parse_chat_id(channel_id)?;
        let draft_id = self.next_draft_id();
        let text = truncate_to_max(text);
        self.send_message_draft(chat_id.0, draft_id, &text).await?;
        // Encode draft context so edit_message can reuse it
        Ok(Some(format!("draft:{}:{}", chat_id.0, draft_id)))
    }

    /// Update the streaming draft with new text.
    async fn edit_message(&self, _channel_id: &str, message_id: &str, text: &str) -> Result<(), SendError> {
        let text = truncate_to_max(text);
        // Parse "draft:{chat_id}:{draft_id}"
        let parts: Vec<&str> = message_id.splitn(3, ':').collect();
        if parts.len() != 3 || parts[0] != "draft" {
            return Err(SendError::Other(format!("invalid draft message_id: {}", message_id)));
        }
        let chat_id: i64 = parts[1].parse()
            .map_err(|_| SendError::Other(format!("invalid chat_id in draft: {}", message_id)))?;
        let draft_id: i64 = parts[2].parse()
            .map_err(|_| SendError::Other(format!("invalid draft_id: {}", message_id)))?;

        self.send_message_draft(chat_id, draft_id, &text).await
    }

    /// Materialize the draft into a permanent message via sendMessage.
    async fn finalize_stream(&self, _channel_id: &str, message_id: &str, final_text: &str) -> Result<(), SendError> {
        use teloxide::prelude::*;
        let text = truncate_to_max(final_text);
        let parts: Vec<&str> = message_id.splitn(3, ':').collect();
        if parts.len() != 3 || parts[0] != "draft" {
            return Ok(()); // not a draft, nothing to materialize
        }
        let chat_id: i64 = parts[1].parse()
            .map_err(|_| SendError::Other(format!("invalid chat_id in draft: {}", message_id)))?;
        let tg_chat_id = teloxide::types::ChatId(chat_id);
        self.bot.send_message(tg_chat_id, text.as_ref()).await
            .map_err(|e| SendError::Other(e.to_string()))?;
        Ok(())
    }

    async fn reply(&self, channel_id: &str, reply_to_message_id: &str, text: &str) -> Result<Option<String>, SendError> {
        use teloxide::prelude::*;
        let chat_id = parse_chat_id(channel_id)?;
        let text = truncate_to_max(text);
        let reply_mid: i32 = reply_to_message_id.parse()
            .map_err(|_| SendError::Other(format!("invalid reply_to message_id: {}", reply_to_message_id)))?;
        let reply_params = teloxide::types::ReplyParameters::new(teloxide::types::MessageId(reply_mid));
        let msg = self.bot.send_message(chat_id, text.as_ref())
            .reply_parameters(reply_params)
            .await
            .map_err(|e| SendError::Other(e.to_string()))?;
        Ok(Some(msg.id.0.to_string()))
    }

    async fn add_reaction(&self, channel_id: &str, message_id: &str, emoji: &str) -> Result<Option<String>, SendError> {
        use teloxide::prelude::*;
        let chat_id = parse_chat_id(channel_id)?;
        let mid: i32 = message_id.parse()
            .map_err(|_| SendError::Other(format!("invalid message_id: {}", message_id)))?;
        let reaction = teloxide::types::ReactionType::Emoji { emoji: emoji.to_string() };
        self.bot.set_message_reaction(chat_id, teloxide::types::MessageId(mid))
            .reaction(vec![reaction])
            .await
            .map_err(|e| SendError::Other(e.to_string()))?;
        Ok(Some(emoji.to_string()))
    }

    async fn remove_reaction(&self, channel_id: &str, message_id: &str, _reaction_id: &str) -> Result<(), SendError> {
        use teloxide::prelude::*;
        let chat_id = parse_chat_id(channel_id)?;
        let mid: i32 = message_id.parse()
            .map_err(|_| SendError::Other(format!("invalid message_id: {}", message_id)))?;
        self.bot.set_message_reaction(chat_id, teloxide::types::MessageId(mid))
            .reaction(Vec::<teloxide::types::ReactionType>::new())
            .await
            .map_err(|e| SendError::Other(e.to_string()))?;
        Ok(())
    }

    async fn send_interactive(
        &self,
        channel_id: &str,
        prompt: &str,
        options: &[InteractiveOption],
        reply_to: Option<&str>,
    ) -> Result<Option<String>, SendError> {
        use teloxide::prelude::*;
        let chat_id = parse_chat_id(channel_id)?;

        let mut rows: Vec<Vec<teloxide::types::InlineKeyboardButton>> = Vec::new();
        let mut current_row = Vec::new();
        for opt in options {
            current_row.push(teloxide::types::InlineKeyboardButton::callback(&opt.label, &opt.value));
            if current_row.len() >= 2 {
                rows.push(std::mem::take(&mut current_row));
            }
        }
        if !current_row.is_empty() {
            rows.push(current_row);
        }
        let keyboard = teloxide::types::InlineKeyboardMarkup::new(rows);

        let mut req = self.bot.send_message(chat_id, prompt);
        req = req.reply_markup(keyboard);
        if let Some(reply_mid_str) = reply_to {
            if let Ok(mid) = reply_mid_str.parse::<i32>() {
                let reply_params = teloxide::types::ReplyParameters::new(teloxide::types::MessageId(mid));
                req = req.reply_parameters(reply_params);
            }
        }
        let msg = req.await.map_err(|e| SendError::Other(e.to_string()))?;
        Ok(Some(msg.id.0.to_string()))
    }
}

pub(crate) fn parse_chat_id(channel_id: &str) -> Result<teloxide::types::ChatId, SendError> {
    let s = channel_id
        .strip_prefix("telegram:")
        .ok_or_else(|| SendError::Other("invalid channel_id (expected telegram:CHAT_ID)".into()))?;
    let id: i64 = s.parse()
        .map_err(|_| SendError::Other(format!("invalid telegram chat_id: {}", channel_id)))?;
    Ok(teloxide::types::ChatId(id))
}

fn truncate_to_max(text: &str) -> std::borrow::Cow<'_, str> {
    if text.len() <= TELEGRAM_MAX_MESSAGE_LEN {
        std::borrow::Cow::Borrowed(text)
    } else {
        std::borrow::Cow::Owned(text[..TELEGRAM_MAX_MESSAGE_LEN].to_string())
    }
}
