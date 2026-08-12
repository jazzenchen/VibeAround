//! Same-origin page and static-resource proxy for remote owner Server previews.
//!
//! The owner-only `/content` route selects one live Server preview by setting a
//! signed, daemon-lifetime routing cookie. Root requests then retain their
//! original path and query while this module forwards them to the selected
//! loopback dev server. This intentionally does not proxy APIs, workers,
//! WebSockets, or writes.

use axum::body::Body;
use axum::extract::{Extension, Request};
use axum::http::{header, Method, StatusCode};
use axum::response::{IntoResponse, Redirect, Response};
use sha2::{Digest, Sha256};

use common::previews::PreviewTarget;

use crate::web_server::auth::AuthState;

use super::access::extract_cookie;

const SERVER_ROUTING_COOKIE: &str = "va_preview_server";

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

pub(super) fn clear_server_routing_cookie() -> String {
    format!("{SERVER_ROUTING_COOKIE}=; Path=/; Max-Age=0; Secure; HttpOnly; SameSite=Lax")
}

/// Root fallback used outside `/va/`. Missing or invalid routing state returns
/// to the dashboard; accepted page/static requests retain their path/query.
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
    if !matches!(*req.method(), Method::GET | Method::HEAD) {
        return Response::builder()
            .status(StatusCode::METHOD_NOT_ALLOWED)
            .header(header::ALLOW, "GET, HEAD")
            .body(Body::from(
                "VibeAround Preview only proxies page and static-resource GET/HEAD requests.",
            ))
            .expect("valid method rejection");
    }

    let destination = req
        .headers()
        .get("sec-fetch-dest")
        .and_then(|value| value.to_str().ok())
        .unwrap_or("document");
    if destination == "document" {
        return Redirect::temporary("/va/").into_response();
    }
    if !matches!(
        destination,
        "iframe" | "script" | "style" | "image" | "font" | "audio" | "video" | "track" | "manifest"
    ) {
        return StatusCode::FORBIDDEN.into_response();
    }

    let Some(auth) = req.extensions().get::<AuthState>() else {
        return Redirect::temporary("/va/").into_response();
    };
    let Some(cookie) = extract_cookie(&req, SERVER_ROUTING_COOKIE) else {
        return Redirect::temporary("/va/").into_response();
    };
    let Some(entry) = verified_server_entry(&cookie, auth) else {
        return invalid_cookie_response();
    };
    let PreviewTarget::Server { port } = entry.target else {
        return invalid_cookie_response();
    };

    let path_and_query = req
        .uri()
        .path_and_query()
        .map(|value| value.as_str())
        .unwrap_or("/");
    let method = req.method().clone();
    let request_headers = req.headers().clone();
    let mut upstream = None;
    for host in ["127.0.0.1", "[::1]"] {
        let url = format!("http://{host}:{port}{path_and_query}");
        let mut upstream_request = client.request(method.clone(), url);
        for name in forwarded_request_headers() {
            if let Some(value) = request_headers.get(name) {
                upstream_request = upstream_request.header(name, value);
            }
        }
        match upstream_request.send().await {
            Ok(response) => {
                upstream = Some(response);
                break;
            }
            Err(error) if error.is_connect() => continue,
            Err(error) => {
                return (
                    StatusCode::BAD_GATEWAY,
                    format!("Server Preview upstream error: {error}"),
                )
                    .into_response();
            }
        }
    }

    let Some(upstream) = upstream else {
        return (
            StatusCode::BAD_GATEWAY,
            format!("Server Preview is not responding on localhost:{port}."),
        )
            .into_response();
    };
    upstream_response(upstream, port).await
}

fn server_routing_cookie(slug: &str, auth: &AuthState) -> String {
    format!(
        "{SERVER_ROUTING_COOKIE}={}; Path=/; Secure; HttpOnly; SameSite=Lax",
        signed_slug(slug, auth)
    )
}

fn signed_slug(slug: &str, auth: &AuthState) -> String {
    let signature = routing_signature(slug, auth);
    format!("{slug}.{}", hex_encode(&signature))
}

fn verified_server_entry(cookie: &str, auth: &AuthState) -> Option<common::previews::PreviewEntry> {
    let (slug, signature) = cookie.rsplit_once('.')?;
    let signature = hex_decode_32(signature)?;
    constant_time_eq(&signature, &routing_signature(slug, auth)).then_some(())?;
    common::previews::lookup_owner(slug)
        .filter(|entry| matches!(entry.target, PreviewTarget::Server { .. }))
}

fn routing_signature(slug: &str, auth: &AuthState) -> [u8; 32] {
    let mut digest = Sha256::new();
    digest.update(auth.0.as_str().as_bytes());
    digest.update([0]);
    digest.update(slug.as_bytes());
    digest.update([0]);
    digest.update(auth.0.as_str().as_bytes());
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

async fn upstream_response(upstream: reqwest::Response, port: u16) -> Response {
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
    let body = match upstream.bytes().await {
        Ok(bytes) => Body::from(bytes),
        Err(error) => {
            return (
                StatusCode::BAD_GATEWAY,
                format!("Failed to read Server Preview response: {error}"),
            )
                .into_response()
        }
    };
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
