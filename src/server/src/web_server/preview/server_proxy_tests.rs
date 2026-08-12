use std::net::SocketAddr;
use std::sync::Arc;

use axum::body::{to_bytes, Body};
use axum::extract::ConnectInfo;
use axum::http::{header, Method, Request, StatusCode};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use super::{
    proxy_request, server_routing_cookie, share_server_routing_cookie, signed_slug,
    verified_server_entry, SERVER_ROUTING_COOKIE,
};
use crate::web_server::auth::AuthState;

fn auth_state() -> AuthState {
    AuthState(Arc::new(common::auth::AuthToken::generate()))
}

fn proxy_request_for(
    uri: &str,
    method: Method,
    destination: &str,
    cookie: Option<&str>,
    auth: &AuthState,
) -> Request<Body> {
    let mut builder = Request::builder()
        .uri(uri)
        .method(method)
        .header(header::HOST, "preview.example.com")
        .header("sec-fetch-dest", destination);
    if let Some(cookie) = cookie {
        builder = builder.header(header::COOKIE, cookie);
    }
    let mut request = builder.body(Body::empty()).unwrap();
    request.extensions_mut().insert(auth.clone());
    request.extensions_mut().insert(ConnectInfo(
        "127.0.0.1:45000".parse::<SocketAddr>().unwrap(),
    ));
    request
}

fn unique_temp_dir(label: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "vibearound-server-proxy-{label}-{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn owner_routing_is_daemon_bound_and_share_routing_revalidates_the_grant() {
    let dir = unique_temp_dir("signature");
    let (slug, share) = common::previews::ensure_server(4318, dir.clone(), "server".into(), None);
    let auth = auth_state();
    let other_auth = auth_state();
    let signed = signed_slug(&slug, &auth);

    assert!(verified_server_entry(&signed, &auth).is_some());
    assert!(verified_server_entry(&signed, &other_auth).is_none());
    assert!(verified_server_entry(&format!("{signed}0"), &auth).is_none());

    let (server_entry, grant) =
        common::previews::verify_share_code(&share.id, &share.code).unwrap();
    let share_cookie = share_server_routing_cookie(&share.id, &grant, &server_entry);
    let share_route = share_cookie
        .split(';')
        .next()
        .unwrap()
        .split_once('=')
        .unwrap()
        .1;
    assert!(verified_server_entry(share_route, &auth).is_some());
    assert!(verified_server_entry(share_route, &other_auth).is_some());
    assert!(verified_server_entry(&format!("share:{}:wrong-grant", share.id), &auth).is_none());

    let file = dir.join("README.md");
    std::fs::write(&file, "readme").unwrap();
    let (file_slug, file_share) = common::previews::ensure_file(file, dir.clone(), "file".into());
    assert!(verified_server_entry(&signed_slug(&file_slug, &auth), &auth).is_none());
    let (file_entry, file_grant) =
        common::previews::verify_share_code(&file_share.id, &file_share.code).unwrap();
    let file_cookie = share_server_routing_cookie(&file_share.id, &file_grant, &file_entry);
    let file_route = file_cookie
        .split(';')
        .next()
        .unwrap()
        .split_once('=')
        .unwrap()
        .1;
    assert!(verified_server_entry(file_route, &auth).is_none());
    assert!(verified_server_entry(&format!("share:{}:{file_grant}", share.id), &auth).is_none());
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn rejects_api_write_worker_and_websocket_before_connecting() {
    let client = reqwest::Client::new();
    let auth = auth_state();

    for (method, destination, expected) in [
        (Method::POST, "iframe", StatusCode::METHOD_NOT_ALLOWED),
        (Method::GET, "empty", StatusCode::FORBIDDEN),
        (Method::GET, "worker", StatusCode::FORBIDDEN),
    ] {
        let response = proxy_request(
            &client,
            proxy_request_for("/api/data", method, destination, None, &auth),
        )
        .await;
        assert_eq!(response.status(), expected);
    }

    let mut websocket = proxy_request_for("/socket", Method::GET, "empty", None, &auth);
    websocket
        .headers_mut()
        .insert(header::UPGRADE, "websocket".parse().unwrap());
    assert_eq!(
        proxy_request(&client, websocket).await.status(),
        StatusCode::FORBIDDEN
    );
}

#[tokio::test]
async fn never_proxies_the_dashboard_namespace() {
    let client = reqwest::Client::new();
    let auth = auth_state();
    let response = proxy_request(
        &client,
        proxy_request_for("/va", Method::GET, "iframe", None, &auth),
    )
    .await;

    assert_eq!(response.status(), StatusCode::TEMPORARY_REDIRECT);
    assert_eq!(response.headers().get(header::LOCATION).unwrap(), "/va/");
}

#[tokio::test]
async fn proxies_static_path_and_query_without_forwarding_credentials() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = tokio::spawn(async move {
        loop {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut bytes = vec![0_u8; 4096];
            let read = socket.read(&mut bytes).await.unwrap();
            if read == 0 {
                continue;
            }
            let request = String::from_utf8_lossy(&bytes[..read]).to_string();
            socket
                .write_all(
                    b"HTTP/1.1 200 OK\r\nContent-Type: application/javascript\r\nContent-Length: 2\r\nETag: test\r\nSet-Cookie: upstream=secret\r\nContent-Security-Policy: default-src 'none'\r\nConnection: close\r\n\r\nok",
                )
                .await
                .unwrap();
            break request;
        }
    });
    let dir = unique_temp_dir("static");
    let (_, share) = common::previews::ensure_server(port, dir.clone(), "server".into(), None);
    let auth = auth_state();
    let (entry, grant) = common::previews::verify_share_code(&share.id, &share.code).unwrap();
    let cookie = share_server_routing_cookie(&share.id, &grant, &entry)
        .split(';')
        .next()
        .unwrap()
        .to_string();
    let mut request = proxy_request_for(
        "/assets/app.js?v=7",
        Method::GET,
        "script",
        Some(&cookie),
        &auth,
    );
    request.headers_mut().insert(
        header::AUTHORIZATION,
        "Bearer owner-secret".parse().unwrap(),
    );
    request.headers_mut().insert(
        header::ORIGIN,
        "https://preview.example.com".parse().unwrap(),
    );
    request.headers_mut().insert(
        header::REFERER,
        "https://preview.example.com/".parse().unwrap(),
    );
    request
        .headers_mut()
        .insert(header::RANGE, "bytes=0-1".parse().unwrap());

    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap();
    let response = proxy_request(&client, request).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.headers().get(header::CONTENT_TYPE).unwrap(),
        "application/javascript"
    );
    assert_eq!(response.headers().get(header::ETAG).unwrap(), "test");
    assert!(response.headers().get(header::SET_COOKIE).is_none());
    assert!(response.headers().get("content-security-policy").is_none());
    assert_eq!(
        response.headers().get(header::CACHE_CONTROL).unwrap(),
        "no-store"
    );
    assert_eq!(
        to_bytes(response.into_body(), 16).await.unwrap().as_ref(),
        b"ok"
    );

    let upstream_request = server.await.unwrap().to_ascii_lowercase();
    assert!(upstream_request.starts_with("get /assets/app.js?v=7 http/1.1"));
    assert!(upstream_request.contains("range: bytes=0-1"));
    assert!(!upstream_request.contains("owner-secret"));
    assert!(!upstream_request.contains("va_preview_server"));
    assert!(!upstream_request.contains(&share.id));
    assert!(!upstream_request.contains(&grant));
    assert!(!upstream_request.contains("origin:"));
    assert!(!upstream_request.contains("referer:"));
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn rewrites_absolute_loopback_redirects_to_the_tunnel_origin() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = tokio::spawn(async move {
        loop {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut bytes = [0_u8; 1024];
            if socket.read(&mut bytes).await.unwrap() == 0 {
                continue;
            }
            let response = format!(
                "HTTP/1.1 302 Found\r\nLocation: http://localhost:{port}/welcome?from=preview\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
            );
            socket.write_all(response.as_bytes()).await.unwrap();
            break;
        }
    });
    let dir = unique_temp_dir("redirect");
    let (slug, _) = common::previews::ensure_server(port, dir.clone(), "server".into(), None);
    let auth = auth_state();
    let cookie = server_routing_cookie(&slug, &auth)
        .split(';')
        .next()
        .unwrap()
        .to_string();
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap();
    let response = proxy_request(
        &client,
        proxy_request_for("/", Method::GET, "iframe", Some(&cookie), &auth),
    )
    .await;

    assert_eq!(response.status(), StatusCode::FOUND);
    assert_eq!(
        response.headers().get(header::LOCATION).unwrap(),
        "/welcome?from=preview"
    );
    server.await.unwrap();
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn cookie_header_uses_the_root_path_without_exposing_the_owner_token() {
    let auth = auth_state();
    let cookie = server_routing_cookie("server-slug", &auth);
    assert!(cookie.starts_with(&format!("{SERVER_ROUTING_COOKIE}=server-slug.")));
    assert!(cookie.contains("Path=/"));
    assert!(cookie.contains("Secure"));
    assert!(cookie.contains("HttpOnly"));
    assert!(!cookie.contains(auth.0.as_str()));
}

#[test]
fn share_cookie_uses_the_existing_grant_and_share_deadline() {
    let dir = unique_temp_dir("share-cookie");
    let (_, share) = common::previews::ensure_server(4319, dir.clone(), "server".into(), None);
    let (entry, grant) = common::previews::verify_share_code(&share.id, &share.code).unwrap();
    let cookie = share_server_routing_cookie(&share.id, &grant, &entry);

    assert!(cookie.starts_with(&format!(
        "{SERVER_ROUTING_COOKIE}=share:{}:{grant};",
        share.id
    )));
    assert!(cookie.contains("Path=/"));
    assert!(cookie.contains("Max-Age="));
    assert!(cookie.contains("Secure"));
    assert!(cookie.contains("HttpOnly"));
    assert!(!cookie.contains(&share.code));
    std::fs::remove_dir_all(dir).unwrap();
}
