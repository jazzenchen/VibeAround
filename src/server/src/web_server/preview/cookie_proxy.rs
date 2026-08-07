//! Cookie-based page proxy: the root `/` fallback handler.
//!
//! Once a preview iframe has set the `va_preview` cookie with an explicit
//! owner or share capability, every sub-resource request the iframe makes
//! lands at `/` on the dashboard server. This handler validates that
//! capability, proxies to the dev server on `localhost:{port}` (trying IPv4
//! then IPv6 loopback), and forwards most response headers except the
//! framing-related ones that would break the iframe.
//!
//! This is intentionally not a general reverse proxy: only GET/HEAD page and
//! static-resource requests from the preview iframe are accepted. Browser
//! fetch/XHR, streaming, worker, and WebSocket destinations are rejected.
//! `Sec-Fetch-Dest` is a browser product boundary, not an API security check.

use axum::body::Body;
use axum::extract::{Request, State};
use axum::http::{header, Method, StatusCode};
use axum::response::{IntoResponse, Redirect, Response};

use common::previews::{PreviewEntry, PreviewTarget};

use crate::web_server::AppState;

use super::iframe::server_not_running_page;

/// Cookie name used to route root-level requests to the dev server.
pub(super) const PREVIEW_COOKIE: &str = "va_preview";

const OWNER_PREVIEW_PREFIX: &str = "owner:";
const SHARE_PREVIEW_PREFIX: &str = "share:";

pub(super) fn owner_routing_cookie(slug: &str) -> String {
    format!("{OWNER_PREVIEW_PREFIX}{slug}")
}

pub(super) fn share_routing_cookie(key: &str) -> String {
    format!("{SHARE_PREVIEW_PREFIX}{key}")
}

/// Fallback handler for root `/` — the cookie-based dev-server proxy.
///
/// Security rules:
/// - `/va/*` paths → serve dashboard SPA (never proxy)
/// - GET/HEAD page or static resource from iframe → proxy to dev server
/// - Browser fetch/XHR, worker, WebSocket, and write requests → reject
/// - Top-level direct navigation → redirect to /va/
/// - No cookie → redirect to `/va/`
pub async fn cookie_proxy_fallback(State(state): State<AppState>, req: Request) -> Response {
    proxy_request(&state.preview_client, &state.dist_for_fallback, req).await
}

async fn proxy_request(
    preview_client: &reqwest::Client,
    dist_for_fallback: &std::path::Path,
    req: Request,
) -> Response {
    let mut response = proxy_request_inner(preview_client, dist_for_fallback, req).await;
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        "no-store".parse().expect("valid cache-control header"),
    );
    response
}

async fn proxy_request_inner(
    preview_client: &reqwest::Client,
    dist_for_fallback: &std::path::Path,
    req: Request,
) -> Response {
    // Never proxy /va/ paths — they belong to the dashboard.
    let path = req.uri().path();
    if path == "/va" || path.starts_with("/va/") {
        return crate::web_server::spa_fallback(dist_for_fallback.to_path_buf()).await;
    }

    if !matches!(*req.method(), Method::GET | Method::HEAD) {
        return Response::builder()
            .status(StatusCode::METHOD_NOT_ALLOWED)
            .header(header::ALLOW, "GET, HEAD")
            .body(Body::from(
                "VibeAround Preview only supports page and static-resource GET/HEAD requests.",
            ))
            .unwrap();
    }

    // Check Sec-Fetch-Dest: only allow iframe and sub-resource contexts.
    // Direct top-level navigation (or missing header, e.g. stripped by tunnel
    // proxy) is blocked. This is an allowlist — unknown values are rejected.
    let sec_fetch_dest = req
        .headers()
        .get("sec-fetch-dest")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("document"); // missing header → treat as direct navigation

    let is_page_resource = matches!(
        sec_fetch_dest,
        "iframe" | "script" | "style" | "image" | "font" | "audio" | "video" | "track" | "manifest"
    );

    // Extract the typed preview capability from the cookie.
    let preview_cookie = match extract_cookie(&req, PREVIEW_COOKIE) {
        Some(s) => s,
        None => return Redirect::temporary("/va/").into_response(),
    };

    // Block direct navigation — preview content must only be accessed
    // inside an iframe wrapper. Redirect to dashboard instead of error page.
    if sec_fetch_dest == "document" {
        return Redirect::temporary("/va/").into_response();
    }
    if !is_page_resource {
        return Response::builder()
            .status(StatusCode::FORBIDDEN)
            .body(Body::from(
                "VibeAround Preview only handles browser page and static-resource requests.",
            ))
            .unwrap();
    }

    let entry = match lookup_preview_cookie(&req, &preview_cookie) {
        Some(e) => e,
        None => {
            // Cookie is invalid, unauthorized, or expired — clear it and redirect.
            let clear_cookie = format!(
                "{}=; Path=/; Max-Age=0; HttpOnly; SameSite=Lax",
                PREVIEW_COOKIE
            );
            return Response::builder()
                .status(StatusCode::FOUND)
                .header("Location", "/va/")
                .header("Set-Cookie", clear_cookie)
                .body(Body::empty())
                .unwrap();
        }
    };

    let port = match &entry.target {
        PreviewTarget::Server { port } => *port,
        PreviewTarget::File => return Redirect::temporary("/va/").into_response(),
    };

    // Proxy the request to the dev server.
    let sub_path = req.uri().path().trim_start_matches('/');
    let query = req
        .uri()
        .query()
        .map(|q| format!("?{q}"))
        .unwrap_or_default();

    // Try IPv4 first, then IPv6 loopback.
    let urls = [
        format!("http://127.0.0.1:{}/{}{}", port, sub_path, query),
        format!("http://[::1]:{}/{}{}", port, sub_path, query),
    ];

    let method = req.method().clone();
    let mut upstream_resp = None;

    for url in &urls {
        let upstream_req = preview_client.request(method.clone(), url);
        match upstream_req.send().await {
            Ok(resp) => {
                upstream_resp = Some(resp);
                break;
            }
            Err(e) if e.is_connect() => continue,
            Err(e) => {
                return (StatusCode::BAD_GATEWAY, format!("Upstream error: {e}")).into_response();
            }
        }
    }

    let upstream = match upstream_resp {
        Some(r) => r,
        None => return server_not_running_page(port),
    };

    let status =
        StatusCode::from_u16(upstream.status().as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);

    let mut builder = Response::builder().status(status);

    // Forward safe response headers, strip framing-related ones.
    for (key, val) in upstream.headers() {
        let name = key.as_str().to_lowercase();
        match name.as_str() {
            // Strip headers that would break iframe embedding or leak info.
            "x-frame-options"
            | "content-security-policy"
            | "strict-transport-security"
            | "set-cookie"
            | "clear-site-data"
            | "service-worker-allowed"
            | "cache-control" => {}
            // Forward everything else.
            _ => {
                builder = builder.header(key, val);
            }
        }
    }

    let body = match upstream.bytes().await {
        Ok(b) => b,
        Err(e) => {
            return (
                StatusCode::BAD_GATEWAY,
                format!("Failed to read upstream body: {e}"),
            )
                .into_response();
        }
    };

    builder.body(Body::from(body)).unwrap()
}

/// Resolve the explicitly typed preview cookie without crossing capability
/// boundaries. Owner cookies still require owner access on every proxied
/// request because the routing cookie itself is browser-controlled.
fn lookup_preview_cookie(req: &Request, cookie: &str) -> Option<PreviewEntry> {
    if let Some(slug) = cookie.strip_prefix(OWNER_PREVIEW_PREFIX) {
        if super::owner_access_allowed(req) {
            common::previews::lookup_owner(slug)
        } else {
            None
        }
    } else if let Some(key) = cookie.strip_prefix(SHARE_PREVIEW_PREFIX) {
        common::previews::lookup_share(key)
    } else {
        None
    }
}

/// Extract a named cookie value from the request.
pub(super) fn extract_cookie(req: &Request, name: &str) -> Option<String> {
    req.headers()
        .get_all("cookie")
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|s| s.split(';'))
        .map(|s| s.trim())
        .find_map(|pair| {
            let (k, v) = pair.split_once('=')?;
            if k.trim() == name {
                Some(v.trim().to_string())
            } else {
                None
            }
        })
}

#[cfg(test)]
#[path = "cookie_proxy_tests.rs"]
mod tests;
