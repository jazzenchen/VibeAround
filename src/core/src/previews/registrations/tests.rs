use super::*;
use crate::previews::store::{slug_from_path, ShareTransaction, SHARE_CODE_ATTEMPT_BURST};
use std::time::{Duration, Instant};

fn temp_path(label: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "va-preview-cleanup-{label}-{}-{}.json",
        std::process::id(),
        uuid::Uuid::new_v4()
    ))
}

fn session(id: PathBuf, target: PreviewTarget) -> PreviewSession {
    let now = Instant::now();
    PreviewSession {
        id: id.clone(),
        workspace: PathBuf::from("/private/workspace"),
        title: "private title".into(),
        target,
        slug: slug_from_path(&id),
        share: Some(ShareTransaction {
            id: "share-secret".into(),
            code: "123456".into(),
            grant: "grant-secret".into(),
            expires_at: now + Duration::from_secs(60),
            attempt_tokens: SHARE_CODE_ATTEMPT_BURST,
            attempts_refilled_at: now,
        }),
        created_at: now,
    }
}

#[test]
fn journal_contains_only_cleanup_information() {
    let path = temp_path("minimal");
    let mut sessions = HashMap::new();
    let file = session(PathBuf::from("/private/file.md"), PreviewTarget::File);
    let server = session(
        PathBuf::from("/private/workspace/:port:4318"),
        PreviewTarget::Server { port: 4318 },
    );
    sessions.insert(file.id.clone(), file);
    sessions.insert(server.id.clone(), server);

    persist_at(&path, &sessions).unwrap();
    let contents = std::fs::read_to_string(&path).unwrap();
    let json: serde_json::Value = serde_json::from_str(&contents).unwrap();
    assert_eq!(json.as_array().unwrap().len(), 2);
    assert!(contents.contains("\"kind\": \"file\""));
    assert!(contents.contains("\"kind\": \"server\""));
    assert!(contents.contains("\"port\": 4318"));
    for private_value in [
        "/private/file.md",
        "/private/workspace",
        "private title",
        "share-secret",
        "123456",
        "grant-secret",
    ] {
        assert!(!contents.contains(private_value));
    }
    assert_eq!(current_server_ports_at(&path).unwrap(), vec![4318]);

    remove_at(&path).unwrap();
}

#[test]
fn old_combined_registration_format_yields_only_server_ports() {
    let path = temp_path("old-combined");
    let json = serde_json::json!([
        {
            "kind": "file",
            "file": "/tmp/README.md",
            "workspace": "/tmp",
            "title": "old file"
        },
        {
            "kind": "server",
            "workspace": "/tmp",
            "port": 5173,
            "title": "old server",
            "listener": { "pid": 101, "start_time": 1001 },
            "owner_session": "old-session"
        }
    ]);
    std::fs::write(&path, serde_json::to_vec_pretty(&json).unwrap()).unwrap();

    assert_eq!(current_server_ports_at(&path).unwrap(), vec![5173]);
    remove_at(&path).unwrap();
}

#[test]
fn legacy_server_lease_format_yields_ports() {
    let path = temp_path("legacy");
    let json = serde_json::json!([
        {
            "workspace": "/tmp",
            "port": 3000,
            "title": "legacy",
            "listener": { "pid": 101, "start_time": 1001 }
        }
    ]);
    std::fs::write(&path, serde_json::to_vec_pretty(&json).unwrap()).unwrap();

    assert_eq!(legacy_server_ports_at(&path).unwrap(), vec![3000]);
    remove_at(&path).unwrap();
}

#[test]
fn empty_registry_removes_the_journal() {
    let path = temp_path("empty");
    std::fs::write(&path, "stale").unwrap();

    persist_at(&path, &HashMap::new()).unwrap();

    assert!(!path.exists());
}
