//! Live preview routes.
//!
//! **Preview pages**:
//! - GET /preview/u/:slug            — owner React app
//! - GET /preview/u/:slug/bootstrap  — owner app data
//! - GET /preview/u/:slug/chat       — owner-only conversation WebSocket
//! - POST /preview/u/:slug/chat/uploads — owner-only chat attachment upload
//! - GET /preview/u/:slug/content    — owner iframe content
//! - GET /preview/s/:share_id        — share gate or authorized preview
//! - POST /preview/s/:share_id       — verify reusable access code
//!   The owner app switches one iframe between previews. Markdown content
//!   renders from the owner-only content route. Server content loads directly
//!   from `localhost:{port}` locally and through the page proxy remotely.
//!
//! Public shares use an opaque link ID plus a reusable six-digit
//! access code. Successful public verification receives a path-scoped browser
//! grant with the same TTL (`common::previews::SHARE_TTL_SECS`). Owner slugs
//! remain stable and require owner access through a loopback request or the
//! `va_owner` cookie. Shared Server iframe navigations and browser-declared
//! subresources revalidate that same grant at the root proxy; the proxy does
//! not forward `/va/*`, and the grant does not include owner chat or review.
//!
//! ## Module layout
//!
//! - [`assets`]        — versioned third-party browser assets
//! - [`bootstrap`]     — owner app target data
//! - [`iframe`]        — owner app response + iframe target dispatch
//! - [`markdown`]      — rendered markdown document page
//! - [`toolbar`]       — shared toolbar HTML/CSS + HTML helpers

mod access;
mod assets;
mod bootstrap;
mod chat;
mod iframe;
mod markdown;
mod server_proxy;
mod toolbar;

use axum::body::Body;
use axum::extract::rejection::FormRejection;
use axum::extract::{Form, Path, Request, State};
use axum::http::{header, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum::Json;
use common::previews::{PreviewTarget, ShareCodeError};
use futures_util::future::join_all;
use serde::Deserialize;

pub(super) use assets::{
    dompurify_script_handler, marked_script_handler, review_bridge_script_handler,
    review_bridge_script_href, theme_stylesheet_handler, DOMPURIFY_SCRIPT_ROUTE,
    MARKED_SCRIPT_ROUTE, REVIEW_BRIDGE_SCRIPT_ROUTE, THEME_STYLESHEET_ROUTE,
};
pub(super) use chat::owner_preview_chat_handler;
pub(in crate::web_server) use server_proxy::{server_proxy_fallback, ServerProxyState};

use access::{
    clear_share_cookie, extract_cookie, render_access_gate, share_cookie_name, share_grant_cookie,
    with_retry_after, AccessGateError,
};
use bootstrap::owner_preview_bootstrap;
use iframe::{render_owner_app, render_owner_content};
use markdown::render_md_page;
use toolbar::url_encode_query;

// ===========================================================================
// Owner Preview app + iframe content
// ===========================================================================

/// GET /preview/u/{slug} — owner Preview app. Requires `va_owner` cookie on
/// non-loopback hosts.
///
/// Serves the Web SPA after validating the selected Preview. Preview performs
/// its own loopback check because these routes are
/// outside the auth middleware. For non-loopback hosts, a missing or invalid
/// cookie redirects through the pairing gate and then back to the requested
/// preview.
pub async fn owner_preview_handler(
    State(state): State<crate::web_server::AppState>,
    Path(slug): Path<String>,
    req: Request,
) -> Response {
    owner_preview_response(Path(slug), req, state.dist_for_fallback).await
}

pub(super) async fn owner_preview_response(
    Path(slug): Path<String>,
    req: Request,
    web_dist: std::path::PathBuf,
) -> Response {
    let local = crate::web_server::auth::request_is_loopback(&req);
    let response = match common::previews::lookup_owner(&slug) {
        Some(_) if owner_access_allowed(&req) => {
            let previews = active_preview_snapshots().await;
            let mut response = match tokio::fs::read_to_string(web_dist.join("index.html")).await {
                Ok(html) => {
                    render_owner_app(html, &previews, local.then(|| preview_server_host(&req)))
                }
                Err(error) => (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    format!("Failed to load Preview app: {error}"),
                )
                    .into_response(),
            };
            if local {
                clear_local_preview_cookies(&mut response);
            }
            response
        }
        Some(_) => owner_pairing_redirect(&req, format!("/preview/u/{}", slug)),
        None if owner_access_allowed(&req) => preview_not_found().into_response(),
        None => owner_pairing_redirect(&req, format!("/preview/u/{}", slug)),
    };
    no_store(response)
}

/// GET /preview/u/{slug}/bootstrap — owner-only SPA data.
pub async fn owner_preview_bootstrap_handler(Path(slug): Path<String>, req: Request) -> Response {
    let response = if !owner_access_allowed(&req) {
        StatusCode::UNAUTHORIZED.into_response()
    } else {
        match common::previews::lookup_owner(&slug) {
            Some(_) => {
                let local = crate::web_server::auth::request_is_loopback(&req);
                Json(owner_preview_bootstrap(
                    &slug,
                    &active_preview_snapshots().await,
                    local.then(|| preview_server_host(&req)),
                ))
                .into_response()
            }
            None => preview_not_found().into_response(),
        }
    };
    no_store(response)
}

/// GET /preview/u/{slug}/content — owner-only iframe content.
///
/// Markdown renders directly. A remote Server preview enters through this
/// route once to establish its signed page-routing cookie.
pub async fn owner_preview_content_handler(Path(slug): Path<String>, req: Request) -> Response {
    let response = match common::previews::lookup_owner(&slug) {
        Some(entry) if owner_access_allowed(&req) => {
            if matches!(entry.target, PreviewTarget::Server { .. })
                && !crate::web_server::auth::request_is_loopback(&req)
            {
                server_proxy::owner_server_content_response(&slug, &req)
            } else {
                // Review is an owner Preview capability, not a reflection of
                // the process-local conversation binding. The chat socket can
                // recover or create that conversation independently.
                render_owner_content(entry)
                    .await
                    .unwrap_or_else(IntoResponse::into_response)
            }
        }
        Some(_) => owner_pairing_redirect_to(format!("/va/preview/u/{slug}")),
        None if owner_access_allowed(&req) => preview_not_found().into_response(),
        None => owner_pairing_redirect_to(format!("/va/preview/u/{slug}")),
    };
    no_store(response)
}

pub(super) async fn active_preview_snapshots() -> Vec<common::previews::PreviewSnapshot> {
    join_all(
        common::previews::list_snapshots()
            .into_iter()
            .map(|preview| async move {
                match preview.port {
                    None => Some(preview),
                    Some(port) if server_port_is_listening(port).await => Some(preview),
                    Some(_) => None,
                }
            }),
    )
    .await
    .into_iter()
    .flatten()
    .collect()
}

async fn server_port_is_listening(port: u16) -> bool {
    use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr};
    use std::time::Duration;

    let ipv4 = tokio::net::TcpStream::connect(SocketAddr::from((Ipv4Addr::LOCALHOST, port)));
    let ipv6 = tokio::net::TcpStream::connect(SocketAddr::from((Ipv6Addr::LOCALHOST, port)));
    tokio::pin!(ipv4, ipv6);

    tokio::time::timeout(Duration::from_millis(250), async {
        tokio::select! {
            result = &mut ipv4 => result.is_ok() || ipv6.await.is_ok(),
            result = &mut ipv6 => result.is_ok() || ipv4.await.is_ok(),
        }
    })
    .await
    .unwrap_or(false)
}

fn preview_server_host(req: &Request) -> &'static str {
    let host = req
        .headers()
        .get(header::HOST)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default()
        .to_ascii_lowercase();
    if host == "localhost" || host.starts_with("localhost:") {
        "localhost"
    } else if host == "::1" || host.starts_with("[::1]") {
        "[::1]"
    } else {
        "127.0.0.1"
    }
}

fn clear_local_preview_cookies(response: &mut Response) {
    for cookie in crate::web_server::auth::owner_cookie_headers(None)
        .into_iter()
        .chain(std::iter::once(
            "va_preview=; Path=/; Max-Age=0; HttpOnly; SameSite=Lax".to_string(),
        ))
        .chain(std::iter::once(server_proxy::clear_server_routing_cookie()))
    {
        response.headers_mut().append(
            header::SET_COOKIE,
            cookie.parse().expect("valid preview cookie"),
        );
    }
}

pub(super) fn owner_access_allowed(req: &Request) -> bool {
    owner_cookie_valid(req) || crate::web_server::auth::request_is_local_dashboard(req)
}

pub(super) async fn require_owner_preview_access(req: Request, next: Next) -> Response {
    if !owner_access_allowed(&req) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    next.run(req).await
}

/// GET /preview/s/{share_id} — public gate or authorized preview.
pub async fn share_preview_handler(Path(share_id): Path<String>, req: Request) -> Response {
    let response = match common::previews::lookup_share_link(&share_id) {
        Some(entry)
            if crate::web_server::auth::request_is_local_dashboard(&req)
                && matches!(&entry.target, PreviewTarget::File) =>
        {
            render_md_page(&entry)
                .await
                .unwrap_or_else(IntoResponse::into_response)
        }
        Some(entry) => match extract_cookie(&req, &share_cookie_name(&share_id)) {
            Some(grant) => match common::previews::authorize_share_grant(&share_id, &grant) {
                Some(authorized) => {
                    if matches!(&authorized.target, PreviewTarget::Server { .. }) {
                        server_proxy::share_server_content_response(&share_id, &grant, &authorized)
                    } else {
                        render_md_page(&authorized)
                            .await
                            .unwrap_or_else(IntoResponse::into_response)
                    }
                }
                None => {
                    let mut gate = render_access_gate(&entry, None);
                    gate.headers_mut().insert(
                        header::SET_COOKIE,
                        clear_share_cookie(&share_id)
                            .parse()
                            .expect("valid share cookie"),
                    );
                    gate
                }
            },
            None => render_access_gate(&entry, None),
        },
        None => preview_not_found().into_response(),
    };
    no_store(response)
}

#[derive(Deserialize)]
pub(super) struct ShareCodeForm {
    code: String,
}

/// POST /preview/s/{share_id} — verify the reusable access code.
pub async fn verify_share_code_handler(
    Path(share_id): Path<String>,
    form: Result<Form<ShareCodeForm>, FormRejection>,
) -> Response {
    let code = form
        .map(|Form(form)| form.code.trim().to_string())
        .unwrap_or_default();
    let response = match common::previews::verify_share_code(&share_id, &code) {
        Ok((entry, grant)) => Response::builder()
            .status(StatusCode::SEE_OTHER)
            .header(header::LOCATION, format!("/va/preview/s/{share_id}"))
            .header(
                header::SET_COOKIE,
                share_grant_cookie(&share_id, &grant, &entry),
            )
            .body(Body::empty())
            .expect("valid access-code redirect"),
        Err(ShareCodeError::Invalid) => common::previews::lookup_share_link(&share_id)
            .map(|entry| render_access_gate(&entry, Some(AccessGateError::Incorrect)))
            .unwrap_or_else(|| preview_not_found().into_response()),
        Err(ShareCodeError::RateLimited { retry_after_secs }) => {
            common::previews::lookup_share_link(&share_id)
                .map(|entry| {
                    with_retry_after(
                        render_access_gate(
                            &entry,
                            Some(AccessGateError::RateLimited(retry_after_secs)),
                        ),
                        retry_after_secs,
                    )
                })
                .unwrap_or_else(|| preview_not_found().into_response())
        }
        Err(ShareCodeError::NotFound) => preview_not_found().into_response(),
    };
    no_store(response)
}

fn no_store(mut response: Response) -> Response {
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        "no-store".parse().expect("valid cache-control header"),
    );
    response
}

fn owner_pairing_redirect(req: &Request, fallback_path: String) -> Response {
    // `req.uri().path_and_query()` sees the path AFTER the `/va` nest
    // prefix has been stripped by axum, so prepend `/va` to rebuild the
    // absolute URL the browser should land on after pairing.
    let inner = req
        .uri()
        .path_and_query()
        .map(|pq| pq.as_str().to_string())
        .unwrap_or(fallback_path);
    owner_pairing_redirect_to(format!("/va{inner}"))
}

fn owner_pairing_redirect_to(next: String) -> Response {
    let location = format!("/va/?next={}", url_encode_query(&next));
    Response::builder()
        .status(StatusCode::FOUND)
        .header("Location", location)
        .body(Body::empty())
        .unwrap()
}

fn preview_not_found() -> (StatusCode, String) {
    (
        StatusCode::NOT_FOUND,
        "Preview not found or expired.".to_string(),
    )
}

/// Check whether the request carries a valid `va_owner` cookie.
fn owner_cookie_valid(req: &Request) -> bool {
    let token = match extract_cookie(req, crate::web_server::auth::OWNER_COOKIE) {
        Some(t) => t,
        None => return false,
    };
    req.extensions()
        .get::<crate::web_server::auth::AuthState>()
        .is_some_and(|auth| auth.0.matches(&token))
}

#[cfg(test)]
#[path = "route_tests.rs"]
mod tests;
