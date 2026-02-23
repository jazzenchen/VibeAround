//! IM channel spec: unified kind enum and build-from-config so all channels are managed the same way.
//! Each concrete channel (Telegram, Feishu) implements ImTransport; this module defines how to
//! construct them from global config for unified dispatch and future plugins.

use std::sync::Arc;

use crate::config;
use crate::im::channels::{feishu, telegram};
use crate::im::transport::ImTransport;

/// Channel kind identifier. Used to list available channels and build transport from config.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImChannelKind {
    Telegram,
    Feishu,
}

impl ImChannelKind {
    /// Unique string id for config and logging (e.g. "telegram", "feishu").
    pub fn kind_id(&self) -> &'static str {
        match self {
            ImChannelKind::Telegram => "telegram",
            ImChannelKind::Feishu => "feishu",
        }
    }

    /// Build transport from global config. Returns None if config is missing or invalid for this channel.
    pub fn build_from_config(&self, config: &config::Config) -> Option<Arc<dyn ImTransport>> {
        match self {
            ImChannelKind::Telegram => {
                let token = config.telegram_bot_token.as_deref()?;
                if token.is_empty() {
                    return None;
                }
                let bot = teloxide::Bot::new(token);
                Some(Arc::new(telegram::TelegramTransport::new(bot)))
            }
            ImChannelKind::Feishu => {
                let app_id = config.feishu_app_id.clone()?;
                let app_secret = config.feishu_app_secret.clone()?;
                if app_id.is_empty() || app_secret.is_empty() {
                    return None;
                }
                Some(Arc::new(feishu::FeishuTransport::new(app_id, app_secret)))
            }
        }
    }

    /// All known channel kinds (for iteration / discovery).
    pub fn all() -> &'static [ImChannelKind] {
        &[ImChannelKind::Telegram, ImChannelKind::Feishu]
    }
}
