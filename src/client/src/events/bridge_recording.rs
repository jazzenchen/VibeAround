use serde::Deserialize;
use serde_json::Value;

use super::WebSocketSpec;
use crate::http::AuthRequirement;

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

pub fn bridge_recording_ws() -> WebSocketSpec {
    WebSocketSpec::new("/ws/bridge-recording", AuthRequirement::BearerToken)
}

pub fn decode_bridge_record_event(value: Value) -> crate::Result<BridgeRecordEvent> {
    serde_json::from_value(value).map_err(crate::ClientError::Decode)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

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
