//! Same-origin page-preview proxy for remote Server previews.
//!
//! An owner `/content` route or an authorized public share selects one live
//! Server preview by setting a root-scoped routing cookie. Root requests then
//! retain their original path and query while this module forwards them to the
//! selected `127.0.0.1` dev server. Owner routing stays signed to the daemon
//! and transparently forwards ordinary HTTP traffic. Share routing carries the
//! existing browser grant and retains its narrower read-only behavior.

use axum::body::Body;
use axum::extract::ws::{Message as ClientMessage, WebSocket, WebSocketUpgrade};
use axum::extract::{Extension, FromRequestParts, Request};
use axum::http::{header, Method, StatusCode};
use axum::response::{IntoResponse, Redirect, Response};
use futures_util::{SinkExt, StreamExt};
use sha2::{Digest, Sha256};
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::Message as UpstreamMessage;

use common::previews::PreviewTarget;

use crate::web_server::auth::AuthState;

use super::access::extract_cookie;
use super::toolbar::{escape_html, remaining_millis};

const SERVER_ROUTING_COOKIE: &str = "va_preview_server";
const SHARE_ROUTE_PREFIX: &str = "share:";

#[derive(Clone)]
pub(in crate::web_server) struct ServerProxyState {
    client: reqwest::Client,
}

impl ServerProxyState {
    pub(in crate::web_server) fn new(client: reqwest::Client) -> Self {
        Self { client }
    }
}

pub(super) fn owner_server_content_response(slug: &str, req: &Request) -> Response {
    let Some(auth) = req.extensions().get::<AuthState>() else {
        return StatusCode::UNAUTHORIZED.into_response();
    };
    Response::builder()
        .status(StatusCode::FOUND)
        .header(header::LOCATION, "/")
        .header(header::SET_COOKIE, server_routing_cookie(slug, auth))
        .body(Body::empty())
        .expect("valid Server Preview redirect")
}

pub(super) fn share_server_content_response(
    share_id: &str,
    grant: &str,
    entry: &common::previews::PreviewEntry,
) -> Response {
    let PreviewTarget::Server { .. } = &entry.target else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    let title = escape_html(&entry.title);
    let html = format!(
        r#"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>Preview — {title}</title>
<style>
  html, body, iframe {{ width: 100%; height: 100%; margin: 0; border: 0; }}
  body {{ overflow: hidden; background: #fff; }}
</style>
</head>
<body><iframe src="/" title="{title}"></iframe></body>
</html>"#,
    );
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "text/html; charset=utf-8")
        .header(
            "Content-Security-Policy",
            "default-src 'none'; style-src 'unsafe-inline'; frame-src 'self'; base-uri 'none'; form-action 'none'; frame-ancestors 'none'",
        )
        .header("Referrer-Policy", "no-referrer")
        .header("X-Content-Type-Options", "nosniff")
        .header(
            header::SET_COOKIE,
            share_server_routing_cookie(share_id, grant, entry),
        )
        .body(Body::from(html))
        .expect("valid shared Server Preview response")
}

pub(super) fn clear_server_routing_cookie() -> String {
    format!("{SERVER_ROUTING_COOKIE}=; Path=/; Max-Age=0; Secure; HttpOnly; SameSite=Lax")
}

/// Root fallback used outside `/va/`. Missing or invalid routing state returns
/// to the dashboard; accepted requests retain path/query.
pub(in crate::web_server) async fn server_proxy_fallback(
    state: Option<Extension<ServerProxyState>>,
    req: Request,
) -> Response {
    let Some(Extension(state)) = state else {
        return Redirect::temporary("/va/").into_response();
    };
    proxy_request(&state.client, req).await
}

async fn proxy_request(client: &reqwest::Client, req: Request) -> Response {
    let mut response = proxy_request_inner(client, req).await;
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        "no-store".parse().expect("valid cache-control"),
    );
    response
}

async fn proxy_request_inner(client: &reqwest::Client, req: Request) -> Response {
    if req.uri().path() == "/va" || req.uri().path().starts_with("/va/") {
        return Redirect::temporary("/va/").into_response();
    }
    let service_worker = req
        .headers()
        .get("sec-fetch-dest")
        .and_then(|value| value.to_str().ok())
        .is_some_and(|destination| destination == "serviceworker");
    if service_worker {
        return StatusCode::FORBIDDEN.into_response();
    }

    let Some(auth) = req.extensions().get::<AuthState>().cloned() else {
        return Redirect::temporary("/va/").into_response();
    };
    let Some(cookie) = extract_cookie(&req, SERVER_ROUTING_COOKIE) else {
        return Redirect::temporary("/va/").into_response();
    };
    let Some(route) = verified_server_entry(&cookie, &auth) else {
        return invalid_cookie_response();
    };
    let PreviewTarget::Server { port } = route.entry.target else {
        return invalid_cookie_response();
    };

    match route.kind {
        ServerRouteKind::Owner if req.headers().contains_key(header::UPGRADE) => {
            proxy_owner_websocket(req, port, &auth).await
        }
        ServerRouteKind::Owner => proxy_owner_http(client, req, port, &auth).await,
        ServerRouteKind::Share => proxy_share_request(client, req, port).await,
    }
}

async fn proxy_owner_http(
    client: &reqwest::Client,
    req: Request,
    port: u16,
    auth: &AuthState,
) -> Response {
    let path_and_query = req
        .uri()
        .path_and_query()
        .map(|value| value.as_str())
        .unwrap_or("/");
    let url = format!("http://127.0.0.1:{port}{path_and_query}");
    let method = req.method().clone();
    let headers = req.headers().clone();
    let body = reqwest::Body::wrap_stream(req.into_body().into_data_stream());
    let mut upstream_request = client.request(method, url).body(body);

    for (name, value) in &headers {
        if !is_hop_by_hop_header(name) && *name != header::HOST && *name != header::COOKIE {
            if *name != header::AUTHORIZATION || !is_daemon_authorization(value, auth) {
                upstream_request = upstream_request.header(name, value);
            }
        }
    }
    if let Some(cookie) = application_cookie_header(&headers) {
        upstream_request = upstream_request.header(header::COOKIE, cookie);
    }

    match upstream_request.send().await {
        Ok(upstream) => transparent_upstream_response(upstream),
        Err(error) => upstream_error(port, error),
    }
}

async fn proxy_owner_websocket(req: Request, port: u16, auth: &AuthState) -> Response {
    let path_and_query = req
        .uri()
        .path_and_query()
        .map(|value| value.as_str())
        .unwrap_or("/")
        .to_string();
    let headers = req.headers().clone();
    let (mut parts, _) = req.into_parts();
    let upgrade = match WebSocketUpgrade::from_request_parts(&mut parts, &()).await {
        Ok(upgrade) => upgrade,
        Err(rejection) => return rejection.into_response(),
    };
    let upstream_request = match upstream_websocket_request(&path_and_query, &headers, port, auth) {
        Ok(request) => request,
        Err(error) => return (StatusCode::BAD_REQUEST, error).into_response(),
    };
    let (upstream, response) = match tokio_tungstenite::connect_async(upstream_request).await {
        Ok(connection) => connection,
        Err(error) => {
            return (
                StatusCode::BAD_GATEWAY,
                format!("Server Preview upstream 127.0.0.1:{port} WebSocket error: {error}"),
            )
                .into_response()
        }
    };
    let protocol = response
        .headers()
        .get(header::SEC_WEBSOCKET_PROTOCOL)
        .and_then(|value| value.to_str().ok())
        .map(str::to_string);
    let upgrade = match protocol {
        Some(protocol) => upgrade.protocols([protocol]),
        None => upgrade,
    };
    upgrade.on_upgrade(move |client| bridge_websockets(client, upstream))
}

fn upstream_websocket_request(
    path_and_query: &str,
    headers: &header::HeaderMap,
    port: u16,
    auth: &AuthState,
) -> Result<tokio_tungstenite::tungstenite::http::Request<()>, String> {
    let mut request = format!("ws://127.0.0.1:{port}{path_and_query}")
        .into_client_request()
        .map_err(|error| format!("Invalid Server Preview WebSocket request: {error}"))?;
    for (name, value) in headers {
        if !is_websocket_handshake_header(name) && *name != header::COOKIE {
            if *name != header::AUTHORIZATION || !is_daemon_authorization(value, auth) {
                request.headers_mut().append(name, value.clone());
            }
        }
    }
    if let Some(cookie) = application_cookie_header(headers) {
        request.headers_mut().insert(header::COOKIE, cookie);
    }
    Ok(request)
}

fn is_websocket_handshake_header(name: &header::HeaderName) -> bool {
    matches!(
        name.as_str(),
        "host"
            | "connection"
            | "upgrade"
            | "sec-websocket-key"
            | "sec-websocket-version"
            | "sec-websocket-extensions"
    )
}

async fn bridge_websockets(
    client: WebSocket,
    upstream: tokio_tungstenite::WebSocketStream<
        tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
    >,
) {
    let (mut client_tx, mut client_rx) = client.split();
    let (mut upstream_tx, mut upstream_rx) = upstream.split();

    loop {
        tokio::select! {
            message = client_rx.next() => match message {
                Some(Ok(message)) => {
                    let closing = matches!(message, ClientMessage::Close(_));
                    if upstream_tx.send(client_to_upstream(message)).await.is_err() || closing {
                        break;
                    }
                }
                _ => break,
            },
            message = upstream_rx.next() => match message {
                Some(Ok(message)) => {
                    let Some(message) = upstream_to_client(message) else {
                        continue;
                    };
                    let closing = matches!(message, ClientMessage::Close(_));
                    if client_tx.send(message).await.is_err() || closing {
                        break;
                    }
                }
                _ => break,
            },
        }
    }
}

fn client_to_upstream(message: ClientMessage) -> UpstreamMessage {
    match message {
        ClientMessage::Text(text) => UpstreamMessage::Text(text.to_string().into()),
        ClientMessage::Binary(bytes) => UpstreamMessage::Binary(bytes),
        ClientMessage::Ping(bytes) => UpstreamMessage::Ping(bytes),
        ClientMessage::Pong(bytes) => UpstreamMessage::Pong(bytes),
        ClientMessage::Close(_) => UpstreamMessage::Close(None),
    }
}

fn upstream_to_client(message: UpstreamMessage) -> Option<ClientMessage> {
    match message {
        UpstreamMessage::Text(text) => Some(ClientMessage::Text(text.to_string().into())),
        UpstreamMessage::Binary(bytes) => Some(ClientMessage::Binary(bytes)),
        UpstreamMessage::Ping(bytes) => Some(ClientMessage::Ping(bytes)),
        UpstreamMessage::Pong(bytes) => Some(ClientMessage::Pong(bytes)),
        UpstreamMessage::Close(_) => Some(ClientMessage::Close(None)),
        UpstreamMessage::Frame(_) => None,
    }
}

async fn proxy_share_request(client: &reqwest::Client, req: Request, port: u16) -> Response {
    if !matches!(*req.method(), Method::GET | Method::HEAD) {
        return Response::builder()
            .status(StatusCode::METHOD_NOT_ALLOWED)
            .header(header::ALLOW, "GET, HEAD")
            .body(Body::from(
                "VibeAround shared Server Preview only supports GET and HEAD requests.",
            ))
            .expect("valid method rejection");
    }
    if req.headers().contains_key(header::UPGRADE) {
        return StatusCode::NOT_IMPLEMENTED.into_response();
    }

    let path_and_query = req
        .uri()
        .path_and_query()
        .map(|value| value.as_str())
        .unwrap_or("/");
    let method = req.method().clone();
    let request_headers = req.headers().clone();
    let url = format!("http://127.0.0.1:{port}{path_and_query}");
    let mut upstream_request = client.request(method, url);
    for name in forwarded_request_headers() {
        if let Some(value) = request_headers.get(name) {
            upstream_request = upstream_request.header(name, value);
        }
    }
    match upstream_request.send().await {
        Ok(upstream) => share_upstream_response(upstream, port).await,
        Err(error) => upstream_error(port, error),
    }
}

fn application_cookie_header(headers: &header::HeaderMap) -> Option<header::HeaderValue> {
    let cookies = headers
        .get_all(header::COOKIE)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|value| value.split(';'))
        .map(str::trim)
        .filter(|pair| {
            pair.split_once('=').is_none_or(|(name, _)| {
                !matches!(
                    name.trim(),
                    SERVER_ROUTING_COOKIE | crate::web_server::auth::OWNER_COOKIE
                )
            })
        })
        .collect::<Vec<_>>();
    if cookies.is_empty() {
        None
    } else {
        cookies.join("; ").parse().ok()
    }
}

fn is_daemon_authorization(value: &header::HeaderValue, auth: &AuthState) -> bool {
    value
        .to_str()
        .ok()
        .and_then(|value| {
            value
                .strip_prefix("Bearer ")
                .or_else(|| value.strip_prefix("bearer "))
        })
        .is_some_and(|token| token.trim() == auth.owner.as_str())
}

fn is_hop_by_hop_header(name: &header::HeaderName) -> bool {
    matches!(
        name.as_str(),
        "connection"
            | "keep-alive"
            | "proxy-authenticate"
            | "proxy-authorization"
            | "te"
            | "trailer"
            | "transfer-encoding"
            | "upgrade"
    )
}

fn transparent_upstream_response(upstream: reqwest::Response) -> Response {
    let status =
        StatusCode::from_u16(upstream.status().as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);
    let headers = upstream.headers().clone();
    let mut builder = Response::builder().status(status);
    for (name, value) in &headers {
        if !is_hop_by_hop_header(name) {
            builder = builder.header(name, value);
        }
    }
    builder
        .body(Body::from_stream(upstream.bytes_stream()))
        .expect("valid proxied response")
}

fn upstream_error(port: u16, error: reqwest::Error) -> Response {
    (
        StatusCode::BAD_GATEWAY,
        format!("Server Preview upstream 127.0.0.1:{port} error: {error}"),
    )
        .into_response()
}

fn server_routing_cookie(slug: &str, auth: &AuthState) -> String {
    format!(
        "{SERVER_ROUTING_COOKIE}={}; Path=/; Secure; HttpOnly; SameSite=Lax",
        signed_slug(slug, auth)
    )
}

fn share_server_routing_cookie(
    share_id: &str,
    grant: &str,
    entry: &common::previews::PreviewEntry,
) -> String {
    let max_age = remaining_millis(entry)
        .map(|milliseconds| (milliseconds / 1000).max(1))
        .unwrap_or(1);
    format!(
        "{SERVER_ROUTING_COOKIE}={SHARE_ROUTE_PREFIX}{share_id}:{grant}; Path=/; Max-Age={max_age}; Secure; HttpOnly; SameSite=Lax"
    )
}

fn signed_slug(slug: &str, auth: &AuthState) -> String {
    let signature = routing_signature(slug, auth);
    format!("{slug}.{}", hex_encode(&signature))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ServerRouteKind {
    Owner,
    Share,
}

struct VerifiedServerRoute {
    entry: common::previews::PreviewEntry,
    kind: ServerRouteKind,
}

fn verified_server_entry(cookie: &str, auth: &AuthState) -> Option<VerifiedServerRoute> {
    if let Some(route) = cookie.strip_prefix(SHARE_ROUTE_PREFIX) {
        let (share_id, grant) = route.split_once(':')?;
        let entry = common::previews::authorize_share_grant(share_id, grant)
            .filter(|entry| matches!(entry.target, PreviewTarget::Server { .. }))?;
        return Some(VerifiedServerRoute {
            entry,
            kind: ServerRouteKind::Share,
        });
    }
    let (slug, signature) = cookie.rsplit_once('.')?;
    let signature = hex_decode_32(signature)?;
    constant_time_eq(&signature, &routing_signature(slug, auth)).then_some(())?;
    let entry = common::previews::lookup_owner(slug)
        .filter(|entry| matches!(entry.target, PreviewTarget::Server { .. }))?;
    Some(VerifiedServerRoute {
        entry,
        kind: ServerRouteKind::Owner,
    })
}

fn routing_signature(slug: &str, auth: &AuthState) -> [u8; 32] {
    let mut digest = Sha256::new();
    digest.update(auth.owner.as_str().as_bytes());
    digest.update([0]);
    digest.update(slug.as_bytes());
    digest.update([0]);
    digest.update(auth.owner.as_str().as_bytes());
    digest.finalize().into()
}

fn constant_time_eq(left: &[u8; 32], right: &[u8; 32]) -> bool {
    left.iter()
        .zip(right)
        .fold(0_u8, |difference, (left, right)| {
            difference | (left ^ right)
        })
        == 0
}

fn invalid_cookie_response() -> Response {
    Response::builder()
        .status(StatusCode::FOUND)
        .header(header::LOCATION, "/va/")
        .header(header::SET_COOKIE, clear_server_routing_cookie())
        .body(Body::empty())
        .expect("valid invalid-cookie redirect")
}

fn forwarded_request_headers() -> &'static [header::HeaderName] {
    &[
        header::ACCEPT,
        header::ACCEPT_LANGUAGE,
        header::RANGE,
        header::IF_MATCH,
        header::IF_NONE_MATCH,
        header::IF_MODIFIED_SINCE,
        header::IF_UNMODIFIED_SINCE,
        header::USER_AGENT,
    ]
}

async fn share_upstream_response(upstream: reqwest::Response, port: u16) -> Response {
    let status =
        StatusCode::from_u16(upstream.status().as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);
    let headers = upstream.headers().clone();
    let mut builder = Response::builder().status(status);
    for name in forwarded_response_headers() {
        if let Some(value) = headers.get(name) {
            if *name == header::LOCATION {
                if let Some(location) = rewrite_loopback_location(value, port) {
                    builder = builder.header(name, location);
                }
            } else {
                builder = builder.header(name, value);
            }
        }
    }
    let body = Body::from_stream(upstream.bytes_stream());
    builder.body(body).expect("valid proxied response")
}

fn forwarded_response_headers() -> &'static [header::HeaderName] {
    &[
        header::CONTENT_TYPE,
        header::CONTENT_LENGTH,
        header::CONTENT_ENCODING,
        header::CONTENT_LANGUAGE,
        header::CONTENT_RANGE,
        header::ACCEPT_RANGES,
        header::ETAG,
        header::LAST_MODIFIED,
        header::LOCATION,
    ]
}

fn rewrite_loopback_location(
    value: &header::HeaderValue,
    port: u16,
) -> Option<header::HeaderValue> {
    let raw = value.to_str().ok()?;
    let Ok(url) = reqwest::Url::parse(raw) else {
        return Some(value.clone());
    };
    let points_to_same_server = url.port_or_known_default() == Some(port)
        && url.host_str().is_some_and(|host| {
            matches!(
                host.to_ascii_lowercase().as_str(),
                "localhost" | "127.0.0.1" | "::1"
            )
        });
    if !points_to_same_server {
        return Some(value.clone());
    }
    let mut target = url.path().to_string();
    if let Some(query) = url.query() {
        target.push('?');
        target.push_str(query);
    }
    if let Some(fragment) = url.fragment() {
        target.push('#');
        target.push_str(fragment);
    }
    target.parse().ok()
}

fn hex_encode(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut encoded = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        encoded.push(HEX[(byte >> 4) as usize] as char);
        encoded.push(HEX[(byte & 0x0f) as usize] as char);
    }
    encoded
}

fn hex_decode_32(value: &str) -> Option<[u8; 32]> {
    if value.len() != 64 {
        return None;
    }
    let mut decoded = [0_u8; 32];
    for (index, pair) in value.as_bytes().chunks_exact(2).enumerate() {
        decoded[index] = (hex_nibble(pair[0])? << 4) | hex_nibble(pair[1])?;
    }
    Some(decoded)
}

fn hex_nibble(value: u8) -> Option<u8> {
    match value {
        b'0'..=b'9' => Some(value - b'0'),
        b'a'..=b'f' => Some(value - b'a' + 10),
        b'A'..=b'F' => Some(value - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
#[path = "server_proxy_tests.rs"]
mod tests;
