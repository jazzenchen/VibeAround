//! Bearer-token auth middleware for protected routes.
//!
//! Accepts the token from either of:
//!   - `Authorization: Bearer <token>` header (preferred for fetch APIs)
//!   - `?token=<token>` query parameter (fallback for initial page load
//!     and for WebSocket upgrades, which cannot carry custom headers)
//!
//! Unauthorized MCP requests return a JSON-RPC error envelope with HTTP 200.

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

/// Cookie set by the pairing flow for browser owner access.
pub(crate) const OWNER_COOKIE: &str = "va_owner";

/// Owner browser cookie headers. Pairing clears the legacy root-scoped cookie
/// before setting the current `/va/` cookie. Local Preview shells pass `None`
/// because loopback access does not need an owner credential and a cookie must
/// not be sent to a dev server on another port of the same host.
pub(crate) fn owner_cookie_headers(token: Option<&str>) -> [String; 2] {
    let legacy = format!(
        "{}=; Path=/; Max-Age=0; Secure; HttpOnly; SameSite=Lax",
        OWNER_COOKIE
    );
    let scoped = match token {
        Some(token) => format!(
            "{}={}; Path=/va/; Secure; HttpOnly; SameSite=Lax",
            OWNER_COOKIE, token
        ),
        None => format!(
            "{}=; Path=/va/; Max-Age=0; Secure; HttpOnly; SameSite=Lax",
            OWNER_COOKIE
        ),
    };
    [legacy, scoped]
}

/// Shared handles to the server's owner and MCP-only tokens.
#[derive(Clone)]
pub struct AuthState {
    pub owner: Arc<AuthToken>,
    pub mcp: Arc<AuthToken>,
}

impl AuthState {
    pub fn new(owner: Arc<AuthToken>, mcp: Arc<AuthToken>) -> Self {
        Self { owner, mcp }
    }
}

pub(crate) fn extract_bearer_token(headers: &HeaderMap) -> Option<String> {
    if let Some(value) = headers.get(header::AUTHORIZATION) {
        if let Ok(s) = value.to_str() {
            if let Some(rest) = s.strip_prefix("Bearer ") {
                return Some(rest.trim().to_string());
            }
            if let Some(rest) = s.strip_prefix("bearer ") {
                return Some(rest.trim().to_string());
            }
        }
    }
    None
}

/// Extract an auth token from the request — bearer header first, then `?token=`.
fn extract_token<B>(req: &Request<B>) -> Option<String> {
    if let Some(token) = extract_bearer_token(req.headers()) {
        return Some(token);
    }

    // Query token fallback for clients that cannot send custom headers.
    if let Some(query) = req.uri().query() {
        for pair in query.split('&') {
            if let Some(rest) = pair.strip_prefix("token=") {
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

    if let Some(rest) = host.strip_prefix('[') {
        let Some((address, suffix)) = rest.split_once(']') else {
            return false;
        };
        return address == "::1" && valid_optional_port(suffix);
    }

    let Some((name, port)) = host.rsplit_once(':') else {
        return false;
    };
    !name.contains(':') && matches!(name, "localhost" | "127.0.0.1") && valid_port(port)
}

fn valid_optional_port(suffix: &str) -> bool {
    suffix.is_empty() || suffix.strip_prefix(':').is_some_and(valid_port)
}

fn valid_port(port: &str) -> bool {
    !port.is_empty() && port.parse::<u16>().is_ok()
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
    let Some(origin) = req
        .headers()
        .get(header::ORIGIN)
        .and_then(|value| value.to_str().ok())
    else {
        return true;
    };
    request_host_port(req).is_some_and(|port| local_request_origin_allowed(origin, port))
}

fn request_host_port<B>(req: &Request<B>) -> Option<u16> {
    let host = req.headers().get(header::HOST)?.to_str().ok()?.trim();
    let authority = if host == "::1" { "[::1]" } else { host };
    reqwest::Url::parse(&format!("http://{authority}"))
        .ok()?
        .port_or_known_default()
}

/// Local browser access may bypass Preview authentication only when the
/// connection and Host are loopback and any browser Origin is also local.
/// Top-level navigations normally omit Origin and remain allowed.
pub(crate) fn request_is_local_dashboard<B>(req: &Request<B>) -> bool {
    request_is_loopback(req) && request_origin_is_local_dashboard(req)
}

/// Headers a reverse proxy stamps onto every request it forwards.
///
/// Tailscale Funnel routes on the TLS SNI and forwards the caller's Host
/// unchanged, so a public caller can send `Host: localhost:<port>` and satisfy
/// both loopback checks below — the peer is the local forwarder either way.
/// The forwarder overwrites these headers with `Set`, so a caller cannot
/// suppress them, which makes their presence a reliable "not local" signal.
const PROXY_FORWARDED_HEADERS: [&str; 4] = [
    "tailscale-funnel-request",
    "x-forwarded-for",
    "x-forwarded-host",
    "forwarded",
];

fn request_was_forwarded<B>(req: &Request<B>) -> bool {
    PROXY_FORWARDED_HEADERS
        .iter()
        .any(|name| req.headers().contains_key(*name))
}

/// Preview's local bypass requires a loopback peer, a loopback Host, and no
/// forwarder headers. Tunnel forwarders also connect over loopback, so peer
/// alone is insufficient, and they can carry a spoofed loopback Host, so the
/// headers they stamp on the way through are the third check.
pub(crate) fn request_is_loopback<B>(req: &Request<B>) -> bool {
    request_peer_is_loopback(req) && request_host_is_loopback(req) && !request_was_forwarded(req)
}

fn request_is_local_bridge<B>(req: &Request<B>) -> bool {
    request_is_local_dashboard(req)
}

fn local_bridge_access_allowed<B>(req: &Request<B>, auth: &AuthToken) -> bool {
    request_is_local_bridge(req)
        || (request_is_loopback(req)
            && extract_token(req).is_some_and(|candidate| auth.matches(&candidate)))
}

/// Allow local API bridge calls only from clients that actually connected via
/// loopback and targeted a loopback dashboard URL. Tunnel forwarders also
/// connect to the daemon over loopback, so the Host/Origin checks are needed
/// to keep `/local-api` off public tunnel URLs while preserving local CLI use.
/// Cross-origin Desktop development requests must additionally carry the
/// daemon owner token; the fixed Vite port is not itself an authority.
pub async fn require_local_bridge(
    State(state): State<AuthState>,
    req: Request<Body>,
    next: Next,
) -> Response {
    if local_bridge_access_allowed(&req, &state.owner) {
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
    if request_is_authorized(&state, &req) {
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

fn request_is_authorized<B>(state: &AuthState, req: &Request<B>) -> bool {
    let expected = if req.uri().path() == "/mcp" {
        &state.mcp
    } else {
        &state.owner
    };
    extract_token(req).is_some_and(|candidate| expected.matches(&candidate))
}

pub(crate) fn headers_have_allowed_dashboard_origin(
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
    if local_dashboard_origin_allowed(origin, port) {
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

fn local_request_origin_allowed(origin: &str, port: u16) -> bool {
    let Some(origin) = parse_origin(origin) else {
        return false;
    };

    if matches!(
        (origin.scheme.as_str(), origin.host.as_str(), origin.port),
        ("tauri", "localhost", None) | ("http", "tauri.localhost", Some(80))
    ) {
        return true;
    }

    origin.scheme == "http"
        && origin.port == Some(port)
        && matches!(origin.host.as_str(), "localhost" | "127.0.0.1" | "::1")
}

fn local_dashboard_origin_allowed(origin: &str, port: u16) -> bool {
    local_request_origin_allowed(origin, port)
        || parse_origin(origin).is_some_and(|origin| {
            matches!(
                (origin.scheme.as_str(), origin.host.as_str(), origin.port),
                ("http", "localhost", Some(5181))
            )
        })
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

/// Percent-decode `%HH` sequences in query-token values.
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
#[path = "auth_tests.rs"]
mod tests;
