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
    assert_eq!(route, preview_web_route_for_slug(&slug));

    let share_error = resolve_owner_chat_route(&share.id, &local_request(), 12358, &[])
        .expect_err("share IDs must not resolve an owner chat route");
    assert_eq!(share_error.0, StatusCode::NOT_FOUND);
}

#[test]
fn owner_chat_auth_does_not_require_an_in_memory_conversation_binding() {
    let (slug, _) = preview_file("unbound");

    let route = resolve_owner_chat_route(&slug, &local_request(), 12358, &[])
        .expect("conversation resolution recovers after owner authorization");
    assert_eq!(route, preview_web_route_for_slug(&slug));
}

#[test]
fn preview_conversation_frame_is_preview_only_and_contains_the_thread_id() {
    let frame = preview_conversation_frame(&WorkspaceThreadId::from("wt_preview_restored"));
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&frame).unwrap(),
        serde_json::json!({
            "kind": "preview_conversation",
            "thread_id": "wt_preview_restored",
        })
    );
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

#[test]
fn owner_chat_accepts_a_paired_remote_server_preview() {
    let dir = std::env::temp_dir().join(format!(
        "vibearound-preview-chat-server-{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let (slug, _) = common::previews::ensure_server(4318, dir.clone(), "server".into(), None);
    let auth = std::sync::Arc::new(common::auth::AuthToken::generate());
    let mut remote = request(
        "preview.example.com",
        "127.0.0.1:45000",
        "https://preview.example.com",
    );
    remote.headers_mut().insert(
        "cookie",
        format!("va_owner={}", auth.as_str()).parse().unwrap(),
    );
    remote
        .extensions_mut()
        .insert(crate::web_server::auth::AuthState(auth));

    let route = resolve_owner_chat_route(
        &slug,
        &remote,
        12358,
        &["https://preview.example.com".to_string()],
    )
    .expect("paired remote Server Preview chat route");
    assert_eq!(route, preview_web_route_for_slug(&slug));
    std::fs::remove_dir_all(dir).unwrap();
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

#[tokio::test]
async fn stale_preview_session_defers_user_message_until_successor_is_ready() {
    let manager = WebChannelManager::new();
    let route = RouteKey::new("web", "ws_preview_successor");
    manager.set_route_agent(&route, "codex".to_string()).await;
    let (tx, mut rx) = manager.sender();
    manager
        .register_connection(&route, "owner".to_string(), tx, false)
        .await;
    manager
        .dispatch_output(ChannelOutput::SessionReady {
            route: route.clone(),
            reply_to: None,
            session_id: "old-session".to_string(),
        })
        .await;
    assert!(matches!(
        rx.try_recv().expect("old session ready"),
        ChannelOutput::SessionReady { .. }
    ));

    let wait_for_session_ready = preview_route_session_is_stale(Some("old-session"), None);
    assert!(wait_for_session_ready);
    manager
        .record_user_message(
            &route,
            "successor-message".to_string(),
            vec![serde_json::json!({"type": "text", "text": "update the preview"})],
            wait_for_session_ready,
        )
        .await;
    assert!(rx.try_recv().is_err());

    manager
        .dispatch_output(ChannelOutput::SessionReady {
            route: route.clone(),
            reply_to: None,
            session_id: "new-session".to_string(),
        })
        .await;
    assert!(matches!(
        rx.try_recv().expect("new session ready"),
        ChannelOutput::SessionReady { .. }
    ));
    let ChannelOutput::RawAcp { payload, .. } = rx.try_recv().expect("successor user message")
    else {
        panic!("expected successor user message replay");
    };
    assert_eq!(payload["sessionId"], "new-session");
    assert_eq!(payload["update"]["messageId"], "successor-message");

    assert!(!preview_route_session_is_stale(
        Some("new-session"),
        Some("new-session")
    ));
    assert!(!preview_route_session_is_stale(None, None));
}

#[tokio::test]
async fn preview_refresh_events_are_live_only_and_filtered_by_owner_slug() {
    let before_subscribe = format!("before-{}", uuid::Uuid::new_v4());
    request_owner_refresh(&before_subscribe);
    let mut rx = PREVIEW_REFRESH_EVENTS.subscribe();
    assert!(matches!(
        rx.try_recv(),
        Err(tokio::sync::broadcast::error::TryRecvError::Empty)
    ));

    let target = format!("target-{}", uuid::Uuid::new_v4());
    request_owner_refresh("another-preview");
    request_owner_refresh(&target);
    receive_owner_refresh(&mut rx, &target).await.unwrap();
}
