//! Browser pairing API endpoints.
//!
//! - POST /va/api/pair/start  — generate a 6-digit code + session ID
//! - GET  /va/api/pair/status — poll for verification + receive auth token

use axum::body::Body;
use axum::{
    extract::{Query, Request},
    http::{header, StatusCode},
    response::{IntoResponse, Response},
    Json,
};

use super::auth::owner_cookie_headers;

/// POST /va/api/pair/start — generate a pairing code.
///
/// Returns `{ "code": "847291", "sid": "uuid" }`.
/// The code expires in 1 minute.
pub async fn start_handler() -> Json<serde_json::Value> {
    let (sid, code) = common::auth::pair::generate();
    Json(serde_json::json!({
        "code": code,
        "sid": sid,
    }))
}

#[derive(serde::Deserialize)]
pub struct StatusQuery {
    sid: String,
}

/// GET /va/api/pair/status?sid={sid} — poll for pairing status.
///
/// Returns:
/// - `{ "status": "pending" }` — waiting for `/pair` command
/// - `{ "status": "expired" }` — code has expired, frontend should refresh
/// - `{ "status": "verified" }` — paired! Also sets `va_owner` cookie with auth token
pub async fn status_handler(Query(q): Query<StatusQuery>, req: Request) -> Response {
    let mut response = match common::auth::pair::check_status(&q.sid) {
        None => {
            // Unknown or expired session.
            Json(serde_json::json!({ "status": "expired" })).into_response()
        }
        Some(false) => {
            // Still pending.
            Json(serde_json::json!({ "status": "pending" })).into_response()
        }
        Some(true) => {
            // Verified! Consume the session and set the owner cookie.
            match common::auth::pair::consume_verified(&q.sid) {
                Some(token) => {
                    let [clear_legacy_cookie, owner_cookie] = owner_cookie_headers(
                        (!super::auth::request_is_loopback(&req)).then_some(token.as_str()),
                    );
                    // Return the token in the body so the SPA can store it in
                    // sessionStorage (existing auth mechanism for API calls).
                    Response::builder()
                        .status(StatusCode::OK)
                        .header("Content-Type", "application/json")
                        .header(header::SET_COOKIE, clear_legacy_cookie)
                        .header(header::SET_COOKIE, owner_cookie)
                        .body(Body::from(
                            serde_json::json!({
                                "status": "verified",
                                "token": token,
                            })
                            .to_string(),
                        ))
                        .unwrap()
                }
                None => {
                    // Race: already consumed or token file missing.
                    Json(serde_json::json!({ "status": "expired" })).into_response()
                }
            }
        }
    };
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        "no-store".parse().expect("valid cache-control header"),
    );
    response
}

#[cfg(test)]
mod tests {
    use crate::web_server::auth::owner_cookie_headers;

    #[test]
    fn owner_cookie_is_scoped_to_vibearound_routes() {
        assert_eq!(
            owner_cookie_headers(Some("test-token")),
            [
                "va_owner=; Path=/; Max-Age=0; Secure; HttpOnly; SameSite=Lax".to_string(),
                "va_owner=test-token; Path=/va/; Secure; HttpOnly; SameSite=Lax".to_string(),
            ]
        );
    }

    #[test]
    fn local_pairing_clears_owner_credentials() {
        assert_eq!(
            owner_cookie_headers(None),
            [
                "va_owner=; Path=/; Max-Age=0; Secure; HttpOnly; SameSite=Lax".to_string(),
                "va_owner=; Path=/va/; Max-Age=0; Secure; HttpOnly; SameSite=Lax".to_string(),
            ]
        );
    }
}
