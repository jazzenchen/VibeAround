use std::collections::VecDeque;
use std::io;
use std::pin::Pin;

use axum::body::Body;
use axum::http::{header, StatusCode};
use axum::response::Response;
use bytes::Bytes;
use futures_util::{Stream, StreamExt};
use serde_json::Value;
use va_ai_api_bridge::{
    DecodeState, EncodeState, ProviderBridgeAdapter, UniversalEvent, WireEvent,
};

use super::completion::{transform_upstream_response, UpstreamResponseTransform};
use super::{json_error, BridgeProtocol};

type UpstreamByteStream =
    Pin<Box<dyn Stream<Item = Result<Bytes, reqwest::Error>> + Send + 'static>>;

pub(super) fn translated_stream_response(
    upstream: reqwest::Response,
    upstream_protocol: BridgeProtocol,
    agent_protocol: BridgeProtocol,
    provider_adapter: ProviderBridgeAdapter,
    agent_model: Option<String>,
    transform: UpstreamResponseTransform,
) -> Response {
    let stream = map_sse_stream(
        upstream,
        upstream_protocol,
        agent_protocol,
        provider_adapter,
        agent_model,
        transform,
    );
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "text/event-stream")
        .header(header::CACHE_CONTROL, "no-cache")
        .body(Body::from_stream(stream))
        .unwrap_or_else(|_| {
            json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "failed to build bridge stream response",
            )
        })
}

fn map_sse_stream(
    upstream: reqwest::Response,
    upstream_protocol: BridgeProtocol,
    agent_protocol: BridgeProtocol,
    provider_adapter: ProviderBridgeAdapter,
    agent_model: Option<String>,
    transform: UpstreamResponseTransform,
) -> impl Stream<Item = Result<Bytes, io::Error>> + Send + 'static {
    let state = SseMapState {
        upstream: Box::pin(upstream.bytes_stream()),
        upstream_protocol,
        agent_protocol,
        provider_adapter,
        agent_model,
        transform,
        decode_state: DecodeState::default(),
        encode_state: EncodeState::default(),
        buffer: Vec::new(),
        queue: VecDeque::new(),
        done: false,
    };

    futures_util::stream::unfold(state, |mut state| async move {
        loop {
            if let Some(item) = state.queue.pop_front() {
                return Some((item, state));
            }
            if state.done {
                return None;
            }

            match state.upstream.next().await {
                Some(Ok(chunk)) => state.ingest_chunk(&chunk),
                Some(Err(e)) => {
                    state.done = true;
                    return Some((
                        Err(io::Error::new(
                            io::ErrorKind::Other,
                            format!("upstream stream error: {e}"),
                        )),
                        state,
                    ));
                }
                None => return None,
            }
        }
    })
}

struct SseMapState {
    upstream: UpstreamByteStream,
    upstream_protocol: BridgeProtocol,
    agent_protocol: BridgeProtocol,
    provider_adapter: ProviderBridgeAdapter,
    agent_model: Option<String>,
    transform: UpstreamResponseTransform,
    decode_state: DecodeState,
    encode_state: EncodeState,
    buffer: Vec<u8>,
    queue: VecDeque<Result<Bytes, io::Error>>,
    done: bool,
}

impl SseMapState {
    fn ingest_chunk(&mut self, chunk: &[u8]) {
        self.buffer.extend_from_slice(chunk);
        while let Some(end) = find_sse_frame_end(&self.buffer) {
            let frame: Vec<u8> = self.buffer.drain(..end).collect();
            self.handle_frame(&frame);
            if self.done {
                break;
            }
        }
    }

    fn handle_frame(&mut self, frame: &[u8]) {
        let Some(data) = sse_data(frame) else {
            return;
        };
        if data.trim().is_empty() {
            return;
        }
        if data.trim() == "[DONE]" {
            self.done = true;
            return;
        }

        let raw = match serde_json::from_str::<Value>(&data) {
            Ok(value) => value,
            Err(e) => {
                self.fail(format!("upstream sent invalid SSE JSON: {e}"));
                return;
            }
        };
        let mut raw = match transform_upstream_response(raw, self.transform) {
            Ok(value) => value,
            Err(message) => {
                self.fail(message);
                return;
            }
        };
        if self.upstream_protocol == BridgeProtocol::OpenAiChat {
            self.provider_adapter.normalize_chat_response(&mut raw);
        }
        let mut events = match self
            .upstream_protocol
            .decode_upstream_stream_chunk(raw, &mut self.decode_state)
        {
            Ok(events) => events,
            Err(error) => {
                self.fail(error.to_string());
                return;
            }
        };
        self.provider_adapter.transform_upstream_events(&mut events);
        apply_agent_model(&mut events, self.agent_model.as_deref());
        let wire_events = match self
            .agent_protocol
            .encode_agent_events(&events, &mut self.encode_state)
        {
            Ok(events) => events,
            Err(error) => {
                self.fail(error.to_string());
                return;
            }
        };
        for event in wire_events {
            self.queue
                .push_back(Ok(Bytes::from(encode_wire_sse_event(event))));
        }
    }

    fn fail(&mut self, message: String) {
        self.done = true;
        self.queue
            .push_back(Err(io::Error::new(io::ErrorKind::InvalidData, message)));
    }
}

fn apply_agent_model(events: &mut [UniversalEvent], agent_model: Option<&str>) {
    let Some(agent_model) = agent_model else {
        return;
    };
    for event in events {
        if let UniversalEvent::ResponseStart { model, .. } = event {
            *model = Some(agent_model.to_string());
        }
    }
}

fn sse_data(frame: &[u8]) -> Option<String> {
    let frame = String::from_utf8_lossy(frame);
    let data = frame
        .lines()
        .filter_map(|line| line.strip_prefix("data:"))
        .map(str::trim_start)
        .collect::<Vec<_>>()
        .join("\n");
    if data.is_empty() {
        None
    } else {
        Some(data)
    }
}

fn encode_wire_sse_event(event: WireEvent) -> String {
    let event_name = event.event.or_else(|| {
        event
            .data
            .get("type")
            .and_then(Value::as_str)
            .map(ToString::to_string)
    });
    let mut out = String::new();
    if let Some(event_name) = event_name {
        out.push_str("event: ");
        out.push_str(&event_name);
        out.push('\n');
    }
    out.push_str("data: ");
    out.push_str(&event.data.to_string());
    out.push_str("\n\n");
    out
}

fn find_sse_frame_end(buffer: &[u8]) -> Option<usize> {
    if buffer.len() >= 2 {
        for index in 0..buffer.len() - 1 {
            if buffer[index] == b'\n' && buffer[index + 1] == b'\n' {
                return Some(index + 2);
            }
        }
    }
    if buffer.len() >= 4 {
        for index in 0..buffer.len() - 3 {
            if &buffer[index..index + 4] == b"\r\n\r\n" {
                return Some(index + 4);
            }
        }
    }
    None
}
