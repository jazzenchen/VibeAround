use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use axum::extract::{
    ws::{Message, WebSocket, WebSocketUpgrade},
    State,
};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use bytes::Bytes;
use futures_util::{SinkExt, StreamExt};
use serde::Serialize;
use serde_json::Value;
use tokio::sync::mpsc;

use super::AppState;

const MAX_CAPTURE_BYTES: usize = 4 * 1024 * 1024;
const SUBSCRIBER_QUEUE_CAPACITY: usize = 256;

#[derive(Clone, Default)]
pub(crate) struct BridgeRecorder {
    inner: Arc<BridgeRecorderInner>,
}

#[derive(Default)]
struct BridgeRecorderInner {
    next_record_id: AtomicU64,
    next_subscriber_id: AtomicU64,
    subscribers: Mutex<Vec<BridgeRecordSubscriber>>,
}

struct BridgeRecordSubscriber {
    id: u64,
    sender: mpsc::Sender<BridgeRecordEvent>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct BridgeRecordEvent {
    pub(crate) record_id: u64,
    pub(crate) request_id: String,
    pub(crate) phase: BridgeRecordPhase,
    pub(crate) timestamp_ms: u128,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) metadata: Option<BridgeRecordMetadata>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) original_request: Option<RecordedPayload>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) bridge_request: Option<RecordedPayload>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) server_response: Option<RecordedPayload>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) bridge_response: Option<RecordedPayload>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) service_side: Option<RecordedPayload>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) search: Option<RecordedPayload>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) status: Option<u16>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct BridgeRecordMetadata {
    pub(crate) profile_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) route_scope: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) manual_scope: Option<String>,
    pub(crate) target_api_type: String,
    pub(crate) client_protocol: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) upstream_protocol: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) upstream_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) stream: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) model: Option<String>,
    pub(crate) passthrough: bool,
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum BridgeRecordPhase {
    Start,
    BridgeRequest,
    ServerResponse,
    BridgeResponse,
    ServiceSide,
    Search,
    Error,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RecordedPayload {
    pub(crate) byte_length: usize,
    pub(crate) truncated: bool,
    pub(crate) text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) json: Option<Value>,
}

pub(crate) struct ActiveBridgeRecord {
    recorder: BridgeRecorder,
    record_id: u64,
    request_id: String,
}

pub(crate) struct BridgeRecordSubscription {
    id: u64,
    receiver: mpsc::Receiver<BridgeRecordEvent>,
    recorder: BridgeRecorder,
}

#[derive(Default)]
pub(crate) struct PayloadCapture {
    byte_length: usize,
    bytes: Vec<u8>,
    truncated: bool,
}

impl BridgeRecorder {
    pub(crate) fn subscriber_count(&self) -> usize {
        self.inner
            .subscribers
            .lock()
            .map(|subscribers| subscribers.len())
            .unwrap_or_default()
    }

    pub(crate) fn has_subscribers(&self) -> bool {
        self.subscriber_count() > 0
    }

    pub(crate) fn begin(
        &self,
        request_id: String,
        metadata: BridgeRecordMetadata,
        original_request: Option<RecordedPayload>,
    ) -> Option<ActiveBridgeRecord> {
        if !self.has_subscribers() {
            return None;
        }
        let record = ActiveBridgeRecord {
            recorder: self.clone(),
            record_id: self.inner.next_record_id.fetch_add(1, Ordering::Relaxed),
            request_id,
        };
        record.emit(BridgeRecordEvent {
            record_id: record.record_id,
            request_id: record.request_id.clone(),
            phase: BridgeRecordPhase::Start,
            timestamp_ms: timestamp_ms(),
            metadata: Some(metadata),
            original_request,
            bridge_request: None,
            server_response: None,
            bridge_response: None,
            service_side: None,
            search: None,
            error: None,
            status: None,
        });
        Some(record)
    }

    fn send(&self, event: BridgeRecordEvent) {
        let Ok(mut subscribers) = self.inner.subscribers.lock() else {
            return;
        };
        subscribers.retain(|subscriber| {
            if subscriber.sender.is_closed() {
                return false;
            }
            match subscriber.sender.try_send(event.clone()) {
                Ok(()) => true,
                Err(mpsc::error::TrySendError::Full(_)) => true,
                Err(mpsc::error::TrySendError::Closed(_)) => false,
            }
        });
    }

    pub(crate) fn subscribe(&self) -> BridgeRecordSubscription {
        let (sender, receiver) = mpsc::channel(SUBSCRIBER_QUEUE_CAPACITY);
        let id = self
            .inner
            .next_subscriber_id
            .fetch_add(1, Ordering::Relaxed);
        if let Ok(mut subscribers) = self.inner.subscribers.lock() {
            subscribers.push(BridgeRecordSubscriber { id, sender });
        }
        BridgeRecordSubscription {
            id,
            receiver,
            recorder: self.clone(),
        }
    }

    fn unsubscribe(&self, id: u64) {
        if let Ok(mut subscribers) = self.inner.subscribers.lock() {
            subscribers.retain(|subscriber| subscriber.id != id);
        }
    }
}

impl ActiveBridgeRecord {
    pub(crate) fn bridge_request(&self, metadata: BridgeRecordMetadata, body: &Value) {
        self.emit(BridgeRecordEvent {
            record_id: self.record_id,
            request_id: self.request_id.clone(),
            phase: BridgeRecordPhase::BridgeRequest,
            timestamp_ms: timestamp_ms(),
            metadata: Some(metadata),
            original_request: None,
            bridge_request: Some(RecordedPayload::from_json(body)),
            server_response: None,
            bridge_response: None,
            service_side: None,
            search: None,
            error: None,
            status: None,
        });
    }

    pub(crate) fn server_response(&self, status: u16, payload: RecordedPayload) {
        self.emit(BridgeRecordEvent {
            record_id: self.record_id,
            request_id: self.request_id.clone(),
            phase: BridgeRecordPhase::ServerResponse,
            timestamp_ms: timestamp_ms(),
            metadata: None,
            original_request: None,
            bridge_request: None,
            server_response: Some(payload),
            bridge_response: None,
            service_side: None,
            search: None,
            error: None,
            status: Some(status),
        });
    }

    pub(crate) fn bridge_response(&self, status: u16, payload: RecordedPayload) {
        self.emit(BridgeRecordEvent {
            record_id: self.record_id,
            request_id: self.request_id.clone(),
            phase: BridgeRecordPhase::BridgeResponse,
            timestamp_ms: timestamp_ms(),
            metadata: None,
            original_request: None,
            bridge_request: None,
            server_response: None,
            bridge_response: Some(payload),
            service_side: None,
            search: None,
            error: None,
            status: Some(status),
        });
    }

    pub(crate) fn bridge_json_response(&self, status: StatusCode, body: &Value) {
        self.bridge_response(status.as_u16(), RecordedPayload::from_json(body));
    }

    pub(crate) fn search(&self, body: &Value) {
        self.emit(BridgeRecordEvent {
            record_id: self.record_id,
            request_id: self.request_id.clone(),
            phase: BridgeRecordPhase::Search,
            timestamp_ms: timestamp_ms(),
            metadata: None,
            original_request: None,
            bridge_request: None,
            server_response: None,
            bridge_response: None,
            service_side: None,
            search: Some(RecordedPayload::from_json(body)),
            error: None,
            status: None,
        });
    }

    pub(crate) fn service_side(&self, body: &Value) {
        self.emit(BridgeRecordEvent {
            record_id: self.record_id,
            request_id: self.request_id.clone(),
            phase: BridgeRecordPhase::ServiceSide,
            timestamp_ms: timestamp_ms(),
            metadata: None,
            original_request: None,
            bridge_request: None,
            server_response: None,
            bridge_response: None,
            service_side: Some(RecordedPayload::from_json(body)),
            search: None,
            error: None,
            status: None,
        });
    }

    pub(crate) fn error(&self, message: &str) {
        self.emit(BridgeRecordEvent {
            record_id: self.record_id,
            request_id: self.request_id.clone(),
            phase: BridgeRecordPhase::Error,
            timestamp_ms: timestamp_ms(),
            metadata: None,
            original_request: None,
            bridge_request: None,
            server_response: None,
            bridge_response: None,
            service_side: None,
            search: None,
            error: Some(message.to_string()),
            status: None,
        });
    }

    fn emit(&self, event: BridgeRecordEvent) {
        self.recorder.send(event);
    }
}

impl Clone for ActiveBridgeRecord {
    fn clone(&self) -> Self {
        Self {
            recorder: self.recorder.clone(),
            record_id: self.record_id,
            request_id: self.request_id.clone(),
        }
    }
}

impl BridgeRecordSubscription {
    pub(crate) async fn recv(&mut self) -> Option<BridgeRecordEvent> {
        self.receiver.recv().await
    }
}

impl Drop for BridgeRecordSubscription {
    fn drop(&mut self) {
        self.recorder.unsubscribe(self.id);
    }
}

impl RecordedPayload {
    pub(crate) fn from_json(value: &Value) -> Self {
        let text = serde_json::to_string_pretty(value).unwrap_or_else(|_| value.to_string());
        Self::from_text(text, Some(value.clone()))
    }

    pub(crate) fn from_bytes(bytes: &[u8]) -> Self {
        let byte_length = bytes.len();
        let truncated = byte_length > MAX_CAPTURE_BYTES;
        let captured = if truncated {
            &bytes[..MAX_CAPTURE_BYTES]
        } else {
            bytes
        };
        let text = String::from_utf8_lossy(captured).to_string();
        let json = if !truncated {
            serde_json::from_slice::<Value>(bytes).ok()
        } else {
            None
        };
        Self {
            byte_length,
            truncated,
            text,
            json,
        }
    }

    fn from_text(text: String, json: Option<Value>) -> Self {
        let byte_length = text.len();
        let truncated = byte_length > MAX_CAPTURE_BYTES;
        let text = if truncated {
            text.chars().take(MAX_CAPTURE_BYTES).collect()
        } else {
            text
        };
        Self {
            byte_length,
            truncated,
            text,
            json: if truncated { None } else { json },
        }
    }
}

impl PayloadCapture {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    pub(crate) fn push(&mut self, bytes: &[u8]) {
        self.byte_length += bytes.len();
        if self.bytes.len() >= MAX_CAPTURE_BYTES {
            self.truncated = true;
            return;
        }
        let remaining = MAX_CAPTURE_BYTES - self.bytes.len();
        let take = remaining.min(bytes.len());
        self.bytes.extend_from_slice(&bytes[..take]);
        if take < bytes.len() {
            self.truncated = true;
        }
    }

    pub(crate) fn push_bytes(&mut self, bytes: &Bytes) {
        self.push(bytes);
    }

    pub(crate) fn into_payload(self) -> RecordedPayload {
        let text = String::from_utf8_lossy(&self.bytes).to_string();
        let json = if !self.truncated {
            serde_json::from_slice::<Value>(&self.bytes).ok()
        } else {
            None
        };
        RecordedPayload {
            byte_length: self.byte_length,
            truncated: self.truncated,
            text,
            json,
        }
    }
}

pub(crate) async fn bridge_recording_ws_handler(
    State(state): State<AppState>,
    headers: HeaderMap,
    ws: WebSocketUpgrade,
) -> Response {
    if !allowed_ws_origin(&state, &headers) {
        return StatusCode::FORBIDDEN.into_response();
    }
    ws.on_upgrade(move |socket| handle_bridge_recording_socket(socket, state.bridge_recorder))
}

async fn handle_bridge_recording_socket(socket: WebSocket, recorder: BridgeRecorder) {
    let mut subscription = recorder.subscribe();
    let (mut sender, mut receiver) = socket.split();
    loop {
        tokio::select! {
            event = subscription.recv() => {
                let Some(event) = event else {
                    break;
                };
                let Ok(text) = serde_json::to_string(&event) else {
                    continue;
                };
                if sender.send(Message::Text(text.into())).await.is_err() {
                    break;
                }
            }
            message = receiver.next() => match message {
                Some(Ok(Message::Close(_))) | None => break,
                Some(Ok(Message::Ping(data))) => {
                    let _ = sender.send(Message::Pong(data)).await;
                }
                _ => {}
            },
        }
    }
}

fn allowed_ws_origin(state: &AppState, headers: &HeaderMap) -> bool {
    let tunnel_urls = state.tunnels.public_urls();
    super::auth::headers_have_allowed_dashboard_origin(headers, state.port, &tunnel_urls)
}

fn timestamp_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[tokio::test]
    async fn service_side_trace_stays_on_parent_bridge_record() {
        let recorder = BridgeRecorder::default();
        let mut subscription = recorder.subscribe();
        let record = recorder
            .begin(
                "request-id".to_string(),
                BridgeRecordMetadata {
                    profile_id: "deepseek".to_string(),
                    route_scope: None,
                    manual_scope: None,
                    target_api_type: "openai-chat".to_string(),
                    client_protocol: "anthropic".to_string(),
                    upstream_protocol: None,
                    upstream_url: None,
                    stream: Some(false),
                    model: Some("deepseek-chat".to_string()),
                    passthrough: false,
                },
                None,
            )
            .expect("subscriber enables recording");
        let _start = subscription.recv().await.expect("start event");

        record.service_side(&json!({
            "capability": "imageInput",
            "stage": "request",
        }));
        let event = subscription.recv().await.expect("service-side event");

        assert_eq!(event.request_id, "request-id");
        assert!(matches!(event.phase, BridgeRecordPhase::ServiceSide));
        assert_eq!(
            event
                .service_side
                .and_then(|payload| payload.json)
                .and_then(|value| value.get("capability").cloned()),
            Some(Value::String("imageInput".to_string()))
        );
    }
}
