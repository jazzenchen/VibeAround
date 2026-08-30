//! Restricted adapter for chat surfaces bound to one server-selected route.

use agent_client_protocol::schema::v1 as acp;
use common::channels::ChannelInput;
use common::routing::RouteKey;

use crate::web_server::AppState;

use super::input::{parse_web_chat_input, WebChatInput};

pub(in crate::web_server) enum BoundChatInput {
    Message(ChannelInput),
    Cancel(ChannelInput),
    PermissionResponse {
        request_id: String,
        response: acp::RequestPermissionResponse,
    },
}

/// Parse the existing web-chat wire format while keeping route and host
/// selection under server control. Text commands such as `/new` remain part
/// of the normal conversation pipeline.
pub(in crate::web_server) fn parse_bound_chat_input(
    route: &RouteKey,
    sender_id: &str,
    text: &str,
) -> Option<BoundChatInput> {
    match parse_web_chat_input(route, sender_id, text)? {
        WebChatInput::Message { mut input, .. } => {
            let ChannelInput::Message { envelope } = &mut input else {
                unreachable!("web message parser returned a non-message input");
            };
            envelope.cli_kind = None;
            Some(BoundChatInput::Message(input))
        }
        WebChatInput::Cancel(input) => Some(BoundChatInput::Cancel(input)),
        WebChatInput::PermissionResponse {
            request_id,
            response,
        } => Some(BoundChatInput::PermissionResponse {
            request_id,
            response,
        }),
        WebChatInput::SetMode { .. }
        | WebChatInput::SetConfigOption { .. }
        | WebChatInput::ResumeSession { .. } => None,
    }
}

pub(in crate::web_server) async fn respond_to_web_permission(
    state: &AppState,
    route: &RouteKey,
    request_id: &str,
    response: acp::RequestPermissionResponse,
) -> Result<(), String> {
    if !state
        .web_channel
        .clear_pending_permission(route, request_id)
        .await
    {
        return Err("permission request is not pending for this route".to_string());
    }
    state
        .channel_hub
        .respond_permission(&route.channel_kind, request_id, response)
        .await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn route() -> RouteKey {
        RouteKey::new("web", "ws_child")
    }

    #[test]
    fn bound_messages_keep_existing_text_commands_but_not_host_selection() {
        for command in ["/new", "/close", "/switch codex"] {
            let frame = serde_json::json!({
                "type": "message",
                "text": command,
                "agent": "claude",
                "profileId": "work",
                "sessionAction": "new",
                "permissionMode": "bypassPermissions",
            })
            .to_string();
            let input =
                parse_bound_chat_input(&route(), "preview-owner", &frame).expect("message input");

            let BoundChatInput::Message(ChannelInput::Message { envelope }) = input else {
                panic!("expected message");
            };
            assert_eq!(envelope.route, route());
            assert_eq!(envelope.sender_id, "preview-owner");
            assert_eq!(envelope.text, command);
            assert_eq!(envelope.cli_kind, None);
        }
    }

    #[test]
    fn bound_parser_rejects_route_and_session_controls() {
        for input in [
            r#"{"type":"resume_session","sessionId":"native-session"}"#,
            r#"{"type":"set_mode","modeId":"plan"}"#,
            r#"{"type":"set_config_option","configId":"model","value":"x"}"#,
        ] {
            assert!(parse_bound_chat_input(&route(), "preview-owner", input).is_none());
        }
    }

    #[test]
    fn bound_cancel_uses_the_fixed_route() {
        let input = parse_bound_chat_input(&route(), "preview-owner", r#"{"type":"cancel"}"#)
            .expect("cancel input");

        let BoundChatInput::Cancel(ChannelInput::Cancel { route: cancelled }) = input else {
            panic!("expected cancel");
        };
        assert_eq!(cancelled, route());
    }
}
