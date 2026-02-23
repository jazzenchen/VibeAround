//! VibeAround server: Axum HTTP + WebSocket. IM (Telegram) lives in common (core).

mod web_server;

pub use web_server::run_web_server;

/// Re-export: Telegram bot runs from core (common::im::telegram). No-op if TELEGRAM_BOT_TOKEN not set.
pub use common::im::telegram::run_telegram_bot;
