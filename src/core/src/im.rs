//! IM (instant messaging) adapters: Telegram, Feishu, etc.
//! Unified send daemon + per-IM transport (teloxide only in channels::telegram).
//! Slash commands (e.g. /list-project) are handled in commands and do not call AI.
//! Log format: [VibeAround][im][channel] chat_id=... message_id=... content=...

pub mod channels;
pub mod commands;
pub mod daemon;
pub mod log;
pub mod spec;
pub mod transport;
pub mod worker;

/// Re-export channels so `common::im::telegram::run_telegram_bot` and `common::im::channels::feishu::run_feishu_bot` work.
pub use channels::{feishu, telegram};
/// Channel kind and unified build-from-config for management and dispatch.
pub use spec::ImChannelKind;
