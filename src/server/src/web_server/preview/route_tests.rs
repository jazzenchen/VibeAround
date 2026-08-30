use axum::body::{to_bytes, Body};
use axum::extract::{ConnectInfo, Form, Path};
use axum::http::{Request, StatusCode};
use axum::response::Response;
use axum::routing::{get, post};
use axum::Router;
use std::net::SocketAddr;
use std::sync::Arc;

use super::{
    active_preview_snapshots, owner_access_allowed, owner_preview_bootstrap_handler,
    owner_preview_content_handler, owner_preview_response, share_preview_handler,
    verify_share_code_handler, ShareCodeForm,
};

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

fn local_request_with_origin(host: &str, origin: &str) -> Request<Body> {
    let mut request = local_request(host);
    request
        .headers_mut()
        .insert("origin", origin.parse().expect("valid origin"));
    request
}

fn remote_owner_request(host: &str, auth: &Arc<common::auth::AuthToken>) -> Request<Body> {
    let mut request = request_with_host_and_peer(host, "127.0.0.1:45000");
    request.headers_mut().insert(
        "cookie",
        format!("va_owner={}", auth.as_str()).parse().unwrap(),
    );
    request
        .extensions_mut()
        .insert(crate::web_server::auth::AuthState::new(
            Arc::clone(auth),
            Arc::clone(auth),
        ));
    request
}

fn unique_temp_dir(label: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "vibearound-preview-route-{label}-{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

async fn owner_preview_for_test(Path(slug): Path<String>, req: Request<Body>) -> Response {
    let web_dist = unique_temp_dir("owner-spa");
    std::fs::write(
        web_dist.join("index.html"),
        "<!doctype html><div id=\"root\" data-preview-spa></div>",
    )
    .unwrap();
    let response = owner_preview_response(Path(slug), req, web_dist.clone()).await;
    std::fs::remove_dir_all(web_dist).unwrap();
    response
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

#[test]
fn local_preview_bypass_rejects_external_browser_origins() {
    assert!(crate::web_server::auth::request_is_local_dashboard(
        &local_request("127.0.0.1:12358")
    ));
    assert!(crate::web_server::auth::request_is_local_dashboard(
        &local_request_with_origin("127.0.0.1:12358", "http://127.0.0.1:12358")
    ));
    assert!(!crate::web_server::auth::request_is_local_dashboard(
        &local_request_with_origin("127.0.0.1:12358", "https://evil.example")
    ));
    assert!(!crate::web_server::auth::request_is_local_dashboard(
        &local_request_with_origin("127.0.0.1:12358", "http://127.0.0.1:5173")
    ));
    assert!(!crate::web_server::auth::request_is_local_dashboard(
        &local_request_with_origin("127.0.0.1:12358", "http://localhost:5181")
    ));
}

#[test]
fn owner_cookie_uses_the_daemon_auth_state() {
    let auth = Arc::new(common::auth::AuthToken::generate());
    let mut request = request_with_host_and_peer("preview.example.com", "127.0.0.1:45000");
    request.headers_mut().insert(
        "cookie",
        format!("va_owner={}", auth.as_str()).parse().unwrap(),
    );
    request
        .extensions_mut()
        .insert(crate::web_server::auth::AuthState::new(
            Arc::clone(&auth),
            Arc::clone(&auth),
        ));
    assert!(owner_access_allowed(&request));

    request
        .extensions_mut()
        .insert(crate::web_server::auth::AuthState::new(
            Arc::new(common::auth::AuthToken::generate()),
            Arc::new(common::auth::AuthToken::generate()),
        ));
    assert!(!owner_access_allowed(&request));
}

#[tokio::test]
async fn public_share_requires_access_code_and_issues_scoped_grant() {
    let dir = unique_temp_dir("share");
    let file = dir.join("share.md");
    std::fs::write(&file, "# Shared markdown").unwrap();
    let (owner_slug, share) = common::previews::ensure_file(file, dir.clone(), "share".into());

    let error = share_preview_handler(
        Path(owner_slug.clone()),
        local_request("preview.example.com"),
    )
    .await;
    assert_eq!(error.status(), StatusCode::NOT_FOUND);
    assert_eq!(error.headers().get("cache-control").unwrap(), "no-store");

    let gate =
        share_preview_handler(Path(share.id.clone()), local_request("preview.example.com")).await;
    assert_eq!(gate.status(), StatusCode::OK);
    assert_eq!(gate.headers().get("cache-control").unwrap(), "no-store");
    let gate_csp = gate
        .headers()
        .get("content-security-policy")
        .unwrap()
        .to_str()
        .unwrap();
    assert!(gate_csp.contains("form-action 'self'"));
    assert!(gate_csp.contains("frame-ancestors 'none'"));
    assert!(gate_csp.contains("style-src 'self' 'unsafe-inline'"));
    assert!(gate_csp.contains("img-src 'self'"));
    assert!(gate.headers().get("set-cookie").is_none());
    let gate_body = to_bytes(gate.into_body(), usize::MAX).await.unwrap();
    let gate_body = String::from_utf8(gate_body.to_vec()).unwrap();
    assert!(gate_body.contains("Enter access code"));
    assert!(gate_body.contains("href=\"/va/preview/assets/theme-"));
    assert!(gate_body.contains(".css?v="));
    assert!(gate_body.contains("src=\"/va/brand/vibearound-mark.svg\""));
    assert!(gate_body.contains("background: var(--primary)"));
    assert!(!gate_body.contains("#0969da"));
    assert!(gate_body.contains("inputmode=\"numeric\""));
    assert!(gate_body.contains("autocomplete=\"one-time-code\""));
    assert!(gate_body.contains("pattern=\"[0-9]{6}\""));
    assert!(!gate_body.contains("maxlength="));
    assert_eq!(gate_body.matches("class=\"slot\"").count(), 6);
    assert!(!gate_body.contains("# Shared markdown"));
    assert!(!gate_body.contains(&dir.display().to_string()));

    let wrong_code = if share.code == "999999" {
        "000000"
    } else {
        "999999"
    };
    let wrong = verify_share_code_handler(
        Path(share.id.clone()),
        Ok(Form(ShareCodeForm {
            code: wrong_code.into(),
        })),
    )
    .await;
    assert_eq!(wrong.status(), StatusCode::UNAUTHORIZED);
    assert!(wrong.headers().get("set-cookie").is_none());

    let verified = verify_share_code_handler(
        Path(share.id.clone()),
        Ok(Form(ShareCodeForm {
            code: share.code.clone(),
        })),
    )
    .await;
    assert_eq!(verified.status(), StatusCode::SEE_OTHER);
    assert_eq!(
        verified.headers().get("location").unwrap(),
        format!("/va/preview/s/{}", share.id).as_str()
    );
    let set_cookie = verified
        .headers()
        .get("set-cookie")
        .unwrap()
        .to_str()
        .unwrap()
        .to_string();
    assert!(set_cookie.starts_with(&format!("va_preview_share_{}=", share.id)));
    assert!(set_cookie.contains(&format!("Path=/va/preview/s/{}", share.id)));
    assert!(set_cookie.contains("Secure"));
    assert!(set_cookie.contains("HttpOnly"));
    assert!(set_cookie.contains("SameSite=Lax"));
    assert!(!set_cookie.contains(&share.code));
    let verified_body = to_bytes(verified.into_body(), usize::MAX).await.unwrap();
    assert!(verified_body.is_empty());

    let cookie_pair = set_cookie.split(';').next().unwrap();
    let mut authorized_request = local_request("preview.example.com");
    authorized_request
        .headers_mut()
        .insert("cookie", cookie_pair.parse().unwrap());
    let response = share_preview_handler(Path(share.id.clone()), authorized_request).await;
    assert_eq!(response.status(), StatusCode::OK);
    let share_csp = response
        .headers()
        .get("content-security-policy")
        .unwrap()
        .to_str()
        .unwrap();
    assert!(share_csp.contains("form-action 'none'"));
    assert!(share_csp.contains("frame-ancestors 'none'"));
    let response_body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let response_body = String::from_utf8(response_body.to_vec()).unwrap();
    assert!(response_body.contains("# Shared markdown"));
    assert!(!response_body.contains(&dir.display().to_string()));

    let second_viewer = verify_share_code_handler(
        Path(share.id.clone()),
        Ok(Form(ShareCodeForm {
            code: share.code.clone(),
        })),
    )
    .await;
    assert_eq!(second_viewer.status(), StatusCode::SEE_OTHER);

    let other_file = dir.join("other-share.md");
    std::fs::write(&other_file, "# Other markdown").unwrap();
    let (_, other_share) = common::previews::ensure_file(other_file, dir.clone(), "other".into());
    let mut cross_share_request = local_request("preview.example.com");
    cross_share_request
        .headers_mut()
        .insert("cookie", cookie_pair.parse().unwrap());
    let cross_share =
        share_preview_handler(Path(other_share.id.clone()), cross_share_request).await;
    assert_eq!(cross_share.status(), StatusCode::OK);
    let cross_body = to_bytes(cross_share.into_body(), usize::MAX).await.unwrap();
    let cross_body = String::from_utf8(cross_body.to_vec()).unwrap();
    assert!(cross_body.contains("Enter access code"));
    assert!(!cross_body.contains("# Other markdown"));

    let local_share =
        share_preview_handler(Path(share.id.clone()), local_request("127.0.0.1:12358")).await;
    assert_eq!(local_share.status(), StatusCode::OK);
    let local_body = to_bytes(local_share.into_body(), usize::MAX).await.unwrap();
    assert!(String::from_utf8(local_body.to_vec())
        .unwrap()
        .contains("# Shared markdown"));

    let external_origin = share_preview_handler(
        Path(other_share.id),
        local_request_with_origin("127.0.0.1:12358", "https://evil.example"),
    )
    .await;
    let external_body = to_bytes(external_origin.into_body(), usize::MAX)
        .await
        .unwrap();
    assert!(String::from_utf8(external_body.to_vec())
        .unwrap()
        .contains("Enter access code"));

    let external_owner = owner_preview_for_test(
        Path(owner_slug.clone()),
        local_request_with_origin("127.0.0.1:12358", "https://evil.example"),
    )
    .await;
    assert_eq!(external_owner.status(), StatusCode::FOUND);

    let owner =
        owner_preview_for_test(Path(owner_slug.clone()), local_request("127.0.0.1:12358")).await;
    assert_eq!(owner.status(), StatusCode::OK);
    assert_eq!(owner.headers().get("cache-control").unwrap(), "no-store");
    let owner_csp = owner
        .headers()
        .get("content-security-policy")
        .unwrap()
        .to_str()
        .unwrap();
    assert!(owner_csp.contains("default-src 'none'"));
    assert!(owner_csp.contains("script-src-attr 'none'"));
    assert!(owner_csp.contains("connect-src 'self'"));
    assert!(owner_csp.contains("frame-src 'self'"));
    assert!(owner_csp.contains("frame-ancestors 'none'"));
    let owner_body = to_bytes(owner.into_body(), usize::MAX).await.unwrap();
    let owner_body = String::from_utf8(owner_body.to_vec()).unwrap();
    assert!(owner_body.contains("data-preview-spa"));
    assert!(!owner_body.contains("# Shared markdown"));
    assert!(!owner_body.contains(&share.id));
    assert!(!owner_body.contains(&share.code));

    let owner_content =
        owner_preview_content_handler(Path(owner_slug), local_request("127.0.0.1:12358")).await;
    assert_eq!(owner_content.status(), StatusCode::OK);
    let content_csp = owner_content
        .headers()
        .get("content-security-policy")
        .unwrap()
        .to_str()
        .unwrap();
    assert!(content_csp.contains("img-src 'self' https:"));
    assert!(content_csp.contains("frame-ancestors 'self'"));
    let content_body = to_bytes(owner_content.into_body(), usize::MAX)
        .await
        .unwrap();
    let content_body = String::from_utf8(content_body.to_vec()).unwrap();
    assert!(content_body.contains("# Shared markdown"));
    assert!(!content_body.contains("class=\"toolbar\""));
    assert!(content_body.contains("/va/preview/assets/review-bridge-"));
}

#[tokio::test]
async fn public_server_share_uses_the_existing_code_and_grant_without_owner_ui() {
    let dir = unique_temp_dir("server-share");
    let (owner_slug, share) =
        common::previews::ensure_server(4320, dir.clone(), "Shared server".into());

    let gate =
        share_preview_handler(Path(share.id.clone()), local_request("preview.example.com")).await;
    assert_eq!(gate.status(), StatusCode::OK);
    let gate_body = to_bytes(gate.into_body(), usize::MAX).await.unwrap();
    assert!(String::from_utf8(gate_body.to_vec())
        .unwrap()
        .contains("Enter access code"));

    // A loopback request does not silently turn a public Server share into
    // owner access; Server shares still enter through the same code gate.
    let local_gate =
        share_preview_handler(Path(share.id.clone()), local_request("127.0.0.1:12358")).await;
    let local_gate_body = to_bytes(local_gate.into_body(), usize::MAX).await.unwrap();
    assert!(String::from_utf8(local_gate_body.to_vec())
        .unwrap()
        .contains("Enter access code"));

    let verified = verify_share_code_handler(
        Path(share.id.clone()),
        Ok(Form(ShareCodeForm {
            code: share.code.clone(),
        })),
    )
    .await;
    assert_eq!(verified.status(), StatusCode::SEE_OTHER);
    let grant_cookie = verified
        .headers()
        .get("set-cookie")
        .unwrap()
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_string();
    let grant = grant_cookie.split_once('=').unwrap().1.to_string();

    let mut authorized_request = local_request("preview.example.com");
    authorized_request
        .headers_mut()
        .insert("cookie", grant_cookie.parse().unwrap());
    let response = share_preview_handler(Path(share.id.clone()), authorized_request).await;
    assert_eq!(response.status(), StatusCode::OK);
    let root_cookie = response
        .headers()
        .get("set-cookie")
        .unwrap()
        .to_str()
        .unwrap();
    assert!(root_cookie.starts_with(&format!("va_preview_server=share:{}:{grant};", share.id)));
    assert!(root_cookie.contains("Path=/"));
    assert!(root_cookie.contains("Max-Age="));
    assert!(root_cookie.contains("Secure"));
    assert!(root_cookie.contains("HttpOnly"));
    assert!(!root_cookie.contains(&share.code));
    assert!(!root_cookie.contains(&owner_slug));

    let csp = response
        .headers()
        .get("content-security-policy")
        .unwrap()
        .to_str()
        .unwrap();
    assert!(csp.contains("frame-src 'self'"));
    assert!(csp.contains("form-action 'none'"));
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let body = String::from_utf8(body.to_vec()).unwrap();
    assert!(body.contains("<iframe src=\"/\""));
    assert!(body.contains("Shared server"));
    assert!(!body.contains("data-preview-spa"));
    assert!(!body.contains("review-bridge"));
    assert!(!body.contains("chat"));
    assert!(!body.contains(&share.id));
    assert!(!body.contains(&grant));
    assert!(!body.contains(&owner_slug));

    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn access_code_form_is_extracted_through_the_real_route() {
    let dir = unique_temp_dir("share-form");
    let file = dir.join("form.md");
    std::fs::write(&file, "form").unwrap();
    let (_, share) = common::previews::ensure_file(file, dir, "form".into());
    let app = Router::new().route("/va/preview/s/{share_id}", post(verify_share_code_handler));
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
        .post(format!("http://{address}/va/preview/s/{}", share.id))
        .form(&[("code", share.code)])
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::SEE_OTHER);
    assert_eq!(response.headers().get("cache-control").unwrap(), "no-store");
    assert!(response.headers().get("set-cookie").is_some());
    server.abort();
}

#[tokio::test]
async fn failed_access_codes_are_rate_limited_per_share_link() {
    let dir = unique_temp_dir("share-rate-limit");
    let file_a = dir.join("a.md");
    let file_b = dir.join("b.md");
    std::fs::write(&file_a, "a").unwrap();
    std::fs::write(&file_b, "b").unwrap();
    let (_, share_a) = common::previews::ensure_file(file_a, dir.clone(), "a".into());
    let (_, share_b) = common::previews::ensure_file(file_b, dir, "b".into());
    let wrong_code = if share_a.code == "999999" {
        "000000"
    } else {
        "999999"
    };

    for _ in 0..common::previews::SHARE_CODE_ATTEMPT_BURST {
        let response = verify_share_code_handler(
            Path(share_a.id.clone()),
            Ok(Form(ShareCodeForm {
                code: wrong_code.into(),
            })),
        )
        .await;
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    let limited = verify_share_code_handler(
        Path(share_a.id),
        Ok(Form(ShareCodeForm { code: share_a.code })),
    )
    .await;
    assert_eq!(limited.status(), StatusCode::TOO_MANY_REQUESTS);
    let retry_after = limited
        .headers()
        .get("retry-after")
        .unwrap()
        .to_str()
        .unwrap()
        .to_string();
    let limited_body = to_bytes(limited.into_body(), usize::MAX).await.unwrap();
    assert!(String::from_utf8(limited_body.to_vec())
        .unwrap()
        .contains(&format!("Try again in {retry_after} seconds.")));

    let other_share = verify_share_code_handler(
        Path(share_b.id),
        Ok(Form(ShareCodeForm { code: share_b.code })),
    )
    .await;
    assert_eq!(other_share.status(), StatusCode::SEE_OTHER);
}

#[tokio::test]
async fn server_preview_uses_direct_local_origin_and_remote_same_origin_proxy() {
    let dir = unique_temp_dir("owner");
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let (owner_slug, _) = common::previews::ensure_server(port, dir.clone(), "owner".into());
    let file = dir.join("other.md");
    std::fs::write(&file, "other").unwrap();
    let (file_owner_slug, share) = common::previews::ensure_file(file, dir, "other".into());

    let error =
        owner_preview_for_test(Path(share.id.clone()), local_request("127.0.0.1:12358")).await;
    assert_eq!(error.status(), StatusCode::NOT_FOUND);

    let local =
        owner_preview_for_test(Path(owner_slug.clone()), local_request("127.0.0.1:12358")).await;
    assert_eq!(local.status(), StatusCode::OK);
    let local_cookies = local
        .headers()
        .get_all("set-cookie")
        .iter()
        .map(|value| value.to_str().unwrap().to_string())
        .collect::<Vec<_>>();
    assert_eq!(local_cookies.len(), 4);
    assert!(local_cookies
        .iter()
        .any(|cookie| cookie.starts_with("va_owner=; Path=/;")));
    assert!(local_cookies
        .iter()
        .any(|cookie| cookie.starts_with("va_owner=; Path=/va/;")));
    assert!(local_cookies
        .iter()
        .any(|cookie| cookie.starts_with("va_preview=; Path=/;")));
    assert!(local_cookies
        .iter()
        .any(|cookie| cookie.starts_with("va_preview_server=; Path=/;")));
    let local_csp = local
        .headers()
        .get("content-security-policy")
        .unwrap()
        .to_str()
        .unwrap()
        .to_string();
    let local_body = to_bytes(local.into_body(), usize::MAX).await.unwrap();
    let local_body = String::from_utf8(local_body.to_vec()).unwrap();
    assert!(local_body.contains("data-preview-spa"));
    assert!(local_csp.contains(&format!("http://127.0.0.1:{port}")));
    assert!(!local_body.contains(&share.id));
    assert!(!local_body.contains(&share.code));

    let bootstrap =
        owner_preview_bootstrap_handler(Path(owner_slug.clone()), local_request("127.0.0.1:12358"))
            .await;
    assert_eq!(bootstrap.status(), StatusCode::OK);
    let bootstrap_body = to_bytes(bootstrap.into_body(), usize::MAX).await.unwrap();
    let bootstrap: serde_json::Value = serde_json::from_slice(&bootstrap_body).unwrap();
    assert!(bootstrap["previews"]
        .as_array()
        .unwrap()
        .iter()
        .any(|preview| preview["slug"] == file_owner_slug));
    assert!(bootstrap["previews"]
        .as_array()
        .unwrap()
        .iter()
        .any(|preview| preview["src"] == format!("http://127.0.0.1:{port}/")));
    assert!(!bootstrap.to_string().contains(&share.id));
    assert!(!bootstrap.to_string().contains(&share.code));

    let content =
        owner_preview_content_handler(Path(owner_slug.clone()), local_request("127.0.0.1:12358"))
            .await;
    assert_eq!(content.status(), StatusCode::BAD_REQUEST);
    assert!(content.headers().get("set-cookie").is_none());
    let content_body = to_bytes(content.into_body(), usize::MAX).await.unwrap();
    assert!(String::from_utf8(content_body.to_vec())
        .unwrap()
        .contains("load directly from their local origin"));

    let unauthorized = owner_preview_for_test(
        Path(owner_slug.clone()),
        local_request("preview.example.com"),
    )
    .await;
    assert_eq!(unauthorized.status(), StatusCode::FOUND);
    assert!(unauthorized.headers().get("set-cookie").is_none());

    let auth = Arc::new(common::auth::AuthToken::generate());
    let public = owner_preview_for_test(
        Path(owner_slug.clone()),
        remote_owner_request("preview.example.com", &auth),
    )
    .await;
    assert_eq!(public.status(), StatusCode::OK);
    let public_csp = public
        .headers()
        .get("content-security-policy")
        .unwrap()
        .to_str()
        .unwrap();
    assert!(public_csp.contains("frame-src 'self'"));
    assert!(!public_csp.contains(&format!("localhost:{port}")));

    let public_bootstrap = owner_preview_bootstrap_handler(
        Path(owner_slug.clone()),
        remote_owner_request("preview.example.com", &auth),
    )
    .await;
    assert_eq!(public_bootstrap.status(), StatusCode::OK);
    let body = to_bytes(public_bootstrap.into_body(), usize::MAX)
        .await
        .unwrap();
    let body: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert!(body["previews"].as_array().unwrap().iter().any(|preview| {
        preview["slug"] == owner_slug
            && preview["src"] == format!("/va/preview/u/{owner_slug}/content")
    }));

    let public_content = owner_preview_content_handler(
        Path(owner_slug.clone()),
        remote_owner_request("preview.example.com", &auth),
    )
    .await;
    assert_eq!(public_content.status(), StatusCode::FOUND);
    assert_eq!(public_content.headers().get("location").unwrap(), "/");
    let routing_cookie = public_content
        .headers()
        .get("set-cookie")
        .unwrap()
        .to_str()
        .unwrap();
    assert!(routing_cookie.starts_with(&format!("va_preview_server={owner_slug}.")));
    assert!(!routing_cookie.contains(auth.as_str()));

    let share_route = share_preview_handler(
        Path(owner_slug.clone()),
        local_request("preview.example.com"),
    )
    .await;
    assert_eq!(share_route.status(), StatusCode::NOT_FOUND);

    let remote_options = active_preview_snapshots().await;
    assert!(remote_options
        .iter()
        .any(|preview| preview.slug == owner_slug));
    assert!(remote_options
        .iter()
        .any(|preview| preview.slug == file_owner_slug));
}

#[tokio::test]
async fn active_previews_keep_registered_server_after_listener_closes() {
    let dir = unique_temp_dir("registered-server");
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let (server_slug, _) = common::previews::ensure_server(port, dir.clone(), "server".into());
    let file = dir.join("active.md");
    std::fs::write(&file, "active").unwrap();
    let (file_slug, _) = common::previews::ensure_file(file, dir, "file".into());

    drop(listener);
    let active = active_preview_snapshots().await;
    assert!(active.iter().any(|preview| preview.slug == server_slug));
    assert!(active.iter().any(|preview| preview.slug == file_slug));

    let bootstrap =
        owner_preview_bootstrap_handler(Path(file_slug.clone()), local_request("127.0.0.1:12358"))
            .await;
    assert_eq!(bootstrap.status(), StatusCode::OK);
    let bootstrap_body = to_bytes(bootstrap.into_body(), usize::MAX).await.unwrap();
    let bootstrap: serde_json::Value = serde_json::from_slice(&bootstrap_body).unwrap();
    assert!(bootstrap["previews"]
        .as_array()
        .unwrap()
        .iter()
        .any(|preview| preview["slug"] == server_slug));
    assert!(bootstrap["previews"]
        .as_array()
        .unwrap()
        .iter()
        .any(|preview| preview["slug"] == file_slug));
}

#[tokio::test]
async fn unauthorized_owner_content_pairs_back_to_the_owner_app() {
    let dir = unique_temp_dir("content-pairing");
    let file = dir.join("paired.md");
    std::fs::write(&file, "paired").unwrap();
    let (owner_slug, _) = common::previews::ensure_file(file, dir, "paired".into());

    let response = owner_preview_content_handler(
        Path(owner_slug.clone()),
        local_request("preview.example.com"),
    )
    .await;

    assert_eq!(response.status(), StatusCode::FOUND);
    assert_eq!(
        response.headers().get("location").unwrap(),
        format!("/va/?next=%2Fva%2Fpreview%2Fu%2F{owner_slug}").as_str()
    );
}

#[tokio::test]
async fn nested_owner_redirect_preserves_full_path_and_query() {
    let dir = unique_temp_dir("nested-owner");
    let file = dir.join("nested.md");
    std::fs::write(&file, "nested").unwrap();
    let (owner_slug, _) = common::previews::ensure_file(file, dir, "nested".into());
    let web_dist = unique_temp_dir("nested-owner-spa");
    std::fs::write(
        web_dist.join("index.html"),
        "<!doctype html><div id=\"root\" data-preview-spa></div>",
    )
    .unwrap();
    let route_dist = web_dist.clone();
    let app = Router::new().nest(
        "/va",
        Router::new().route(
            "/preview/u/{slug}",
            get(move |path, req| owner_preview_response(path, req, route_dist.clone())),
        ),
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
    std::fs::remove_dir_all(web_dist).unwrap();
}
