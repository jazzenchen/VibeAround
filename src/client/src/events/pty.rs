use serde::Serialize;

use super::WebSocketSpec;
use crate::http::{path_with_query, AuthRequirement};

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum PtyClientMessage {
    Resize { cols: u16, rows: u16 },
}

pub fn pty_ws() -> WebSocketSpec {
    WebSocketSpec::new("/ws", AuthRequirement::BearerToken)
}

pub fn pty_ws_for_session(session_id: &str) -> WebSocketSpec {
    WebSocketSpec::new(
        path_with_query("/ws", &[("session_id", Some(session_id.to_string()))]),
        AuthRequirement::BearerToken,
    )
}

pub fn pty_resize(cols: u16, rows: u16) -> PtyClientMessage {
    PtyClientMessage::Resize { cols, rows }
}

pub fn encode_pty_client_message(message: &PtyClientMessage) -> crate::Result<String> {
    serde_json::to_string(message).map_err(crate::ClientError::Encode)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pty_ws_encodes_session_query() {
        let spec = pty_ws_for_session("abc/123");
        assert_eq!(spec.path, "/ws?session_id=abc%2F123");
        assert_eq!(spec.auth, AuthRequirement::BearerToken);
    }

    #[test]
    fn pty_resize_message_matches_server_shape() {
        let text = encode_pty_client_message(&pty_resize(120, 40)).expect("json");
        assert_eq!(text, r#"{"type":"resize","cols":120,"rows":40}"#);
    }
}
