//! Bearer-token auth middleware for protected routes.
//!
//! Accepts the token from either of:
//!   - `Authorization: Bearer <token>` header (preferred for fetch APIs)
//!   - `?token=<token>` query parameter (fallback for initial page load
//!     and for WebSocket upgrades, which cannot carry custom headers)
//!
//! On mismatch returns `401 Unauthorized` with an empty body for regular
//! routes. For the `/mcp` JSON-RPC endpoint we instead return HTTP 200 with
//! a JSON-RPC error envelope, because MCP clients (Claude Code, Codex, etc.)
//! try to parse the response body as JSON-RPC and surface "Failed to parse
//! JSON" on an empty body — that symptom is invisible to end users and
//! makes stale-token situations extremely confusing.
//!
//! The token is loaded once per daemon start (see `common::auth`) and held
//! as part of `AppState`, so the middleware is a pure function over the
//! incoming request.

use std::sync::Arc;

use axum::{
    body::Body,
    extract::connect_info::ConnectInfo,
    extract::State,
    http::{header, HeaderMap, Request, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
    Json,
};

use common::auth::AuthToken;
use std::net::SocketAddr;

/// Shared handle to the server's current auth token.
#[derive(Clone)]
pub struct AuthState(pub Arc<AuthToken>);

/// Extract a bearer token from the request — header first, then `?token=`.
fn extract_token<B>(req: &Request<B>) -> Option<String> {
    // 1. Authorization: Bearer <token>
    if let Some(value) = req.headers().get(header::AUTHORIZATION) {
        if let Ok(s) = value.to_str() {
            if let Some(rest) = s.strip_prefix("Bearer ") {
                return Some(rest.trim().to_string());
            }
            if let Some(rest) = s.strip_prefix("bearer ") {
                return Some(rest.trim().to_string());
            }
        }
    }
    // 2. ?token=<token>  (brittle but good enough — we only look for the
    //    exact key; real parsing happens via url::form_urlencoded)
    if let Some(query) = req.uri().query() {
        for pair in query.split('&') {
            if let Some(rest) = pair.strip_prefix("token=") {
                // URL-decode the value. `+` is a space in form encoding,
                // but a hex token never contains one — still, be safe.
                let decoded = url_decode(rest);
                return Some(decoded);
            }
        }
    }
    None
}

pub(crate) fn is_loopback_host(host: &str) -> bool {
    let host = host.trim().to_ascii_lowercase();
    if matches!(host.as_str(), "localhost" | "127.0.0.1" | "::1") {
        return true;
    }

    let without_port = host
        .strip_prefix('[')
        .and_then(|rest| rest.split_once(']').map(|(addr, _)| addr.to_string()))
        .or_else(|| host.rsplit_once(':').map(|(addr, _)| addr.to_string()))
        .unwrap_or(host);

    matches!(without_port.as_str(), "localhost" | "127.0.0.1" | "::1")
}

fn request_host_is_loopback<B>(req: &Request<B>) -> bool {
    req.headers()
        .get(header::HOST)
        .and_then(|value| value.to_str().ok())
        .is_some_and(is_loopback_host)
}

fn request_peer_is_loopback<B>(req: &Request<B>) -> bool {
    req.extensions()
        .get::<ConnectInfo<SocketAddr>>()
        .is_some_and(|ConnectInfo(addr)| addr.ip().is_loopback())
}

fn request_origin_is_local_dashboard<B>(req: &Request<B>) -> bool {
    req.headers()
        .get(header::ORIGIN)
        .and_then(|value| value.to_str().ok())
        .is_none_or(|origin| local_dashboard_origin_allowed(origin, None))
}

/// Allow local API bridge calls only from clients that actually connected via
/// loopback and targeted a loopback dashboard URL. Tunnel forwarders also
/// connect to the daemon over loopback, so the Host/Origin checks are needed
/// to keep `/local-api` off public tunnel URLs while preserving local CLI use.
pub async fn require_local_bridge(req: Request<Body>, next: Next) -> Response {
    if request_peer_is_loopback(&req)
        && request_host_is_loopback(&req)
        && request_origin_is_local_dashboard(&req)
    {
        return next.run(req).await;
    }

    StatusCode::FORBIDDEN.into_response()
}

/// axum middleware that rejects any request lacking a valid token.
pub async fn require_auth(
    State(state): State<AuthState>,
    req: Request<Body>,
    next: Next,
) -> Response {
    let is_mcp = req.uri().path() == "/mcp";
    let token = extract_token(&req);
    let authorized = match token.as_deref() {
        Some(candidate) => state.0.matches(candidate),
        None => false,
    };
    if authorized {
        return next.run(req).await;
    }
    if is_mcp {
        // MCP clients parse the response body as JSON-RPC. Return a
        // legible error envelope instead of an empty 401 body so the
        // coding agent surfaces "auth required" rather than
        // "Failed to parse JSON".
        let body = Json(serde_json::json!({
            "jsonrpc": "2.0",
            "id": serde_json::Value::Null,
            "error": {
                "code": -32001,
                "message": "Unauthorized — VibeAround MCP requires a token. \
                            Restart your coding agent so it reloads the MCP \
                            config written by the VibeAround daemon \
                            (includes ?token=… in the URL).",
            },
        }));
        return (StatusCode::OK, body).into_response();
    }
    StatusCode::UNAUTHORIZED.into_response()
}

pub(crate) fn headers_have_allowed_ws_origin(
    headers: &HeaderMap,
    port: u16,
    tunnel_urls: &[String],
) -> bool {
    headers
        .get(header::ORIGIN)
        .and_then(|value| value.to_str().ok())
        .is_none_or(|origin| dashboard_origin_allowed(origin, port, tunnel_urls))
}

pub(crate) fn dashboard_origin_allowed(origin: &str, port: u16, tunnel_urls: &[String]) -> bool {
    if local_dashboard_origin_allowed(origin, Some(port)) {
        return true;
    }

    let Some(origin) = parse_origin(origin) else {
        return false;
    };
    tunnel_urls.iter().any(|url| {
        parse_origin(url)
            .as_ref()
            .is_some_and(|tunnel_origin| tunnel_origin == &origin)
    })
}

fn local_dashboard_origin_allowed(origin: &str, port: Option<u16>) -> bool {
    let Some(origin) = parse_origin(origin) else {
        return false;
    };

    if matches!(
        (
            origin.scheme.as_str(),
            origin.host.as_str(),
            origin.port,
            port
        ),
        ("tauri", "localhost", None, _)
            | ("http", "tauri.localhost", Some(80), _)
            | ("http", "localhost", Some(5181), _)
    ) {
        return true;
    }

    match port {
        Some(port) => {
            origin.scheme == "http"
                && origin.port == Some(port)
                && matches!(origin.host.as_str(), "localhost" | "127.0.0.1" | "::1")
        }
        None => {
            origin.scheme == "http"
                && matches!(origin.host.as_str(), "localhost" | "127.0.0.1" | "::1")
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
struct OriginParts {
    scheme: String,
    host: String,
    port: Option<u16>,
}

fn parse_origin(value: &str) -> Option<OriginParts> {
    let parsed = reqwest::Url::parse(value.trim().trim_end_matches('/')).ok()?;
    let host = parsed
        .host_str()?
        .trim_matches(['[', ']'])
        .to_ascii_lowercase();
    Some(OriginParts {
        scheme: parsed.scheme().to_ascii_lowercase(),
        host,
        port: parsed.port_or_known_default(),
    })
}

/// Minimal percent-decoder for the `?token=` value. We only need to handle
/// `%HH` sequences; the hex token alphabet is URL-safe.
fn url_decode(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let (Some(h), Some(l)) = (from_hex(bytes[i + 1]), from_hex(bytes[i + 2])) {
                out.push((h << 4) | l);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8(out).unwrap_or_else(|_| input.to_string())
}

fn from_hex(c: u8) -> Option<u8> {
    match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(c - b'a' + 10),
        b'A'..=b'F' => Some(c - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;

    fn req_with_header(value: &str) -> Request<Body> {
        Request::builder()
            .uri("/api/sessions")
            .header(header::AUTHORIZATION, value)
            .body(Body::empty())
            .unwrap()
    }

    fn req_with_query(query: &str) -> Request<Body> {
        Request::builder()
            .uri(format!("/api/sessions?{query}"))
            .body(Body::empty())
            .unwrap()
    }

    #[test]
    fn extracts_bearer_header() {
        let r = req_with_header("Bearer abc123");
        assert_eq!(extract_token(&r), Some("abc123".into()));
    }

    #[test]
    fn extracts_lowercase_bearer_header() {
        let r = req_with_header("bearer xyz");
        assert_eq!(extract_token(&r), Some("xyz".into()));
    }

    #[test]
    fn ignores_non_bearer_auth_header() {
        let r = req_with_header("Basic dXNlcjpwYXNz");
        assert_eq!(extract_token(&r), None);
    }

    #[test]
    fn extracts_token_query_param() {
        let r = req_with_query("token=deadbeef");
        assert_eq!(extract_token(&r), Some("deadbeef".into()));
    }

    #[test]
    fn extracts_token_query_param_among_others() {
        let r = req_with_query("session_id=abc&token=deadbeef&foo=bar");
        assert_eq!(extract_token(&r), Some("deadbeef".into()));
    }

    #[test]
    fn no_token_returns_none() {
        let r = Request::builder()
            .uri("/api/sessions")
            .body(Body::empty())
            .unwrap();
        assert_eq!(extract_token(&r), None);
    }

    #[test]
    fn url_decode_handles_hex() {
        assert_eq!(url_decode("hello%20world"), "hello world");
        assert_eq!(url_decode("plain"), "plain");
        assert_eq!(url_decode("deadbeef"), "deadbeef");
    }

    #[test]
    fn recognizes_loopback_hosts() {
        assert!(is_loopback_host("localhost"));
        assert!(is_loopback_host("localhost:12358"));
        assert!(is_loopback_host("127.0.0.1"));
        assert!(is_loopback_host("127.0.0.1:12358"));
        assert!(is_loopback_host("::1"));
        assert!(is_loopback_host("[::1]:12358"));
        assert!(!is_loopback_host("example.com"));
        assert!(!is_loopback_host("example.com:12358"));
    }

    #[test]
    fn validates_static_dashboard_origins() {
        assert!(dashboard_origin_allowed(
            "http://127.0.0.1:12358",
            12358,
            &[]
        ));
        assert!(dashboard_origin_allowed(
            "http://localhost:12358",
            12358,
            &[]
        ));
        assert!(dashboard_origin_allowed("tauri://localhost", 12358, &[]));
        assert!(!dashboard_origin_allowed("http://evil.example", 12358, &[]));
    }

    #[test]
    fn validates_tunnel_origins_by_active_url() {
        assert!(dashboard_origin_allowed(
            "https://demo.loca.lt",
            12358,
            &["https://demo.loca.lt".to_string()]
        ));
        assert!(!dashboard_origin_allowed(
            "https://other.loca.lt",
            12358,
            &["https://demo.loca.lt".to_string()]
        ));
    }
}
