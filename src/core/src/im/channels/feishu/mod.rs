//! Feishu (Lark) IM channel: transport (send/reply/react/interactive card) and webhook receiver.

mod interaction;
mod transport;
mod webhook;

pub use interaction::build_card;
pub use transport::{FeishuTransport, FEISHU_MAX_MESSAGE_LEN};
pub use webhook::{FeishuWebhookState, handle_card_callback, handle_webhook_body, run_feishu_bot};
