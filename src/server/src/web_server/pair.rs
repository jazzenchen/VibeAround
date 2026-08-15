//! Browser pairing API endpoints.
//!
//! - POST /va/api/pair/start    — generate a 6-digit code + session ID
//! - GET  /va/api/pair/status   — poll for verification + receive auth token
//! - POST /va/api/pair/complete — finish pairing with a valid bearer token

use axum::body::Body;
use axum::{
    extract::{Query, Request, State},
    http::{header, StatusCode},
    response::{IntoResponse, Response},
    Json,
};

use super::auth::{extract_bearer_token, owner_cookie_headers, AuthState};
use super::AppState;

/// POST /va/api/pair/start — generate a pairing code.
///
/// Returns `{ "code": "847291", "sid": "uuid" }`.
/// The code expires in 1 minute.
pub async fn start_handler(State(state): State<AppState>, req: Request) -> Response {
    if !pairing_origin_allowed(&state, &req) {
        return StatusCode::FORBIDDEN.into_response();
    }
    let mut response = match common::auth::pair::generate() {
        Ok((sid, code)) => Json(serde_json::json!({
            "code": code,
            "sid": sid,
        }))
        .into_response(),
        Err(common::auth::pair::GenerateError::TooManyPending) => {
            let mut response = (
                StatusCode::TOO_MANY_REQUESTS,
                Json(serde_json::json!({
                    "error": "Too many pending pairing requests. Try again shortly."
                })),
            )
                .into_response();
            response.headers_mut().insert(
                header::RETRY_AFTER,
                common::auth::pair::CODE_TTL_SECS
                    .to_string()
                    .parse()
                    .expect("valid retry-after"),
            );
            response
        }
    };
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        "no-store".parse().expect("valid cache-control header"),
    );
    response
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
pub async fn status_handler(
    State(state): State<AppState>,
    Query(q): Query<StatusQuery>,
    req: Request,
) -> Response {
    if !pairing_origin_allowed(&state, &req) {
        return StatusCode::FORBIDDEN.into_response();
    }
    status_response(q, req)
}

fn status_response(q: StatusQuery, req: Request) -> Response {
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
            let Some(auth) = req.extensions().get::<AuthState>() else {
                return StatusCode::INTERNAL_SERVER_ERROR.into_response();
            };
            let token = auth.0.as_str();
            let [clear_legacy_cookie, owner_cookie] = owner_cookie_headers_for_request(&req, token);
            // Return the daemon's in-memory token so the SPA can store it in
            // sessionStorage. The verified sid remains idempotent until expiry.
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
    };
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        "no-store".parse().expect("valid cache-control header"),
    );
    response
}

fn pairing_origin_allowed(state: &AppState, req: &Request) -> bool {
    let tunnel_urls = state.tunnels.public_urls();
    super::auth::headers_have_allowed_dashboard_origin(req.headers(), state.port, &tunnel_urls)
}

/// POST /va/api/pair/complete — finish the Desktop-token fallback flow.
///
/// The protected router validates the bearer token before this handler runs.
/// Requiring the credential in the header keeps query-string tokens from
/// minting an owner cookie.
pub async fn complete_handler(req: Request) -> Response {
    let Some(token) = extract_bearer_token(req.headers()) else {
        return Response::builder()
            .status(StatusCode::UNAUTHORIZED)
            .header(header::CACHE_CONTROL, "no-store")
            .body(Body::empty())
            .expect("valid unauthorized response");
    };
    let [clear_legacy_cookie, owner_cookie] = owner_cookie_headers_for_request(&req, &token);
    Response::builder()
        .status(StatusCode::NO_CONTENT)
        .header(header::CACHE_CONTROL, "no-store")
        .header(header::SET_COOKIE, clear_legacy_cookie)
        .header(header::SET_COOKIE, owner_cookie)
        .body(Body::empty())
        .expect("valid pairing response")
}

fn owner_cookie_headers_for_request(req: &Request, token: &str) -> [String; 2] {
    owner_cookie_headers((!super::auth::request_is_loopback(req)).then_some(token))
}

#[cfg(test)]
mod tests {
    use std::{net::SocketAddr, sync::Arc};

    use axum::{
        body::{to_bytes, Body},
        http::{header, Request, StatusCode},
        middleware,
        routing::post,
        Router,
    };
    use common::auth::AuthToken;

    use crate::web_server::auth::{owner_cookie_headers, require_auth, AuthState};

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

    #[tokio::test]
    async fn verified_pairing_status_is_idempotent_and_uses_daemon_token() {
        let auth = Arc::new(AuthToken::generate());
        let expected_token = auth.as_str().to_string();
        let (sid, code) = common::auth::pair::generate().unwrap();
        assert!(common::auth::pair::validate(&code));

        for _ in 0..2 {
            let mut request = Request::builder()
                .header(header::HOST, "preview.example")
                .body(Body::empty())
                .unwrap();
            request
                .extensions_mut()
                .insert(AuthState(Arc::clone(&auth)));
            let response = super::status_response(super::StatusQuery { sid: sid.clone() }, request);
            assert_eq!(response.status(), StatusCode::OK);
            let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
            let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
            assert_eq!(json["status"], "verified");
            assert_eq!(json["token"], expected_token);
        }
    }

    #[test]
    fn browser_pairing_accepts_only_dashboard_origins() {
        let allowed = |origin: Option<&str>, tunnel_urls: &[String]| {
            let mut headers = axum::http::HeaderMap::new();
            if let Some(origin) = origin {
                headers.insert(header::ORIGIN, origin.parse().unwrap());
            }
            crate::web_server::auth::headers_have_allowed_dashboard_origin(
                &headers,
                12358,
                tunnel_urls,
            )
        };

        assert!(allowed(Some("http://127.0.0.1:12358"), &[]));
        assert!(allowed(
            Some("https://preview.example"),
            &["https://preview.example".into()]
        ));
        assert!(!allowed(Some("https://evil.example"), &[]));
        assert!(allowed(None, &[]), "native clients do not send Origin");
    }

    #[tokio::test]
    async fn desktop_token_completion_sets_only_valid_owner_credentials() {
        let auth = Arc::new(AuthToken::generate());
        let token = auth.as_str().to_string();
        let app = Router::new()
            .route("/", post(super::complete_handler))
            .route_layer(middleware::from_fn_with_state(
                AuthState(auth),
                require_auth,
            ));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            axum::serve(
                listener,
                app.into_make_service_with_connect_info::<SocketAddr>(),
            )
            .await
            .unwrap();
        });
        let client = reqwest::Client::new();
        let url = format!("http://{address}/");

        let public = client
            .post(&url)
            .header(header::HOST, "preview.example")
            .bearer_auth(&token)
            .send()
            .await
            .unwrap();
        assert_eq!(public.status(), reqwest::StatusCode::NO_CONTENT);
        assert_eq!(
            public.headers().get(header::CACHE_CONTROL).unwrap(),
            "no-store"
        );
        let public_cookies = public
            .headers()
            .get_all(header::SET_COOKIE)
            .iter()
            .map(|value| value.to_str().unwrap())
            .collect::<Vec<_>>();
        assert_eq!(public_cookies.len(), 2);
        assert_eq!(
            public_cookies[0],
            "va_owner=; Path=/; Max-Age=0; Secure; HttpOnly; SameSite=Lax"
        );
        assert_eq!(
            public_cookies[1],
            format!("va_owner={token}; Path=/va/; Secure; HttpOnly; SameSite=Lax")
        );

        let local = client.post(&url).bearer_auth(&token).send().await.unwrap();
        assert_eq!(local.status(), reqwest::StatusCode::NO_CONTENT);
        let local_cookies = local
            .headers()
            .get_all(header::SET_COOKIE)
            .iter()
            .map(|value| value.to_str().unwrap())
            .collect::<Vec<_>>();
        assert_eq!(
            local_cookies,
            [
                "va_owner=; Path=/; Max-Age=0; Secure; HttpOnly; SameSite=Lax",
                "va_owner=; Path=/va/; Max-Age=0; Secure; HttpOnly; SameSite=Lax",
            ]
        );

        for response in [
            client
                .post(&url)
                .header(header::HOST, "preview.example")
                .bearer_auth("wrong-token")
                .send()
                .await
                .unwrap(),
            client
                .post(format!("{url}?token={token}"))
                .header(header::HOST, "preview.example")
                .send()
                .await
                .unwrap(),
        ] {
            assert_eq!(response.status(), reqwest::StatusCode::UNAUTHORIZED);
            assert!(response
                .headers()
                .get_all(header::SET_COOKIE)
                .iter()
                .next()
                .is_none());
        }

        server.abort();
    }
}
