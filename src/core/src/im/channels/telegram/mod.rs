//! Telegram IM channel: transport (send/edit/react/interactive) and receiver (long polling).

mod transport;
mod receiver;

pub use transport::{TelegramTransport, TELEGRAM_MAX_MESSAGE_LEN};
pub use receiver::{run_telegram_receiver, run_telegram_bot};
