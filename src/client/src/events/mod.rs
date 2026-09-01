mod bridge_recording;
mod chat;
mod snapshots;

use crate::http::AuthRequirement;

pub use bridge_recording::*;
pub use chat::*;
pub use snapshots::*;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WebSocketSpec {
    pub path: String,
    pub auth: AuthRequirement,
}

impl WebSocketSpec {
    pub fn new(path: impl Into<String>, auth: AuthRequirement) -> Self {
        Self {
            path: path.into(),
            auth,
        }
    }
}
