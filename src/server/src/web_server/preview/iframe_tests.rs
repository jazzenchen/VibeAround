use std::path::PathBuf;
use std::time::Instant;

use axum::body::to_bytes;
use common::previews::{PreviewEntry, PreviewSnapshot, PreviewTarget};

use super::render_owner_shell;

fn snapshot(
    workspace: &str,
    slug: &str,
    title: &str,
    port: Option<u16>,
    created_at_ms: u64,
) -> PreviewSnapshot {
    PreviewSnapshot {
        slug: slug.into(),
        id: PathBuf::from(workspace).join(title),
        workspace: workspace.into(),
        title: title.into(),
        kind: if port.is_some() { "server" } else { "file" },
        port,
        share_id: Some("secret-share-id".into()),
        share_code: Some("123456".into()),
        share_expires_at_ms: Some(1),
        created_at_ms,
    }
}

#[tokio::test]
async fn owner_shell_groups_previews_without_rendering_share_credentials() {
    let entry = PreviewEntry {
        id: "/tmp/beta/readme.md".into(),
        workspace: "/tmp/beta".into(),
        title: "Read <me>".into(),
        target: PreviewTarget::File,
        created_at: Instant::now(),
        expires_at: None,
    };
    let previews = vec![
        snapshot("/tmp/beta", "beta-server", "Web", Some(5173), 2),
        snapshot("/tmp/alpha", "alpha-readme", "Guide", None, 1),
        snapshot("/tmp/beta", "beta-readme", "Read <me>", None, 1),
    ];

    let response = render_owner_shell(&entry, "beta-readme", &previews, "localhost");
    let csp = response
        .headers()
        .get("content-security-policy")
        .unwrap()
        .to_str()
        .unwrap()
        .to_string();
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let body = String::from_utf8(body.to_vec()).unwrap();

    assert!(csp.contains("frame-src 'self'"));
    assert!(csp.contains("http://localhost:5173"));
    assert!(csp.contains("frame-ancestors 'none'"));
    assert!(csp.contains("script-src-attr 'none'"));
    assert_eq!(body.matches("<optgroup").count(), 2);
    assert!(body.find("label=\"alpha\"").unwrap() < body.find("label=\"beta\"").unwrap());
    assert!(body.contains(
        "value=\"beta-readme\" data-title=\"Read &lt;me&gt;\" data-src=\"/va/preview/u/beta-readme/content\" selected"
    ));
    assert!(body
        .contains("value=\"beta-server\" data-title=\"Web\" data-src=\"http://localhost:5173/\""));
    assert!(body.contains("Web · :5173"));
    assert!(body.contains("Guide · Markdown"));
    assert!(body.contains("src=\"/va/preview/u/beta-readme/content\""));
    assert!(body.contains("referrerpolicy=\"no-referrer\""));
    assert!(body.contains("aria-label=\"Workspace and preview\""));
    assert!(body.contains("frame.title = \"Preview content — \" + title"));
    assert!(body.contains("location.reload()"));
    assert!(!body.contains("<details class=\"switcher\" open>"));
    assert!(!body.contains("secret-share-id"));
    assert!(!body.contains("123456"));
    assert!(!body.contains("Read <me>"));
}

#[tokio::test]
async fn server_owner_shell_loads_the_dev_server_origin_directly() {
    let entry = PreviewEntry {
        id: "/tmp/web/:port:4173".into(),
        workspace: "/tmp/web".into(),
        title: "Web".into(),
        target: PreviewTarget::Server { port: 4173 },
        created_at: Instant::now(),
        expires_at: None,
    };
    let previews = vec![snapshot("/tmp/web", "web-server", "Web", Some(4173), 1)];

    let response = render_owner_shell(&entry, "web-server", &previews, "localhost");
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let body = String::from_utf8(body.to_vec()).unwrap();

    assert!(body.contains("src=\"http://localhost:4173/\""));
    assert!(!body.contains("va_preview"));
    assert!(!body.contains("/va/preview/u/web-server/content\"></iframe>"));
}
