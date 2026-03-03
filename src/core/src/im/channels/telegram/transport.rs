//! Telegram transport: send, edit, reply, reactions, inline keyboard.
//! All teloxide types stay inside this module.

use crate::im::transport::{ImChannelCapabilities, ImTransport, InteractiveOption, SendError};

pub const TELEGRAM_MAX_MESSAGE_LEN: usize = 4096;

/// Telegram send implementation: parses channel_id as "telegram:CHAT_ID" and calls send_message.
pub struct TelegramTransport {
    pub(crate) bot: teloxide::Bot,
}

impl TelegramTransport {
    pub fn new(bot: teloxide::Bot) -> Self {
        Self { bot }
    }
}

#[async_trait::async_trait]
impl ImTransport for TelegramTransport {
    fn capabilities(&self) -> ImChannelCapabilities {
        ImChannelCapabilities {
            supports_stream_edit: true,
            buffer_stream: true,
            max_message_len: TELEGRAM_MAX_MESSAGE_LEN,
            channel_id_prefix: "telegram",
            // Telegram reactions must be from the allowed emoji list:
            // https://core.telegram.org/bots/api#reactiontypeemoji
            processing_reaction: "👀",
            done_reaction: "🎉",
        }
    }

    async fn send(&self, channel_id: &str, text: &str) -> Result<Option<String>, SendError> {
        use teloxide::prelude::*;
        let chat_id = parse_chat_id(channel_id)?;
        let text = truncate_to_max(text).into_owned();
        let msg = self.bot.send_message(chat_id, text.as_str()).await
            .map_err(|e| SendError::Other(e.to_string()))?;
        Ok(Some(msg.id.0.to_string()))
    }

    async fn edit_message(&self, channel_id: &str, message_id: &str, text: &str) -> Result<(), SendError> {
        use teloxide::prelude::*;
        let chat_id = parse_chat_id(channel_id)?;
        let text = truncate_to_max(text).into_owned();
        let mid: i32 = message_id.parse()
            .map_err(|_| SendError::Other(format!("invalid message_id: {}", message_id)))?;
        self.bot.edit_message_text(chat_id, teloxide::types::MessageId(mid), text.as_str()).await
            .map_err(|e| SendError::Other(e.to_string()))?;
        Ok(())
    }

    async fn reply(&self, channel_id: &str, reply_to_message_id: &str, text: &str) -> Result<Option<String>, SendError> {
        use teloxide::prelude::*;
        let chat_id = parse_chat_id(channel_id)?;
        let text = truncate_to_max(text).into_owned();
        let reply_mid: i32 = reply_to_message_id.parse()
            .map_err(|_| SendError::Other(format!("invalid reply_to message_id: {}", reply_to_message_id)))?;
        let reply_params = teloxide::types::ReplyParameters::new(teloxide::types::MessageId(reply_mid));
        let msg = self.bot.send_message(chat_id, text.as_str())
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

        // Build inline keyboard rows (2 buttons per row)
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
