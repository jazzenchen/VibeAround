use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use axum::body::Body;
use axum::extract::ConnectInfo;
use axum::http::{Method, Request, StatusCode};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use super::{
    lookup_preview_cookie, owner_routing_cookie, proxy_request, share_routing_cookie,
    PREVIEW_COOKIE,
};

fn request_with_host(host: &str) -> Request<Body> {
    request(host, "ignored", Method::GET, "document")
}

fn request(host: &str, cookie: &str, method: Method, destination: &str) -> Request<Body> {
    let mut request = Request::builder()
        .method(method)
        .uri("/asset.js")
        .header("host", host)
        .header("sec-fetch-dest", destination)
        .header("cookie", format!("{PREVIEW_COOKIE}={cookie}"))
        .body(Body::empty())
        .unwrap();
    request.extensions_mut().insert(ConnectInfo(
        "127.0.0.1:45000"
            .parse::<SocketAddr>()
            .expect("valid peer address"),
    ));
    request
}

#[test]
fn preview_cookie_preserves_owner_and_share_boundaries() {
    let dir = std::env::temp_dir().join(format!(
        "vibearound-preview-cookie-{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let (owner_slug, share_key) = common::previews::ensure_server(4213, dir, "cookie".into(), None);

    let loopback = request_with_host("127.0.0.1:12358");
    let public = request_with_host("preview.example.com");
    let owner_cookie = owner_routing_cookie(&owner_slug);
    let share_cookie = share_routing_cookie(&share_key);

    assert!(lookup_preview_cookie(&loopback, &owner_cookie).is_some());
    assert!(lookup_preview_cookie(&public, &owner_cookie).is_none());
    assert!(lookup_preview_cookie(&public, &share_cookie).is_some());
    assert!(lookup_preview_cookie(&loopback, &owner_slug).is_none());
    assert!(lookup_preview_cookie(&loopback, &owner_routing_cookie(&share_key)).is_none());
    assert!(lookup_preview_cookie(&public, &share_routing_cookie(&owner_slug)).is_none());
}

#[tokio::test]
async fn api_and_write_requests_are_rejected_before_upstream() {
    let client = reqwest::Client::new();
    let dist = std::env::temp_dir();

    let write = proxy_request(
        &client,
        &dist,
        request("127.0.0.1:12358", "ignored", Method::POST, "iframe"),
    )
    .await;
    assert_eq!(write.status(), StatusCode::METHOD_NOT_ALLOWED);

    let api = proxy_request(
        &client,
        &dist,
        request("127.0.0.1:12358", "ignored", Method::GET, "empty"),
    )
    .await;
    assert_eq!(api.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn public_owner_cookie_cannot_reach_preview_upstream() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let upstream_hit = Arc::new(AtomicBool::new(false));
    let server_hit = Arc::clone(&upstream_hit);
    let mock_server = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        server_hit.store(true, Ordering::SeqCst);
        let mut request = [0_u8; 1024];
        let _ = socket.read(&mut request).await.unwrap();
        socket
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nSet-Cookie: va_preview=evil\r\nClear-Site-Data: \"cookies\"\r\nConnection: close\r\n\r\nok")
            .await
            .unwrap();
    });

    let dir =
        std::env::temp_dir().join(format!("vibearound-preview-proxy-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let (owner_slug, share_key) =
        common::previews::ensure_server(port, dir.clone(), "proxy".into(), None);
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap();

    let forged_owner = owner_routing_cookie(&owner_slug);
    let response = proxy_request(
        &client,
        &dir,
        request("preview.example.com", &forged_owner, Method::GET, "script"),
    )
    .await;
    assert_eq!(response.status(), StatusCode::FOUND);
    assert_eq!(response.headers().get("cache-control").unwrap(), "no-store");
    assert!(!upstream_hit.load(Ordering::SeqCst));

    let valid_share = share_routing_cookie(&share_key);
    let response = proxy_request(
        &client,
        &dir,
        request("preview.example.com", &valid_share, Method::GET, "script"),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    assert!(response.headers().get("set-cookie").is_none());
    assert!(response.headers().get("clear-site-data").is_none());
    assert_eq!(response.headers().get("cache-control").unwrap(), "no-store");
    let body = axum::body::to_bytes(response.into_body(), 16)
        .await
        .unwrap();
    assert_eq!(&body[..], b"ok");
    mock_server.await.unwrap();
    assert!(upstream_hit.load(Ordering::SeqCst));
}
