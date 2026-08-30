use super::{is_dashboard_api_path, mount_dashboard, redirect_to_dashboard};
use axum::{routing::get, Router};

#[tokio::test]
async fn dashboard_mount_serves_trailing_slash_without_redirect_loop() {
    let dashboard = Router::new()
        .route(
            "/health",
            get(|| async { axum::http::StatusCode::NO_CONTENT }),
        )
        .fallback(|| async { "dashboard" });
    let app = mount_dashboard(dashboard);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap();

    let root = client
        .get(format!("http://{address}/va/"))
        .send()
        .await
        .unwrap();
    assert_eq!(root.status(), reqwest::StatusCode::OK);
    assert_eq!(root.text().await.unwrap(), "dashboard");

    let nested_fallback = client
        .get(format!("http://{address}/va/unknown"))
        .send()
        .await
        .unwrap();
    assert_eq!(nested_fallback.status(), reqwest::StatusCode::OK);
    assert_eq!(nested_fallback.text().await.unwrap(), "dashboard");

    let health = client
        .get(format!("http://{address}/va/health"))
        .send()
        .await
        .unwrap();
    assert_eq!(health.status(), reqwest::StatusCode::NO_CONTENT);

    for path in ["/va", "/outside"] {
        let response = client
            .get(format!("http://{address}{path}"))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), reqwest::StatusCode::TEMPORARY_REDIRECT);
        assert_eq!(response.headers().get("location").unwrap(), "/va/");
    }

    server.abort();
}

#[tokio::test]
async fn root_fallback_redirects_to_dashboard() {
    let response = axum::response::IntoResponse::into_response(redirect_to_dashboard().await);
    assert_eq!(
        response.status(),
        axum::http::StatusCode::TEMPORARY_REDIRECT
    );
    assert_eq!(response.headers().get("location").unwrap(), "/va/");
}

#[test]
fn recognizes_dashboard_api_fallback_paths() {
    assert!(is_dashboard_api_path(
        "/va/local-api/deepseek/scope/extra/openai-chat/v1/responses"
    ));
    assert!(is_dashboard_api_path(
        "/va/local-agent/claude/direct/v1/responses"
    ));
    assert!(is_dashboard_api_path(
        "/local-api/deepseek/scope/extra/openai-chat/v1/responses"
    ));
    assert!(is_dashboard_api_path(
        "/local-agent/claude/direct/v1/responses"
    ));
    assert!(is_dashboard_api_path(
        "/va/bridge/profile/openai-chat/v1/responses"
    ));
    assert!(!is_dashboard_api_path("/va/"));
    assert!(!is_dashboard_api_path("/va/assets/index.css"));
}
