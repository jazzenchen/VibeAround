//! Live preview routes.
//!
//! **Preview pages**:
//! - GET /preview/u/:slug            — owner preview
//! - GET /preview/s/:share_id        — Markdown share gate or document
//! - POST /preview/s/:share_id       — verify reusable access code
//!   Local Server owner previews set a `va_preview` cookie and render an
//!   iframe with `src="/"`; Markdown previews render directly.
//!
//! **Cookie proxy fallback** (root `/` handler):
//!   - Has cookie + page/asset GET or HEAD → proxy to dev server
//!   - Browser fetch/XHR, worker, and WebSocket destinations → rejected
//!   - Has cookie + direct navigation (`Sec-Fetch-Dest: document`) → dashboard
//!   - No cookie → redirect to `/va/`
//!
//! Markdown shares use a public opaque link ID plus a reusable six-digit
//! access code. Successful public verification receives a path-scoped browser
//! grant with the same TTL (`common::previews::SHARE_TTL_SECS`). Owner slugs
//! remain stable and require owner access through a loopback request or the
//! `va_owner` cookie. Server previews are local-only.
//!
//! ## Module layout
//!
//! - [`assets`]        — versioned third-party browser assets
//! - [`iframe`]        — owner dispatcher + server iframe wrapper
//! - [`markdown`]      — rendered markdown document page
//! - [`cookie_proxy`]  — root `/` fallback: dev-server page proxy
//! - [`toolbar`]       — shared toolbar HTML/CSS + HTML helpers

mod access;
mod assets;
mod cookie_proxy;
mod iframe;
mod markdown;
mod toolbar;

use axum::body::Body;
use axum::extract::rejection::FormRejection;
use axum::extract::{Form, Path, Request};
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use common::previews::{PreviewEntry, PreviewTarget, ShareCodeError};
use serde::Deserialize;

pub(super) use assets::{marked_script_handler, MARKED_SCRIPT_ROUTE};
pub use cookie_proxy::cookie_proxy_fallback;

use access::{
    clear_share_cookie, render_access_gate, share_cookie_name, share_grant_cookie,
    with_retry_after, AccessGateError,
};
use cookie_proxy::{extract_cookie, owner_routing_cookie};
use iframe::render_owner_preview;
use markdown::render_md_page;
use toolbar::url_encode_query;

// ===========================================================================
// Preview iframe — sets cookie, renders iframe with src="/"
// ===========================================================================

/// GET /preview/u/{slug} — owner preview. Requires `va_owner` cookie on
/// non-loopback hosts.
///
/// Dispatches by session target: Server → iframe + cookie proxy,
/// File → rendered markdown. Preview performs its own loopback check because
/// these routes are outside the auth middleware. For non-loopback hosts, a
/// missing or invalid cookie redirects through the pairing gate and then back
/// to the requested preview.
pub async fn owner_preview_handler(Path(slug): Path<String>, req: Request) -> Response {
    let response = match common::previews::lookup_owner(&slug) {
        Some(entry) if !preview_target_available(&req, &entry) => {
            server_preview_local_only().into_response()
        }
        Some(entry) if owner_access_allowed(&req) => {
            render_owner_preview(entry, &owner_routing_cookie(&slug))
                .await
                .unwrap_or_else(IntoResponse::into_response)
        }
        Some(_) => owner_pairing_redirect(&req, format!("/preview/u/{}", slug)),
        None if owner_access_allowed(&req) => preview_not_found().into_response(),
        None => owner_pairing_redirect(&req, format!("/preview/u/{}", slug)),
    };
    no_store(response)
}

pub(super) fn owner_access_allowed(req: &Request) -> bool {
    owner_cookie_valid(req) || crate::web_server::auth::request_is_local_dashboard(req)
}

/// GET /preview/s/{share_id} — public Markdown gate or authorized document.
pub async fn share_preview_handler(Path(share_id): Path<String>, req: Request) -> Response {
    let response = match common::previews::lookup_share_link(&share_id) {
        Some(entry) if crate::web_server::auth::request_is_local_dashboard(&req) => {
            render_md_page(&entry)
                .await
                .unwrap_or_else(IntoResponse::into_response)
        }
        Some(entry) => match extract_cookie(&req, &share_cookie_name(&share_id)) {
            Some(grant) => match common::previews::authorize_share_grant(&share_id, &grant) {
                Some(authorized) => render_md_page(&authorized)
                    .await
                    .unwrap_or_else(IntoResponse::into_response),
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

/// Whether this target may be served in the current request context.
/// Markdown can be remote; live Server content requires a real loopback
/// connection and loopback Host header.
pub(super) fn preview_target_available(req: &Request, entry: &PreviewEntry) -> bool {
    !matches!(entry.target, PreviewTarget::Server { .. })
        || crate::web_server::auth::request_is_loopback(req)
}

pub(super) fn server_preview_local_only() -> (StatusCode, &'static str) {
    (
        StatusCode::FORBIDDEN,
        "Live server previews are only available on localhost.",
    )
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
    let next = format!("/va{}", inner);
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
    common::auth::read_token_file()
        .map(|f| f.token == token)
        .unwrap_or(false)
}

#[cfg(test)]
#[path = "route_tests.rs"]
mod tests;
