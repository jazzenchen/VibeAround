use super::*;
use crate::previews::store::{ShareTransaction, SHARE_CODE_ATTEMPT_BURST};
use std::time::Duration;

fn temp_path(label: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "va-preview-registrations-{label}-{}-{}.json",
        std::process::id(),
        uuid::Uuid::new_v4()
    ))
}

fn file_session(file: &Path, workspace: &Path) -> PreviewSession {
    let file = canonical(file);
    let now = Instant::now();
    PreviewSession {
        id: file.clone(),
        workspace: canonical(workspace),
        title: "file".into(),
        target: PreviewTarget::File,
        slug: slug_from_path(&file),
        share: Some(ShareTransaction {
            id: "share-secret".into(),
            code: "123456".into(),
            grant: "grant-secret".into(),
            expires_at: now + Duration::from_secs(60),
            attempt_tokens: SHARE_CODE_ATTEMPT_BURST,
            attempts_refilled_at: now,
        }),
        conversation_thread_id: None,
        created_at: now,
    }
}

#[test]
fn persistence_keeps_only_files_without_private_state() {
    let path = temp_path("files-only");
    let workspace = path.with_extension("workspace");
    std::fs::create_dir_all(&workspace).unwrap();
    let file = path.with_extension("external.md");
    std::fs::write(&file, "preview").unwrap();

    let mut source = HashMap::new();
    let file_session = file_session(&file, &workspace);
    source.insert(file_session.id.clone(), file_session);
    let server_id = canonical(&workspace).join(":port:4318");
    source.insert(
        server_id.clone(),
        PreviewSession {
            id: server_id.clone(),
            workspace: canonical(&workspace),
            title: "server".into(),
            target: PreviewTarget::Server { port: 4318 },
            slug: slug_from_path(&server_id),
            share: None,
            conversation_thread_id: None,
            created_at: Instant::now(),
        },
    );

    persist_at(&path, &source).unwrap();
    let json: serde_json::Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    assert_eq!(json.as_array().unwrap().len(), 1);
    assert_eq!(json[0]["kind"], "file");
    assert!(!json.to_string().contains("share-secret"));
    assert!(!json.to_string().contains("grant-secret"));
    assert!(!json.to_string().contains("conversation"));

    let mut restored = HashMap::new();
    assert_eq!(reconcile_at(&path, &mut restored).unwrap(), 1);
    assert_eq!(restored.len(), 1);
    assert!(restored.contains_key(&canonical(&file)));
    assert!(restored.values().all(|session| session.share.is_none()));

    std::fs::remove_file(path).unwrap();
    std::fs::remove_file(file).unwrap();
    std::fs::remove_dir_all(workspace).unwrap();
}

#[test]
fn missing_files_are_pruned_while_external_files_are_restored() {
    let path = temp_path("external");
    let workspace = path.with_extension("workspace");
    let outside = path.with_extension("outside");
    std::fs::create_dir_all(&workspace).unwrap();
    std::fs::create_dir_all(&outside).unwrap();
    let valid_file = workspace.join("README.md");
    let missing_file = workspace.join("MISSING.md");
    let outside_file = outside.join("README.md");
    std::fs::write(&valid_file, "valid").unwrap();
    std::fs::write(&outside_file, "outside").unwrap();

    persist_registrations_at(
        &path,
        vec![
            PreviewRegistration::File {
                file: valid_file.clone(),
                workspace: workspace.clone(),
                title: "valid".into(),
            },
            PreviewRegistration::File {
                file: missing_file,
                workspace: workspace.clone(),
                title: "missing".into(),
            },
            PreviewRegistration::File {
                file: outside_file.clone(),
                workspace: workspace.clone(),
                title: "outside".into(),
            },
        ],
    )
    .unwrap();

    let mut restored = HashMap::new();
    assert_eq!(reconcile_at(&path, &mut restored).unwrap(), 2);
    assert!(restored.contains_key(&canonical(&valid_file)));
    assert!(restored.contains_key(&canonical(&outside_file)));

    std::fs::remove_file(path).unwrap();
    std::fs::remove_dir_all(workspace).unwrap();
    std::fs::remove_dir_all(outside).unwrap();
}

#[test]
fn old_server_registrations_are_discarded_without_hiding_files() {
    let path = temp_path("old-server");
    let workspace = path.with_extension("workspace");
    std::fs::create_dir_all(&workspace).unwrap();
    let file = workspace.join("README.md");
    std::fs::write(&file, "preview").unwrap();
    let json = serde_json::json!([
        {
            "kind": "server",
            "workspace": workspace,
            "port": 4318,
            "title": "old server",
            "listener": { "pid": 101, "start_time": 1001 },
            "owner_session": "old-session"
        },
        {
            "kind": "file",
            "file": file,
            "workspace": workspace,
            "title": "file"
        }
    ]);
    std::fs::write(&path, serde_json::to_vec_pretty(&json).unwrap()).unwrap();

    let mut restored = HashMap::new();
    assert_eq!(reconcile_at(&path, &mut restored).unwrap(), 1);
    assert!(restored
        .values()
        .all(|session| matches!(session.target, PreviewTarget::File)));
    let normalized: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    assert_eq!(normalized.as_array().unwrap().len(), 1);
    assert_eq!(normalized[0]["kind"], "file");

    std::fs::remove_file(path).unwrap();
    std::fs::remove_dir_all(workspace).unwrap();
}

#[test]
fn obsolete_server_lease_file_is_removed() {
    let legacy_path = temp_path("legacy");
    std::fs::write(&legacy_path, "stale").unwrap();

    remove_legacy_at(&legacy_path).unwrap();

    assert!(!legacy_path.exists());
}
