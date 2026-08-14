use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use axum::body::{to_bytes, Body};
use axum::extract::ws::WebSocketUpgrade;
use axum::extract::ConnectInfo;
use axum::http::{header, Method, Request, StatusCode};
use axum::routing::{any, get};
use axum::{Extension, Router};
use futures_util::{SinkExt, StreamExt};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::Message;

use super::{
    is_daemon_authorization, proxy_request, server_routing_cookie, share_server_routing_cookie,
    signed_slug, verified_server_entry, ServerProxyState, ServerRouteKind, SERVER_ROUTING_COOKIE,
};
use crate::web_server::auth::AuthState;

fn auth_state() -> AuthState {
    AuthState(Arc::new(common::auth::AuthToken::generate()))
}

fn proxy_request_for(
    uri: &str,
    method: Method,
    cookie: Option<&str>,
    auth: &AuthState,
) -> Request<Body> {
    let mut builder = Request::builder()
        .uri(uri)
        .method(method)
        .header(header::HOST, "preview.example.com");
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
    let (slug, share) = common::previews::ensure_server(4318, dir.clone(), "server".into());
    let auth = auth_state();
    let other_auth = auth_state();
    let signed = signed_slug(&slug, &auth);

    assert_eq!(
        verified_server_entry(&signed, &auth).unwrap().kind,
        ServerRouteKind::Owner
    );
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
    assert_eq!(
        verified_server_entry(share_route, &auth).unwrap().kind,
        ServerRouteKind::Share
    );
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
async fn shared_server_rejects_writes_upgrades_and_service_workers() {
    let client = reqwest::Client::new();
    let auth = auth_state();
    let dir = unique_temp_dir("share-transport");
    let (_, share) = common::previews::ensure_server(4318, dir.clone(), "server".into());
    let (entry, grant) = common::previews::verify_share_code(&share.id, &share.code).unwrap();
    let cookie = share_server_routing_cookie(&share.id, &grant, &entry)
        .split(';')
        .next()
        .unwrap()
        .to_string();
    let response = proxy_request(
        &client,
        proxy_request_for("/api/data", Method::POST, Some(&cookie), &auth),
    )
    .await;
    assert_eq!(response.status(), StatusCode::METHOD_NOT_ALLOWED);

    let mut websocket = proxy_request_for("/socket", Method::GET, Some(&cookie), &auth);
    websocket
        .headers_mut()
        .insert(header::UPGRADE, "websocket".parse().unwrap());
    assert_eq!(
        proxy_request(&client, websocket).await.status(),
        StatusCode::NOT_IMPLEMENTED
    );

    let mut service_worker = proxy_request_for("/sw.js", Method::GET, Some(&cookie), &auth);
    service_worker
        .headers_mut()
        .insert("sec-fetch-dest", "serviceworker".parse().unwrap());
    assert_eq!(
        proxy_request(&client, service_worker).await.status(),
        StatusCode::FORBIDDEN
    );
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn owner_server_rejects_service_workers() {
    let client = reqwest::Client::new();
    let auth = auth_state();
    let dir = unique_temp_dir("owner-service-worker");
    let (slug, _) = common::previews::ensure_server(4318, dir.clone(), "server".into());
    let cookie = server_routing_cookie(&slug, &auth)
        .split(';')
        .next()
        .unwrap()
        .to_string();
    let mut request = proxy_request_for("/sw.js", Method::GET, Some(&cookie), &auth);
    request
        .headers_mut()
        .insert("sec-fetch-dest", "serviceworker".parse().unwrap());

    assert_eq!(
        proxy_request(&client, request).await.status(),
        StatusCode::FORBIDDEN
    );
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn never_proxies_the_dashboard_namespace() {
    let client = reqwest::Client::new();
    let auth = auth_state();
    let response = proxy_request(&client, proxy_request_for("/va", Method::GET, None, &auth)).await;

    assert_eq!(response.status(), StatusCode::TEMPORARY_REDIRECT);
    assert_eq!(response.headers().get(header::LOCATION).unwrap(), "/va/");
}

#[tokio::test]
async fn proxies_get_path_and_query_without_forwarding_credentials() {
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
    let (_, share) = common::previews::ensure_server(port, dir.clone(), "server".into());
    let auth = auth_state();
    let (entry, grant) = common::previews::verify_share_code(&share.id, &share.code).unwrap();
    let cookie = share_server_routing_cookie(&share.id, &grant, &entry)
        .split(';')
        .next()
        .unwrap()
        .to_string();
    let mut request = proxy_request_for("/assets/app.js?v=7", Method::GET, Some(&cookie), &auth);
    request
        .headers_mut()
        .insert("sec-fetch-dest", "empty".parse().unwrap());
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
async fn owner_proxy_transparently_forwards_http_to_ipv4_loopback() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut request = Vec::new();
        loop {
            let mut chunk = [0_u8; 1024];
            let read = socket.read(&mut chunk).await.unwrap();
            request.extend_from_slice(&chunk[..read]);
            if request
                .windows(b"\r\n\r\n".len())
                .position(|window| window == b"\r\n\r\n")
                .is_some_and(|headers_end| request.len() >= headers_end + 4 + 11)
            {
                break;
            }
        }
        socket
            .write_all(
                b"HTTP/1.1 201 Created\r\nContent-Type: application/json\r\nContent-Length: 11\r\nX-App-Response: yes\r\nSet-Cookie: app_session=updated\r\nContent-Security-Policy: default-src 'self'\r\nCache-Control: public, max-age=60\r\nConnection: close\r\n\r\n{\"saved\":1}",
            )
            .await
            .unwrap();
        String::from_utf8(request).unwrap()
    });
    let dir = unique_temp_dir("owner-http");
    let (slug, _) = common::previews::ensure_server(port, dir.clone(), "server".into());
    let auth = auth_state();
    let routing_cookie = server_routing_cookie(&slug, &auth)
        .split(';')
        .next()
        .unwrap()
        .to_string();
    let mut request = proxy_request_for("/api/items?draft=1", Method::POST, None, &auth);
    *request.body_mut() = Body::from(r#"{"value":1}"#);
    request.headers_mut().insert(
        header::COOKIE,
        format!("{routing_cookie}; app_session=abc")
            .parse()
            .unwrap(),
    );
    request
        .headers_mut()
        .insert(header::CONTENT_TYPE, "application/json".parse().unwrap());
    request
        .headers_mut()
        .insert(header::CONTENT_LENGTH, "11".parse().unwrap());
    request
        .headers_mut()
        .insert(header::AUTHORIZATION, "Bearer app-secret".parse().unwrap());
    request
        .headers_mut()
        .insert("x-app-request", "yes".parse().unwrap());

    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap();
    let response = proxy_request(&client, request).await;

    assert_eq!(response.status(), StatusCode::CREATED);
    assert_eq!(response.headers()["x-app-response"], "yes");
    assert_eq!(
        response.headers()[header::SET_COOKIE],
        "app_session=updated"
    );
    assert_eq!(
        response.headers()["content-security-policy"],
        "default-src 'self'"
    );
    assert_eq!(
        response.headers()[header::CACHE_CONTROL],
        "public, max-age=60"
    );
    assert_eq!(
        to_bytes(response.into_body(), 16).await.unwrap().as_ref(),
        br#"{"saved":1}"#
    );

    let upstream_request = server.await.unwrap().to_ascii_lowercase();
    assert!(upstream_request.starts_with("post /api/items?draft=1 http/1.1"));
    assert!(upstream_request.contains(&format!("host: 127.0.0.1:{port}")));
    assert!(upstream_request.contains("content-type: application/json"));
    assert!(upstream_request.contains("authorization: bearer app-secret"));
    assert!(upstream_request.contains("x-app-request: yes"));
    assert!(upstream_request.contains("cookie: app_session=abc"));
    assert!(!upstream_request.contains("va_preview_server"));
    assert!(upstream_request.ends_with(r#"{"value":1}"#));
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn owner_proxy_does_not_forward_the_daemon_bearer_token() {
    let auth = auth_state();
    let daemon = format!("Bearer {}", auth.0.as_str()).parse().unwrap();
    let app = "Bearer app-secret".parse().unwrap();

    assert!(is_daemon_authorization(&daemon, &auth));
    assert!(!is_daemon_authorization(&app, &auth));
}

#[tokio::test]
async fn owner_proxy_bridges_websocket_hmr_to_ipv4_loopback() {
    let upstream_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let upstream_port = upstream_listener.local_addr().unwrap().port();
    let upstream_app = Router::new().route(
        "/hmr",
        get(|upgrade: WebSocketUpgrade| async move {
            upgrade.on_upgrade(|mut socket| async move {
                if let Some(Ok(message)) = socket.recv().await {
                    socket.send(message).await.unwrap();
                }
            })
        }),
    );
    let upstream_server = tokio::spawn(async move {
        axum::serve(upstream_listener, upstream_app).await.unwrap();
    });

    let dir = unique_temp_dir("owner-websocket");
    let (slug, _) = common::previews::ensure_server(upstream_port, dir.clone(), "server".into());
    let auth = auth_state();
    let routing_cookie = server_routing_cookie(&slug, &auth)
        .split(';')
        .next()
        .unwrap()
        .to_string();
    let proxy_client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap();
    let proxy_app = Router::new()
        .fallback(any(super::server_proxy_fallback))
        .layer(Extension(auth))
        .layer(Extension(ServerProxyState::new(proxy_client)));
    let proxy_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let proxy_port = proxy_listener.local_addr().unwrap().port();
    let proxy_server = tokio::spawn(async move {
        axum::serve(proxy_listener, proxy_app).await.unwrap();
    });

    let mut request = format!("ws://127.0.0.1:{proxy_port}/hmr?token=dev")
        .into_client_request()
        .unwrap();
    request
        .headers_mut()
        .insert(header::COOKIE, routing_cookie.parse().unwrap());
    let (mut socket, _) = tokio_tungstenite::connect_async(request).await.unwrap();
    socket.send(Message::Text("update".into())).await.unwrap();
    let echoed = tokio::time::timeout(Duration::from_secs(1), socket.next())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert_eq!(echoed, Message::Text("update".into()));

    proxy_server.abort();
    upstream_server.abort();
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn streams_the_upstream_body_after_returning_response_headers() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let (release_tx, release_rx) = tokio::sync::oneshot::channel();
    let server = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut request = [0_u8; 1024];
        socket.read(&mut request).await.unwrap();
        socket
            .write_all(
                b"HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n2\r\nok\r\n",
            )
            .await
            .unwrap();
        let _ = release_rx.await;
        socket.write_all(b"0\r\n\r\n").await.unwrap();
    });
    let dir = unique_temp_dir("stream");
    let (slug, _) = common::previews::ensure_server(port, dir.clone(), "server".into());
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

    let response = tokio::time::timeout(
        Duration::from_secs(1),
        proxy_request(
            &client,
            proxy_request_for("/stream", Method::GET, Some(&cookie), &auth),
        ),
    )
    .await
    .expect("proxy returns after upstream headers");
    assert_eq!(response.status(), StatusCode::OK);
    release_tx.send(()).unwrap();
    assert_eq!(
        to_bytes(response.into_body(), 16).await.unwrap().as_ref(),
        b"ok"
    );

    server.await.unwrap();
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn owner_proxy_preserves_upstream_redirects() {
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
    let (slug, _) = common::previews::ensure_server(port, dir.clone(), "server".into());
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
        proxy_request_for("/", Method::GET, Some(&cookie), &auth),
    )
    .await;

    assert_eq!(response.status(), StatusCode::FOUND);
    assert_eq!(
        response.headers().get(header::LOCATION).unwrap(),
        &format!("http://localhost:{port}/welcome?from=preview")
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
    let (_, share) = common::previews::ensure_server(4319, dir.clone(), "server".into());
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
