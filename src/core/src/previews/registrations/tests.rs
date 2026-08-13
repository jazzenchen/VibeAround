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

fn server_session(workspace: &Path, port: u16, listener: ListenerProcess) -> PreviewSession {
    let workspace = canonical(workspace);
    let id = workspace.join(format!(":port:{port}"));
    PreviewSession {
        id: id.clone(),
        workspace,
        title: format!("server-{port}"),
        target: PreviewTarget::Server { port },
        listener: Some(listener),
        slug: slug_from_path(&id),
        share: None,
        conversation_thread_id: None,
        owner_session: Some("owner-session".into()),
        created_at: Instant::now(),
    }
}

#[test]
fn matching_registrations_restore_files_and_servers_without_private_state() {
    let path = temp_path("matching");
    let workspace = path.with_extension("workspace");
    std::fs::create_dir_all(&workspace).unwrap();
    let first = ListenerProcess {
        pid: 101,
        start_time: 1001,
    };
    let second = ListenerProcess {
        pid: 202,
        start_time: 2002,
    };
    let mut source = HashMap::new();
    let first_session = server_session(&workspace, 4318, first);
    let second_session = server_session(&workspace, 4319, second);
    source.insert(first_session.id.clone(), first_session);
    source.insert(second_session.id.clone(), second_session);
    let file_id = path.with_extension("external.md");
    std::fs::write(&file_id, "preview").unwrap();
    let now = Instant::now();
    source.insert(
        file_id.clone(),
        PreviewSession {
            id: file_id.clone(),
            workspace: canonical(&workspace),
            title: "file".into(),
            target: PreviewTarget::File,
            listener: None,
            slug: slug_from_path(&file_id),
            share: Some(ShareTransaction {
                id: "share-secret".into(),
                code: "123456".into(),
                grant: "grant-secret".into(),
                expires_at: now + Duration::from_secs(60),
                attempt_tokens: SHARE_CODE_ATTEMPT_BURST,
                attempts_refilled_at: now,
            }),
            conversation_thread_id: None,
            owner_session: None,
            created_at: now,
        },
    );

    persist_at(&path, &source).unwrap();
    let json: serde_json::Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    assert_eq!(json.as_array().unwrap().len(), 3);
    let file_object = json
        .as_array()
        .unwrap()
        .iter()
        .find(|registration| registration["kind"] == "file")
        .unwrap()
        .as_object()
        .unwrap();
    assert_eq!(file_object.len(), 4);
    for key in ["file", "kind", "title", "workspace"] {
        assert!(file_object.contains_key(key));
    }
    let server_object = json
        .as_array()
        .unwrap()
        .iter()
        .find(|registration| registration["kind"] == "server")
        .unwrap()
        .as_object()
        .unwrap();
    assert_eq!(server_object.len(), 6);
    for key in [
        "kind",
        "listener",
        "owner_session",
        "port",
        "title",
        "workspace",
    ] {
        assert!(server_object.contains_key(key));
    }
    assert!(!json.to_string().contains("share"));
    assert!(!json.to_string().contains("grant-secret"));
    assert!(!json.to_string().contains("conversation"));

    let mut restored = HashMap::new();
    let count = reconcile_at(&path, &mut restored, |port| match port {
        4318 => Some(first),
        4319 => Some(second),
        _ => None,
    })
    .unwrap();

    assert_eq!(count, 3);
    assert_eq!(restored.len(), 3);
    assert!(restored.values().all(|session| session.share.is_none()));
    assert!(restored
        .values()
        .all(|session| session.conversation_thread_id.is_none()));
    assert_eq!(
        restored.get(&canonical(&file_id)).unwrap().owner_session,
        None
    );
    assert!(restored
        .values()
        .filter(|session| matches!(session.target, PreviewTarget::Server { .. }))
        .all(|session| session.owner_session.as_deref() == Some("owner-session")));

    std::fs::remove_file(path).unwrap();
    std::fs::remove_file(file_id).unwrap();
    std::fs::remove_dir_all(workspace).unwrap();
}

#[test]
fn missing_files_are_pruned_while_external_files_are_restored() {
    let path = temp_path("files");
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
    let count = reconcile_at(&path, &mut restored, |_| {
        panic!("no Server listener expected")
    })
    .unwrap();

    assert_eq!(count, 2);
    assert_eq!(restored.len(), 2);
    assert!(restored.contains_key(&canonical(&valid_file)));
    assert!(restored.contains_key(&canonical(&outside_file)));
    let normalized: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    assert_eq!(normalized.as_array().unwrap().len(), 2);

    std::fs::remove_file(path).unwrap();
    std::fs::remove_dir_all(workspace).unwrap();
    std::fs::remove_dir_all(outside).unwrap();
}

#[test]
fn dead_or_mismatched_listeners_are_dropped_without_killing() {
    let path = temp_path("stale");
    let workspace = path.with_extension("workspace");
    std::fs::create_dir_all(&workspace).unwrap();
    let dead = ListenerProcess {
        pid: 303,
        start_time: 3003,
    };
    let mismatched = ListenerProcess {
        pid: 404,
        start_time: 4004,
    };
    let mut source = HashMap::new();
    let dead_session = server_session(&workspace, 4320, dead);
    let mismatched_session = server_session(&workspace, 4321, mismatched);
    source.insert(dead_session.id.clone(), dead_session);
    source.insert(mismatched_session.id.clone(), mismatched_session);
    persist_at(&path, &source).unwrap();

    let mut checked = Vec::new();
    let mut restored = HashMap::new();
    let count = reconcile_at(&path, &mut restored, |port| {
        checked.push(port);
        match port {
            4320 => None,
            4321 => Some(ListenerProcess {
                pid: mismatched.pid,
                start_time: mismatched.start_time + 1,
            }),
            _ => unreachable!(),
        }
    })
    .unwrap();

    checked.sort_unstable();
    assert_eq!(checked, [4320, 4321]);
    assert_eq!(count, 0);
    assert!(restored.is_empty());
    assert!(!path.exists(), "stale projection is cleared");

    std::fs::remove_dir_all(workspace).unwrap();
}

#[test]
fn legacy_server_lease_file_is_migrated_to_tagged_registrations() {
    let legacy_path = temp_path("legacy");
    let path = temp_path("registrations");
    let workspace = path.with_extension("workspace");
    std::fs::create_dir_all(&workspace).unwrap();
    let listener = ListenerProcess {
        pid: 505,
        start_time: 5005,
    };
    let bytes = serde_json::to_vec_pretty(&vec![LegacyServerLease {
        workspace: workspace.clone(),
        port: 4322,
        title: "legacy".into(),
        listener,
        owner_session: Some("owner-session".into()),
    }])
    .unwrap();
    std::fs::write(&legacy_path, bytes).unwrap();

    migrate_legacy_at(&legacy_path, &path).unwrap();

    assert!(!legacy_path.exists());
    let json: serde_json::Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    assert_eq!(json[0]["kind"], "server");
    assert_eq!(json[0]["port"], 4322);

    let mut restored = HashMap::new();
    assert_eq!(
        reconcile_at(&path, &mut restored, |port| (port == 4322)
            .then_some(listener))
        .unwrap(),
        1
    );

    std::fs::remove_file(path).unwrap();
    std::fs::remove_dir_all(workspace).unwrap();
}

#[test]
fn obsolete_legacy_file_is_removed_when_current_registry_exists() {
    let legacy_path = temp_path("obsolete-legacy");
    let path = temp_path("current");
    std::fs::write(&legacy_path, "stale").unwrap();
    std::fs::write(&path, "[]").unwrap();

    migrate_legacy_at(&legacy_path, &path).unwrap();

    assert!(!legacy_path.exists());
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "[]");
    std::fs::remove_file(path).unwrap();
}
