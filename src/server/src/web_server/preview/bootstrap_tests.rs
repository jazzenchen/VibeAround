use std::path::PathBuf;

use common::previews::PreviewSnapshot;

use super::owner_preview_bootstrap;

fn snapshot(
    workspace: &str,
    slug: &str,
    title: &str,
    kind: &'static str,
    port: Option<u16>,
) -> PreviewSnapshot {
    PreviewSnapshot {
        slug: slug.into(),
        id: PathBuf::from(workspace).join(title),
        workspace: workspace.into(),
        title: title.into(),
        kind,
        port,
        share_id: Some("private-share-id".into()),
        share_code: Some("123456".into()),
        share_expires_at_ms: Some(1),
        created_at_ms: 1,
    }
}

#[test]
fn bootstrap_contains_only_owner_ui_data_and_server_built_sources() {
    let previews = vec![
        snapshot("/tmp/beta", "server", "App", "server", Some(5173)),
        snapshot("/tmp/alpha", "readme", "README", "file", None),
    ];

    let value = serde_json::to_value(owner_preview_bootstrap(
        "readme",
        &previews,
        Some("localhost"),
    ))
    .unwrap();

    assert_eq!(value["selectedSlug"], "readme");
    assert_eq!(value["previews"][0]["workspace"], "/tmp/alpha");
    assert_eq!(value["previews"][0]["kind"], "file");
    assert_eq!(value["previews"][0]["src"], "/va/preview/u/readme/content");
    assert_eq!(value["previews"][1]["kind"], "server");
    assert_eq!(value["previews"][1]["src"], "http://localhost:5173/");
    assert!(value["previews"][0].get("chatAvailable").is_none());
    assert!(!value.to_string().contains("private-share-id"));
    assert!(!value.to_string().contains("123456"));
}

#[test]
fn remote_bootstrap_routes_server_content_through_the_daemon_origin() {
    let previews = vec![snapshot(
        "/tmp/alpha",
        "server",
        "App",
        "server",
        Some(5173),
    )];

    let value = serde_json::to_value(owner_preview_bootstrap("server", &previews, None)).unwrap();

    assert_eq!(value["previews"][0]["src"], "/va/preview/u/server/content");
}
