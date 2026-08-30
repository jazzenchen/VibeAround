use std::io;
use std::pin::Pin;

use axum::body::Body;
use axum::http::{HeaderName, HeaderValue, Response, StatusCode};
use bytes::Bytes;
use futures_util::{Stream, StreamExt};

use super::super::bridge_recording::{ActiveBridgeRecord, PayloadCapture, RecordedPayload};
use super::json_error;

type UpstreamByteStream =
    Pin<Box<dyn Stream<Item = Result<Bytes, reqwest::Error>> + Send + 'static>>;

pub(super) async fn buffered_passthrough_response(
    upstream: reqwest::Response,
    record: Option<&ActiveBridgeRecord>,
) -> Response<Body> {
    let status =
        StatusCode::from_u16(upstream.status().as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);
    let builder = response_builder(&upstream, status);
    let bytes = match upstream.bytes().await {
        Ok(bytes) => bytes,
        Err(error) => {
            return json_error(
                StatusCode::BAD_GATEWAY,
                &format!("failed to read upstream passthrough response: {error}"),
            );
        }
    };
    if let Some(record) = record {
        let payload = RecordedPayload::from_bytes(&bytes);
        record.server_response(status.as_u16(), payload.clone());
        record.bridge_response(status.as_u16(), payload);
    }
    builder.body(Body::from(bytes)).unwrap_or_else(|_| {
        json_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "failed to build buffered passthrough bridge response",
        )
    })
}

pub(super) fn passthrough_response(
    upstream: reqwest::Response,
    record: Option<ActiveBridgeRecord>,
) -> Response<Body> {
    let status =
        StatusCode::from_u16(upstream.status().as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);
    let builder = response_builder(&upstream, status);
    let stream = passthrough_stream(upstream, status.as_u16(), record);
    builder.body(Body::from_stream(stream)).unwrap_or_else(|_| {
        json_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "failed to build passthrough bridge response",
        )
    })
}

fn passthrough_stream(
    upstream: reqwest::Response,
    status: u16,
    record: Option<ActiveBridgeRecord>,
) -> impl Stream<Item = Result<Bytes, io::Error>> + Send + 'static {
    let state = PassthroughStreamState {
        upstream: Box::pin(upstream.bytes_stream()),
        status,
        record,
        capture: PayloadCapture::new(),
    };
    futures_util::stream::unfold(state, |mut state| async move {
        match state.upstream.next().await {
            Some(Ok(bytes)) => {
                state.capture.push_bytes(&bytes);
                Some((Ok(bytes), state))
            }
            Some(Err(error)) => {
                if let Some(record) = state.record.as_ref() {
                    record.error(&format!("upstream passthrough stream error: {error}"));
                }
                Some((
                    Err(io::Error::other(format!(
                        "upstream passthrough stream error: {error}"
                    ))),
                    state,
                ))
            }
            None => {
                state.finish_recording();
                None
            }
        }
    })
}

struct PassthroughStreamState {
    upstream: UpstreamByteStream,
    status: u16,
    record: Option<ActiveBridgeRecord>,
    capture: PayloadCapture,
}

impl PassthroughStreamState {
    fn finish_recording(&mut self) {
        let Some(record) = self.record.take() else {
            return;
        };
        let payload = std::mem::take(&mut self.capture).into_payload();
        record.server_response(self.status, payload.clone());
        record.bridge_response(self.status, payload);
    }
}

impl Drop for PassthroughStreamState {
    fn drop(&mut self) {
        self.finish_recording();
    }
}

fn response_builder(
    upstream: &reqwest::Response,
    status: StatusCode,
) -> axum::http::response::Builder {
    let mut builder = Response::builder().status(status);
    for (name, value) in upstream.headers() {
        if !should_forward_response_header(name.as_str()) {
            continue;
        }
        let Ok(header_name) = HeaderName::from_bytes(name.as_str().as_bytes()) else {
            continue;
        };
        let Ok(header_value) = HeaderValue::from_bytes(value.as_bytes()) else {
            continue;
        };
        builder = builder.header(header_name, header_value);
    }
    builder
}

fn should_forward_response_header(name: &str) -> bool {
    !matches!(
        name.to_ascii_lowercase().as_str(),
        "connection"
            | "keep-alive"
            | "proxy-authenticate"
            | "proxy-authorization"
            | "te"
            | "trailer"
            | "transfer-encoding"
            | "upgrade"
            | "content-length"
    )
}
