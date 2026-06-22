use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::http::path_with_query;
use crate::http::AuthRequirement;
use crate::runtime::{AgentInfo, AgentRuntime, ChannelRuntime, TunnelRuntime};

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

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum PtyClientMessage {
    Resize { cols: u16, rows: u16 },
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BridgeRecordEvent {
    pub record_id: u64,
    pub request_id: String,
    pub phase: BridgeRecordPhase,
    pub timestamp_ms: u128,
    pub metadata: Option<BridgeRecordMetadata>,
    pub original_request: Option<RecordedPayload>,
    pub bridge_request: Option<RecordedPayload>,
    pub server_response: Option<RecordedPayload>,
    pub bridge_response: Option<RecordedPayload>,
    pub search: Option<RecordedPayload>,
    pub error: Option<String>,
    pub status: Option<u16>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BridgeRecordMetadata {
    pub profile_id: String,
    pub route_scope: Option<String>,
    pub manual_scope: Option<String>,
    pub target_api_type: String,
    pub client_protocol: String,
    pub upstream_protocol: Option<String>,
    pub upstream_url: Option<String>,
    pub stream: Option<bool>,
    pub model: Option<String>,
    pub passthrough: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum BridgeRecordPhase {
    Start,
    BridgeRequest,
    ServerResponse,
    BridgeResponse,
    Search,
    Error,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecordedPayload {
    pub byte_length: usize,
    pub truncated: bool,
    pub text: String,
    pub json: Option<Value>,
}

pub fn chat_ws() -> WebSocketSpec {
    WebSocketSpec::new("/ws/chat", AuthRequirement::BearerToken)
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

pub fn decode_channels_event(value: Value) -> crate::Result<Vec<ChannelRuntime>> {
    serde_json::from_value(value).map_err(crate::ClientError::Decode)
}

pub fn decode_tunnels_event(value: Value) -> crate::Result<Vec<TunnelRuntime>> {
    serde_json::from_value(value).map_err(crate::ClientError::Decode)
}

pub fn decode_agents_runtime_event(value: Value) -> crate::Result<Vec<AgentRuntime>> {
    serde_json::from_value(value).map_err(crate::ClientError::Decode)
}

pub fn decode_bridge_record_event(value: Value) -> crate::Result<BridgeRecordEvent> {
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
    fn decodes_domain_snapshot_events() {
        let channels = decode_channels_event(json!([{
            "kind": "feishu",
            "version": "1.0.0",
            "plugin_dir": null,
            "status": "running",
            "reason": null
        }]))
        .expect("channels");
        assert_eq!(channels[0].kind, "feishu");

        let tunnels = decode_tunnels_event(json!([{
            "provider": "cloudflare",
            "url": "https://example.test",
            "status": { "state": "running" },
            "uptime_secs": 12
        }]))
        .expect("tunnels");
        assert_eq!(tunnels[0].provider, "cloudflare");

        let agents = decode_agents_runtime_event(json!([{
            "route_key": "workspace-thread",
            "channel_kind": "workspace",
            "chat_id": "workspace-thread",
            "attached_routes": [],
            "cli_kind": "codex",
            "profile": null,
            "profile_label": null,
            "session_id": null,
            "workspace": "/tmp/project",
            "busy": false,
            "failed": null,
            "started_at": 0,
            "agent_name": "Codex",
            "agent_title": null,
            "agent_version": "1.0.0",
            "multi_agent_turns": [],
            "subagents": []
        }]))
        .expect("agents");
        assert_eq!(agents[0].channel_kind, "workspace");
    }

    #[test]
    fn decodes_bridge_record_event() {
        let event = decode_bridge_record_event(json!({
            "recordId": 7,
            "requestId": "req-1",
            "phase": "serverResponse",
            "timestampMs": 123,
            "metadata": null,
            "originalRequest": null,
            "bridgeRequest": null,
            "serverResponse": {
                "byteLength": 2,
                "truncated": false,
                "text": "{}",
                "json": {}
            },
            "bridgeResponse": null,
            "search": null,
            "error": null,
            "status": 200
        }))
        .expect("bridge event");

        assert_eq!(event.phase, BridgeRecordPhase::ServerResponse);
        assert_eq!(event.status, Some(200));
    }
}
