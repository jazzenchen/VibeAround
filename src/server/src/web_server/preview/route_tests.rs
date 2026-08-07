use axum::body::Body;
use axum::extract::{ConnectInfo, Path};
use axum::http::{Request, StatusCode};
use axum::routing::get;
use axum::Router;
use std::net::SocketAddr;

use super::{owner_preview_handler, share_preview_handler};

fn request_with_host_and_peer(host: &str, peer: &str) -> Request<Body> {
    let mut request = Request::builder()
        .header("host", host)
        .body(Body::empty())
        .unwrap();
    request.extensions_mut().insert(ConnectInfo(
        peer.parse::<SocketAddr>().expect("valid peer address"),
    ));
    request
}

fn local_request(host: &str) -> Request<Body> {
    request_with_host_and_peer(host, "127.0.0.1:45000")
}

fn unique_temp_dir(label: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "vibearound-preview-route-{label}-{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn owner_preview_trusts_loopback_hosts() {
    for host in [
        "localhost",
        "localhost:12358",
        "127.0.0.1",
        "127.0.0.1:12358",
        "::1",
        "[::1]:12358",
    ] {
        assert!(
            crate::web_server::auth::request_is_loopback(&local_request(host)),
            "{host}"
        );
    }
}

#[test]
fn owner_preview_does_not_trust_non_loopback_hosts() {
    for host in ["example.com", "example.com:12358", "192.168.1.20:12358"] {
        assert!(
            !crate::web_server::auth::request_is_loopback(&local_request(host)),
            "{host}"
        );
    }
    assert!(!crate::web_server::auth::request_is_loopback(
        &request_with_host_and_peer("127.0.0.1:12358", "192.0.2.1:45000")
    ));
}

#[tokio::test]
async fn share_preview_accepts_only_ephemeral_file_share_key() {
    let dir = unique_temp_dir("share");
    let file = dir.join("share.md");
    std::fs::write(&file, "# Shared markdown").unwrap();
    let (owner_slug, share_key) = common::previews::ensure_file(file, dir, "share".into());

    let error = share_preview_handler(Path(owner_slug)).await;
    assert_eq!(error.status(), StatusCode::NOT_FOUND);
    assert_eq!(error.headers().get("cache-control").unwrap(), "no-store");

    let response = share_preview_handler(Path(share_key)).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers().get("cache-control").unwrap(), "no-store");
    assert!(response.headers().get("set-cookie").is_none());
}

#[tokio::test]
async fn server_preview_is_local_only_and_accepts_only_owner_slug() {
    let dir = unique_temp_dir("owner");
    let owner_slug = common::previews::ensure_server(4212, dir.clone(), "owner".into(), None);
    let file = dir.join("other.md");
    std::fs::write(&file, "other").unwrap();
    let (_, share_key) = common::previews::ensure_file(file, dir, "other".into());

    let error = owner_preview_handler(Path(share_key), local_request("127.0.0.1:12358")).await;
    assert_eq!(error.status(), StatusCode::NOT_FOUND);

    let local =
        owner_preview_handler(Path(owner_slug.clone()), local_request("127.0.0.1:12358")).await;
    assert_eq!(local.status(), StatusCode::OK);
    assert!(local
        .headers()
        .get("set-cookie")
        .unwrap()
        .to_str()
        .unwrap()
        .contains(&format!("va_preview=owner:{owner_slug}")));

    let public = owner_preview_handler(
        Path(owner_slug.clone()),
        local_request("preview.example.com"),
    )
    .await;
    assert_eq!(public.status(), StatusCode::FORBIDDEN);
    assert!(public.headers().get("location").is_none());
    assert!(public.headers().get("set-cookie").is_none());

    let share_route = share_preview_handler(Path(owner_slug)).await;
    assert_eq!(share_route.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn nested_owner_redirect_preserves_full_path_and_query() {
    let dir = unique_temp_dir("nested-owner");
    let file = dir.join("nested.md");
    std::fs::write(&file, "nested").unwrap();
    let (owner_slug, _) = common::previews::ensure_file(file, dir, "nested".into());
    let app = Router::new().nest(
        "/va",
        Router::new().route("/preview/u/{slug}", get(owner_preview_handler)),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap();

    let response = client
        .get(format!(
            "http://{address}/va/preview/u/{owner_slug}?view=compact"
        ))
        .header("host", "preview.example.com")
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::FOUND);
    assert_eq!(response.headers().get("cache-control").unwrap(), "no-store");
    assert_eq!(
        response.headers().get("location").unwrap(),
        format!(
            "/va/?next=%2Fva%2Fpreview%2Fu%2F{}%3Fview%3Dcompact",
            owner_slug
        )
        .as_str()
    );
    server.abort();
}
