//! Browser pairing API endpoints.
//!
//! - POST /va/api/pair/start  — generate a 6-digit code + session ID
//! - GET  /va/api/pair/status — poll for verification + receive auth token

use axum::body::Body;
use axum::{
    extract::Query,
    http::{header, StatusCode},
    response::{IntoResponse, Response},
    Json,
};

use super::auth::OWNER_COOKIE;

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
pub async fn status_handler(Query(q): Query<StatusQuery>) -> Response {
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
                    let [clear_legacy_cookie, owner_cookie] = owner_cookies(&token);
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

fn owner_cookies(token: &str) -> [String; 2] {
    // Releases before the root preview proxy scoped this cookie to `/va/`.
    // Clear that more-specific cookie so it cannot shadow the new root cookie.
    [
        format!(
            "{}=; Path=/va/; Max-Age=0; HttpOnly; SameSite=Lax",
            OWNER_COOKIE
        ),
        format!(
            "{}={}; Path=/; Secure; HttpOnly; SameSite=Lax",
            OWNER_COOKIE, token
        ),
    ]
}

#[cfg(test)]
mod tests {
    use super::owner_cookies;

    #[test]
    fn owner_cookie_migrates_from_va_path_to_root() {
        assert_eq!(
            owner_cookies("test-token"),
            [
                "va_owner=; Path=/va/; Max-Age=0; HttpOnly; SameSite=Lax".to_string(),
                "va_owner=test-token; Path=/; Secure; HttpOnly; SameSite=Lax".to_string(),
            ]
        );
    }
}
