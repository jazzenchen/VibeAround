use serde::Deserialize;
use serde_json::Value;

use crate::http::AuthRequirement;
use crate::runtime::AgentInfo;

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

pub fn chat_ws() -> WebSocketSpec {
    WebSocketSpec::new("/ws/chat", AuthRequirement::BearerToken)
}

pub fn pty_ws() -> WebSocketSpec {
    WebSocketSpec::new("/ws", AuthRequirement::BearerToken)
}

pub fn channels_ws() -> WebSocketSpec {
    WebSocketSpec::new("/ws/channels", AuthRequirement::BearerToken)
}

pub fn tunnels_ws() -> WebSocketSpec {
    WebSocketSpec::new("/ws/tunnels", AuthRequirement::BearerToken)
}

pub fn agents_runtime_ws() -> WebSocketSpec {
    WebSocketSpec::new("/ws/agents/runtime", AuthRequirement::BearerToken)
}

pub fn bridge_recording_ws() -> WebSocketSpec {
    WebSocketSpec::new("/ws/bridge-recording", AuthRequirement::BearerToken)
}

pub fn decode_chat_event(value: Value) -> crate::Result<ChatEvent> {
    serde_json::from_value(value).map_err(crate::ClientError::Decode)
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
}
