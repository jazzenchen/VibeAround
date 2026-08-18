use super::*;
use axum::{body::Body, routing::get, Router};

fn local_request(host: &str, peer: &str) -> Request<Body> {
    let mut request = Request::builder()
        .uri("/preview/u/test")
        .header(header::HOST, host)
        .body(Body::empty())
        .unwrap();
    request.extensions_mut().insert(ConnectInfo(
        peer.parse::<SocketAddr>().expect("valid peer address"),
    ));
    request
}

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
fn mcp_uses_only_the_scoped_credential() {
    let owner = Arc::new(AuthToken::generate());
    let mcp = Arc::new(AuthToken::generate());
    let state = AuthState::new(Arc::clone(&owner), Arc::clone(&mcp));

    let request = |path: &str, token: &AuthToken| {
        Request::builder()
            .uri(format!("{path}?token={}", token.as_str()))
            .body(Body::empty())
            .unwrap()
    };
    assert!(request_is_authorized(&state, &request("/mcp", &mcp)));
    assert!(!request_is_authorized(&state, &request("/mcp", &owner)));
    assert!(request_is_authorized(
        &state,
        &request("/api/sessions", &owner)
    ));
    assert!(!request_is_authorized(
        &state,
        &request("/api/sessions", &mcp)
    ));
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
    assert!(is_loopback_host("[::1]"));
    assert!(is_loopback_host("[::1]:12358"));
    assert!(!is_loopback_host("example.com"));
    assert!(!is_loopback_host("example.com:12358"));
    assert!(!is_loopback_host("[::1].example.com"));
    assert!(!is_loopback_host("[::1]evil"));
    assert!(!is_loopback_host("127.0.0.1:not-a-port"));
    assert!(!is_loopback_host("127.0.0.1:65536"));
}

#[test]
fn loopback_request_requires_loopback_peer_and_host() {
    assert!(request_is_loopback(&local_request(
        "127.0.0.1:12358",
        "127.0.0.1:45000"
    )));
    assert!(!request_is_loopback(&local_request(
        "preview.example.com",
        "127.0.0.1:45000"
    )));
    assert!(!request_is_loopback(&local_request(
        "127.0.0.1:12358",
        "192.0.2.10:45000"
    )));
}

#[test]
fn desktop_dev_origin_needs_an_actual_owner_credential() {
    let auth = AuthToken::generate();
    let mut desktop = local_request("127.0.0.1:12358", "127.0.0.1:45000");
    desktop
        .headers_mut()
        .insert(header::ORIGIN, "http://localhost:5181".parse().unwrap());
    assert!(!request_is_local_dashboard(&desktop));
    assert!(!local_bridge_access_allowed(&desktop, &auth));

    desktop.headers_mut().insert(
        header::AUTHORIZATION,
        format!("Bearer {}", auth.as_str()).parse().unwrap(),
    );
    assert!(local_bridge_access_allowed(&desktop, &auth));

    let mut preview = local_request("127.0.0.1:12358", "127.0.0.1:45000");
    preview
        .headers_mut()
        .insert(header::ORIGIN, "http://127.0.0.1:5173".parse().unwrap());
    assert!(!request_is_local_dashboard(&preview));
    assert!(!local_bridge_access_allowed(&preview, &auth));
}

#[tokio::test]
async fn local_bridge_middleware_enforces_the_desktop_credential() {
    let auth = Arc::new(AuthToken::generate());
    let bearer = auth.as_str().to_string();
    let app = Router::new()
        .route("/", get(|| async { StatusCode::OK }))
        .route_layer(axum::middleware::from_fn_with_state(
            AuthState::new(Arc::clone(&auth), auth),
            require_local_bridge,
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

    assert_eq!(
        client
            .get(format!("http://{address}/"))
            .header(header::ORIGIN, "http://localhost:5181")
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::FORBIDDEN,
    );
    assert_eq!(
        client
            .get(format!("http://{address}/"))
            .header(header::ORIGIN, "http://localhost:5181")
            .bearer_auth(&bearer)
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::OK,
    );
    assert_eq!(
        client
            .get(format!("http://{address}/"))
            .header(header::HOST, "preview.example.com")
            .header(header::ORIGIN, "http://localhost:5181")
            .bearer_auth(bearer)
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::FORBIDDEN,
    );
    server.abort();
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
