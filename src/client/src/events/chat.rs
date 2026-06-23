use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::WebSocketSpec;
use crate::http::path_with_query;
use crate::http::AuthRequirement;
use crate::runtime::AgentInfo;

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ChatEvent {
    Config {
        channel_id: String,
        agents: Vec<AgentInfo>,
        default_agent: String,
    },
    AgentReady {
        agent: String,
        version: String,
    },
    SessionReady {
        session_id: String,
    },
    SessionMode {
        session_mode: Value,
    },
    CommandMenu {
        system_commands: Value,
        agent_commands: Value,
    },
    PermissionRequest {
        request_id: String,
        request: Value,
    },
    MultiAgentTurn {
        turn: Value,
        agents: Vec<Value>,
    },
    SubagentStatus {
        agent: Value,
    },
    SubagentAcpNotification {
        agent: Value,
        payload: Value,
    },
    PromptDone {
        message_id: Option<String>,
    },
    TurnStatus {
        active: bool,
    },
    SystemText {
        text: String,
    },
    AcpNotification {
        payload: Value,
    },
    Error {
        error: String,
    },
}

/// One client-to-server `/ws/chat` frame.
///
/// Hosts own the WebSocket transport; this enum only owns the JSON wire shape
/// accepted by the server's web chat adapter.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ChatClientMessage {
    Message {
        #[serde(skip_serializing_if = "String::is_empty")]
        text: String,
        #[serde(rename = "messageId", skip_serializing_if = "Option::is_none")]
        message_id: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        agent: Option<String>,
        #[serde(rename = "profileId", skip_serializing_if = "Option::is_none")]
        profile_id: Option<String>,
        #[serde(rename = "sessionAction", skip_serializing_if = "Option::is_none")]
        session_action: Option<ChatSessionAction>,
        #[serde(rename = "sessionId", skip_serializing_if = "Option::is_none")]
        session_id: Option<String>,
        #[serde(rename = "sessionWorkspace", skip_serializing_if = "Option::is_none")]
        session_workspace: Option<String>,
        #[serde(rename = "permissionMode", skip_serializing_if = "Option::is_none")]
        permission_mode: Option<String>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        attachments: Vec<ChatAttachment>,
    },
    SetMode {
        #[serde(rename = "modeId")]
        mode_id: String,
    },
    SetConfigOption {
        #[serde(rename = "configId")]
        config_id: String,
        value: String,
    },
    ResumeSession {
        #[serde(skip_serializing_if = "Option::is_none")]
        agent: Option<String>,
        #[serde(rename = "profileId", skip_serializing_if = "Option::is_none")]
        profile_id: Option<String>,
        #[serde(rename = "sessionId")]
        session_id: String,
        #[serde(rename = "sessionWorkspace", skip_serializing_if = "Option::is_none")]
        session_workspace: Option<String>,
    },
    Stop,
    PermissionResponse {
        #[serde(rename = "requestId")]
        request_id: String,
        #[serde(rename = "optionId", skip_serializing_if = "Option::is_none")]
        option_id: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        outcome: Option<ChatPermissionOutcome>,
    },
}

impl ChatClientMessage {
    pub fn message(text: impl Into<String>) -> Self {
        Self::Message {
            text: text.into(),
            message_id: None,
            agent: None,
            profile_id: None,
            session_action: None,
            session_id: None,
            session_workspace: None,
            permission_mode: None,
            attachments: Vec::new(),
        }
    }

    pub fn resume_message(text: impl Into<String>, session_id: impl Into<String>) -> Self {
        let mut message = Self::message(text);
        if let Self::Message {
            session_action,
            session_id: message_session_id,
            ..
        } = &mut message
        {
            *session_action = Some(ChatSessionAction::Resume);
            *message_session_id = Some(session_id.into());
        }
        message
    }

    pub fn new_session_message(text: impl Into<String>) -> Self {
        let mut message = Self::message(text);
        if let Self::Message { session_action, .. } = &mut message {
            *session_action = Some(ChatSessionAction::New);
        }
        message
    }

    pub fn set_mode(mode_id: impl Into<String>) -> Self {
        Self::SetMode {
            mode_id: mode_id.into(),
        }
    }

    pub fn set_config_option(config_id: impl Into<String>, value: impl Into<String>) -> Self {
        Self::SetConfigOption {
            config_id: config_id.into(),
            value: value.into(),
        }
    }

    pub fn resume_session(session_id: impl Into<String>) -> Self {
        Self::resume_session_with_options(session_id, None, None, None)
    }

    pub fn resume_session_with_options(
        session_id: impl Into<String>,
        agent: Option<String>,
        profile_id: Option<String>,
        session_workspace: Option<String>,
    ) -> Self {
        Self::ResumeSession {
            agent,
            profile_id,
            session_id: session_id.into(),
            session_workspace,
        }
    }

    pub fn stop() -> Self {
        Self::Stop
    }

    pub fn permission_selected(
        request_id: impl Into<String>,
        option_id: impl Into<String>,
    ) -> Self {
        Self::PermissionResponse {
            request_id: request_id.into(),
            option_id: Some(option_id.into()),
            outcome: None,
        }
    }

    pub fn permission_cancelled(request_id: impl Into<String>) -> Self {
        Self::PermissionResponse {
            request_id: request_id.into(),
            option_id: None,
            outcome: Some(ChatPermissionOutcome::Cancelled),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ChatSessionAction {
    New,
    Resume,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ChatPermissionOutcome {
    Cancelled,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ChatAttachment {
    #[serde(rename = "fileKey")]
    pub file_key: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(rename = "mimeType", skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size: Option<u64>,
}

impl ChatAttachment {
    pub fn new(file_key: impl Into<String>) -> Self {
        Self {
            file_key: file_key.into(),
            name: None,
            mime_type: None,
            size: None,
        }
    }

    pub fn with_name(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }

    pub fn with_mime_type(mut self, mime_type: impl Into<String>) -> Self {
        self.mime_type = Some(mime_type.into());
        self
    }

    pub fn with_size(mut self, size: u64) -> Self {
        self.size = Some(size);
        self
    }
}

pub fn chat_ws() -> WebSocketSpec {
    WebSocketSpec::new("/ws/chat", AuthRequirement::BearerToken)
}

pub fn chat_ws_for_channel(channel: &str) -> WebSocketSpec {
    WebSocketSpec::new(
        path_with_query("/ws/chat", &[("channel", Some(channel.to_string()))]),
        AuthRequirement::BearerToken,
    )
}

pub fn decode_chat_event(value: Value) -> crate::Result<ChatEvent> {
    serde_json::from_value(value).map_err(crate::ClientError::Decode)
}

pub fn encode_chat_client_message(message: &ChatClientMessage) -> crate::Result<String> {
    serde_json::to_string(message).map_err(crate::ClientError::Encode)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn chat_ws_requires_bearer_token() {
        let spec = chat_ws();
        assert_eq!(spec.path, "/ws/chat");
        assert_eq!(spec.auth, AuthRequirement::BearerToken);
    }

    #[test]
    fn chat_ws_channel_adds_query_param() {
        let spec = chat_ws_for_channel("tui");
        assert_eq!(spec.path, "/ws/chat?channel=tui");
        assert_eq!(spec.auth, AuthRequirement::BearerToken);
    }

    #[test]
    fn decodes_chat_event_envelope() {
        let event = decode_chat_event(json!({
            "kind": "session_ready",
            "session_id": "01HX"
        }))
        .expect("event");
        assert_eq!(
            event,
            ChatEvent::SessionReady {
                session_id: "01HX".to_string()
            }
        );
    }

    #[test]
    fn keeps_acp_payload_opaque() {
        let event = decode_chat_event(json!({
            "kind": "acp_notification",
            "payload": { "update": { "sessionUpdate": "anything" } }
        }))
        .expect("event");
        let ChatEvent::AcpNotification { payload } = event else {
            panic!("expected acp notification");
        };
        assert_eq!(payload["update"]["sessionUpdate"], "anything");
    }

    #[test]
    fn encodes_basic_chat_message() {
        let value = serde_json::to_value(ChatClientMessage::message("hello")).expect("json");
        assert_eq!(
            value,
            json!({
                "type": "message",
                "text": "hello"
            })
        );
    }

    #[test]
    fn encodes_resume_message_with_options() {
        let message = ChatClientMessage::Message {
            text: "continue".into(),
            message_id: Some("msg-1".into()),
            agent: Some("codex".into()),
            profile_id: Some("deepseek".into()),
            session_action: Some(ChatSessionAction::Resume),
            session_id: Some("sid-1".into()),
            session_workspace: Some("/tmp/project".into()),
            permission_mode: Some("acceptEdits".into()),
            attachments: vec![ChatAttachment::new("uploads/report.md")
                .with_name("report.md")
                .with_mime_type("text/markdown")
                .with_size(42)],
        };

        let value = serde_json::to_value(message).expect("json");
        assert_eq!(
            value,
            json!({
                "type": "message",
                "text": "continue",
                "messageId": "msg-1",
                "agent": "codex",
                "profileId": "deepseek",
                "sessionAction": "resume",
                "sessionId": "sid-1",
                "sessionWorkspace": "/tmp/project",
                "permissionMode": "acceptEdits",
                "attachments": [{
                    "fileKey": "uploads/report.md",
                    "name": "report.md",
                    "mimeType": "text/markdown",
                    "size": 42
                }]
            })
        );
    }

    #[test]
    fn encodes_session_control_messages() {
        assert_eq!(
            serde_json::to_value(ChatClientMessage::new_session_message("start over"))
                .expect("json"),
            json!({
                "type": "message",
                "text": "start over",
                "sessionAction": "new"
            })
        );
        assert_eq!(
            serde_json::to_value(ChatClientMessage::resume_session("sid-1")).expect("json"),
            json!({
                "type": "resume_session",
                "sessionId": "sid-1"
            })
        );
        assert_eq!(
            serde_json::to_value(ChatClientMessage::resume_session_with_options(
                "sid-1",
                Some("codex".into()),
                Some("deepseek".into()),
                Some("/tmp/project".into()),
            ))
            .expect("json"),
            json!({
                "type": "resume_session",
                "agent": "codex",
                "profileId": "deepseek",
                "sessionId": "sid-1",
                "sessionWorkspace": "/tmp/project"
            })
        );
        assert_eq!(
            serde_json::to_value(ChatClientMessage::stop()).expect("json"),
            json!({
                "type": "stop"
            })
        );
    }

    #[test]
    fn encodes_permission_responses() {
        assert_eq!(
            serde_json::to_value(ChatClientMessage::permission_selected(
                "req-1",
                "allow-once"
            ))
            .expect("json"),
            json!({
                "type": "permission_response",
                "requestId": "req-1",
                "optionId": "allow-once"
            })
        );
        assert_eq!(
            serde_json::to_value(ChatClientMessage::permission_cancelled("req-2")).expect("json"),
            json!({
                "type": "permission_response",
                "requestId": "req-2",
                "outcome": "cancelled"
            })
        );
    }

    #[test]
    fn encodes_chat_client_message_string() {
        let text = encode_chat_client_message(&ChatClientMessage::set_mode("bypassPermissions"))
            .expect("json");
        assert_eq!(text, r#"{"type":"set_mode","modeId":"bypassPermissions"}"#);
    }
}
