use axum::body::Body;
use axum::extract::ConnectInfo;
use axum::http::{Request, StatusCode};
use std::net::SocketAddr;

use common::channels::{ChannelOutput, WebChannelManager};
use common::workspace::threads::WorkspaceThreadId;

use super::*;

fn request(host: &str, peer: &str, origin: &str) -> Request<Body> {
    let mut request = Request::builder()
        .uri("/preview/u/test/chat")
        .header("host", host)
        .header("origin", origin)
        .body(Body::empty())
        .unwrap();
    request.extensions_mut().insert(ConnectInfo(
        peer.parse::<SocketAddr>().expect("valid peer address"),
    ));
    request
}

fn local_request() -> Request<Body> {
    request(
        "127.0.0.1:12358",
        "127.0.0.1:45000",
        "http://127.0.0.1:12358",
    )
}

fn preview_file(label: &str) -> (String, common::previews::PreviewShare) {
    let dir = std::env::temp_dir().join(format!(
        "vibearound-preview-chat-{label}-{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("README.md");
    std::fs::write(&file, "# Preview chat").unwrap();
    common::previews::ensure_file(file, dir, label.to_string())
}

#[test]
fn owner_chat_resolves_only_the_bound_child_route() {
    let (slug, share) = preview_file("bound");
    let child = WorkspaceThreadId::from("wt_preview_child");
    common::previews::bind_owner_conversation(&slug, child.clone()).unwrap();

    let route =
        resolve_owner_chat_route(&slug, &local_request(), 12358, &[]).expect("owner chat route");
    assert_eq!(route, web_route_for_thread(&child));

    let share_error = resolve_owner_chat_route(&share.id, &local_request(), 12358, &[])
        .expect_err("share IDs must not resolve an owner chat route");
    assert_eq!(share_error.0, StatusCode::NOT_FOUND);
}

#[test]
fn owner_chat_requires_an_existing_conversation_binding() {
    let (slug, _) = preview_file("unbound");

    let error = resolve_owner_chat_route(&slug, &local_request(), 12358, &[])
        .expect_err("unbound Preview must not create a conversation implicitly");
    assert_eq!(error.0, StatusCode::CONFLICT);
}

#[test]
fn owner_chat_enforces_origin_and_strict_loopback_access() {
    let (slug, _) = preview_file("auth");
    common::previews::bind_owner_conversation(&slug, WorkspaceThreadId::from("wt_preview_auth"))
        .unwrap();

    let hostile_origin = request("127.0.0.1:12358", "127.0.0.1:45000", "https://evil.example");
    assert_eq!(
        resolve_owner_chat_route(&slug, &hostile_origin, 12358, &[])
            .expect_err("hostile origin")
            .0,
        StatusCode::FORBIDDEN
    );

    let forwarded_host = request(
        "preview.example.com",
        "127.0.0.1:45000",
        "https://preview.example.com",
    );
    assert_eq!(
        resolve_owner_chat_route(
            &slug,
            &forwarded_host,
            12358,
            &["https://preview.example.com".to_string()],
        )
        .expect_err("remote access needs the owner cookie")
        .0,
        StatusCode::UNAUTHORIZED
    );

    let remote_peer = request(
        "127.0.0.1:12358",
        "192.0.2.10:45000",
        "http://127.0.0.1:12358",
    );
    assert_eq!(
        resolve_owner_chat_route(&slug, &remote_peer, 12358, &[])
            .expect_err("loopback Host alone is insufficient")
            .0,
        StatusCode::UNAUTHORIZED
    );
}

#[tokio::test]
async fn unregistering_preview_connection_preserves_route_replay() {
    let manager = WebChannelManager::new();
    let route = RouteKey::new("web", "ws_preview_lifecycle");
    let (first_tx, mut first_rx) = manager.sender();
    manager
        .register_connection(&route, "first".to_string(), first_tx, true)
        .await;
    manager
        .dispatch_output(ChannelOutput::SystemText {
            route: route.clone(),
            text: "kept after disconnect".to_string(),
            reply_to: None,
        })
        .await;
    assert!(matches!(
        first_rx.try_recv().expect("first delivery"),
        ChannelOutput::SystemText { .. }
    ));

    manager.unregister_connection(&route, "first").await;

    let (second_tx, mut second_rx) = manager.sender();
    manager
        .register_connection(&route, "second".to_string(), second_tx, true)
        .await;
    assert_eq!(
        second_rx.try_recv().expect("replayed delivery"),
        ChannelOutput::SystemText {
            route,
            text: "kept after disconnect".to_string(),
            reply_to: None,
        }
    );
}
