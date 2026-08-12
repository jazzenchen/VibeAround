use super::*;
use std::path::Path;
use std::time::Duration;

#[test]
fn slug_from_full_path_is_stable_and_unique() {
    assert_eq!(slug_from_path(Path::new("/tmp/my-app")), "tmp-my-app");
    assert_eq!(
        slug_from_path(Path::new("/tmp/my-app/README.md")),
        "tmp-my-app-readme-md"
    );
    assert_ne!(
        slug_from_path(Path::new("/a/readme.md")),
        slug_from_path(Path::new("/b/readme.md")),
    );
}

#[test]
fn ensure_server_is_idempotent() {
    let path = std::env::temp_dir().join("va-preview-test-server");
    std::fs::create_dir_all(&path).unwrap();

    let before = Instant::now();
    let (slug_a, share_a) = ensure_server(3000, path.clone(), "t".into(), None);
    let (slug_b, share_b) = ensure_server(3000, path.clone(), "t".into(), None);
    assert_eq!(slug_a, slug_b);
    assert_eq!(share_a, share_b);
    assert!(share_a.expires_at >= before + SHARE_TTL);
    assert_eq!(share_a.code.len(), SHARE_CODE_LENGTH);
    assert!(share_a.code.bytes().all(|byte| byte.is_ascii_digit()));

    let snapshot = list_snapshots()
        .into_iter()
        .find(|preview| preview.slug == slug_a)
        .expect("server preview is listed");
    assert_eq!(snapshot.share_id.as_deref(), Some(share_a.id.as_str()));
    assert_eq!(snapshot.share_code.as_deref(), Some(share_a.code.as_str()));
    assert!(snapshot.share_expires_at_ms.is_some());
}

#[test]
fn ensure_server_keeps_different_ports_separate() {
    let path = std::env::temp_dir().join("va-preview-test-multiport");
    std::fs::create_dir_all(&path).unwrap();

    let (slug_a, _) = ensure_server(3456, path.clone(), "liquid".into(), None);
    let (slug_b, _) = ensure_server(5000, path.clone(), "python".into(), None);
    assert_ne!(
        slug_a, slug_b,
        "same workspace + different ports must not collapse"
    );

    let entry_a = lookup_owner(&slug_a).expect("slug A still registered");
    let entry_b = lookup_owner(&slug_b).expect("slug B still registered");
    assert!(matches!(
        entry_a.target,
        PreviewTarget::Server { port: 3456 }
    ));
    assert!(matches!(
        entry_b.target,
        PreviewTarget::Server { port: 5000 }
    ));
}

#[test]
fn server_registration_tracks_the_current_listener_fingerprint() {
    let workspace =
        std::env::temp_dir().join(format!("va-preview-test-listener-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&workspace).unwrap();
    let workspace = canonical(&workspace);
    let id = workspace.join(":port:3457");
    let first = ListenerProcess {
        pid: 123,
        start_time: 456,
    };
    let replacement = ListenerProcess {
        pid: 789,
        start_time: 1011,
    };

    ensure_session(
        id.clone(),
        workspace.clone(),
        "server".into(),
        PreviewTarget::Server { port: 3457 },
        None,
        Some(first),
    );
    ensure_session(
        id.clone(),
        workspace.clone(),
        "server".into(),
        PreviewTarget::Server { port: 3457 },
        None,
        Some(replacement),
    );
    ensure_session(
        id.clone(),
        workspace,
        "server".into(),
        PreviewTarget::Server { port: 3457 },
        None,
        None,
    );

    assert_eq!(
        SESSIONS.lock().get(&id).unwrap().listener,
        Some(replacement)
    );
}

#[test]
fn ensure_file_is_idempotent_and_independent_of_server() {
    let dir = std::env::temp_dir().join("va-preview-test-file");
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("README.md");
    std::fs::write(&file, "hi").unwrap();

    let (srv_slug, srv_share) = ensure_server(4000, dir.clone(), "srv".into(), None);
    let (file_slug_a, file_share_a) = ensure_file(file.clone(), dir.clone(), "md".into());
    let (file_slug_b, file_share_b) = ensure_file(file.clone(), dir.clone(), "md".into());

    assert_ne!(srv_slug, file_slug_a, "server and file share different ids");
    assert_ne!(srv_share.id, file_share_a.id);
    assert_eq!(file_slug_a, file_slug_b);
    assert_eq!(file_share_a, file_share_b);
    assert_eq!(file_share_a.id.len(), 32);
    assert!(file_share_a.id.bytes().all(|byte| byte.is_ascii_hexdigit()));
    assert_eq!(file_share_a.code.len(), SHARE_CODE_LENGTH);
    assert!(file_share_a.code.bytes().all(|byte| byte.is_ascii_digit()));
}

#[test]
fn lookups_preserve_owner_and_share_boundaries() {
    let dir = std::env::temp_dir().join("va-preview-test-lookup");
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("share.md");
    std::fs::write(&file, "share").unwrap();

    let (server_slug, server_share) = ensure_server(4100, dir.clone(), "server".into(), None);
    let (file_slug, share) = ensure_file(file, dir, "file".into());

    assert!(lookup_owner(&server_slug).is_some());
    assert!(lookup_owner(&file_slug).is_some());
    assert!(lookup_share_link(&server_share.id).is_some());
    assert!(lookup_share_link(&share.id).is_some());
    assert!(lookup_owner(&server_share.id).is_none());
    assert!(lookup_owner(&share.id).is_none());
    assert!(lookup_share_link(&server_slug).is_none());
    assert!(lookup_share_link(&file_slug).is_none());
}

#[test]
fn server_share_supports_code_grant_expiry_and_target_scope() {
    let dir = std::env::temp_dir().join(format!(
        "va-preview-test-server-share-{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("share.md");
    std::fs::write(&file, "share").unwrap();

    let before = Instant::now();
    let (server_slug, first) = ensure_server(42001, dir.clone(), "server".into(), None);
    let (_, file_share) = ensure_file(file, dir.clone(), "file".into());
    assert!(first.expires_at >= before + Duration::from_secs(SHARE_TTL_SECS));
    assert!(first.expires_at <= Instant::now() + Duration::from_secs(SHARE_TTL_SECS));

    let server_entry = lookup_share_link(&first.id).expect("Server share resolves");
    assert!(matches!(
        server_entry.target,
        PreviewTarget::Server { port: 42001 }
    ));
    let (verified, first_grant) =
        verify_share_code(&first.id, &first.code).expect("Server code verifies");
    assert!(matches!(
        verified.target,
        PreviewTarget::Server { port: 42001 }
    ));
    assert!(matches!(
        authorize_share_grant(&first.id, &first_grant)
            .expect("Server grant authorizes")
            .target,
        PreviewTarget::Server { port: 42001 }
    ));
    assert!(authorize_share_grant(&file_share.id, &first_grant).is_none());

    let id = canonical(&dir).join(":port:42001");
    SESSIONS
        .lock()
        .get_mut(&id)
        .and_then(|session| session.share.as_mut())
        .expect("Server share transaction exists")
        .expires_at = Instant::now() - Duration::from_secs(1);

    assert!(lookup_share_link(&first.id).is_none());
    assert!(authorize_share_grant(&first.id, &first_grant).is_none());
    let (rotated_slug, rotated) = ensure_server(42001, dir, "server".into(), None);
    assert_eq!(rotated_slug, server_slug);
    assert_ne!(rotated.id, first.id);
    assert_ne!(rotated.code, first.code);
    assert!(verify_share_code(&rotated.id, &rotated.code).is_ok());
}

#[test]
fn owner_conversation_binding_is_idempotent_and_cannot_be_retargeted() {
    let dir = std::env::temp_dir().join(format!(
        "va-preview-test-conversation-{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("review.md");
    std::fs::write(&file, "review").unwrap();
    let (slug, share) = ensure_file(file, dir, "review".into());
    let child_id = crate::workspace::threads::WorkspaceThreadId::from("wt_child");

    assert_eq!(owner_conversation_thread_id(&slug), None);
    assert_eq!(bind_owner_conversation(&slug, child_id.clone()), Ok(()));
    assert_eq!(bind_owner_conversation(&slug, child_id.clone()), Ok(()));
    assert_eq!(owner_conversation_thread_id(&slug), Some(child_id));
    assert_eq!(
        bind_owner_conversation(
            &slug,
            crate::workspace::threads::WorkspaceThreadId::from("wt_other")
        ),
        Err(PreviewConversationBindError::Conflict {
            existing_thread_id: crate::workspace::threads::WorkspaceThreadId::from("wt_child")
        })
    );
    assert_eq!(owner_conversation_thread_id(&share.id), None);
    assert_eq!(
        bind_owner_conversation(
            &share.id,
            crate::workspace::threads::WorkspaceThreadId::from("wt_share")
        ),
        Err(PreviewConversationBindError::NotFound)
    );
}

#[test]
fn owner_conversation_lifecycle_replaces_the_latest_child() {
    let dir = std::env::temp_dir().join(format!(
        "va-preview-test-conversation-lifecycle-{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("review.md");
    std::fs::write(&file, "review").unwrap();
    let (slug, _) = ensure_file(file, dir, "review".into());
    let first = crate::workspace::threads::WorkspaceThreadId::from("wt_first");
    let second = crate::workspace::threads::WorkspaceThreadId::from("wt_second");

    bind_owner_conversation(&slug, first.clone()).unwrap();
    replace_owner_conversation(&slug, second.clone()).unwrap();
    assert_eq!(owner_conversation_thread_id(&slug), Some(second));
}

#[test]
fn access_code_is_reusable_and_bound_to_its_share_link() {
    let dir = std::env::temp_dir().join(format!(
        "va-preview-test-access-code-{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let file_a = dir.join("a.md");
    let file_b = dir.join("b.md");
    std::fs::write(&file_a, "a").unwrap();
    std::fs::write(&file_b, "b").unwrap();
    let (_, share_a) = ensure_file(file_a, dir.clone(), "a".into());
    let (_, share_b) = ensure_file(file_b, dir, "b".into());

    let (_, grant_a1) =
        verify_share_code(&share_a.id, &share_a.code).expect("first viewer verifies");
    let (_, grant_a2) =
        verify_share_code(&share_a.id, &share_a.code).expect("second viewer verifies");
    assert_eq!(grant_a1, grant_a2, "one transaction owns one browser grant");
    assert_eq!(grant_a1.len(), 64);
    assert!(grant_a1.bytes().all(|byte| byte.is_ascii_hexdigit()));
    assert!(authorize_share_grant(&share_a.id, &grant_a1).is_some());

    assert!(matches!(
        verify_share_code("missing-share-id", &share_a.code),
        Err(ShareCodeError::NotFound)
    ));
    assert!(authorize_share_grant(&share_b.id, &grant_a1).is_none());
}

#[test]
fn failed_access_codes_are_rate_limited_per_share() {
    let dir = std::env::temp_dir().join(format!(
        "va-preview-test-rate-limit-{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("rate.md");
    std::fs::write(&file, "rate").unwrap();
    let (_, share) = ensure_file(file, dir, "rate".into());

    let wrong_code = if share.code == "999999" {
        "000000"
    } else {
        "999999"
    };
    for _ in 0..SHARE_CODE_ATTEMPT_BURST {
        assert!(matches!(
            verify_share_code(&share.id, wrong_code),
            Err(ShareCodeError::Invalid)
        ));
    }
    assert!(matches!(
        verify_share_code(&share.id, &share.code),
        Err(ShareCodeError::RateLimited { .. })
    ));

    let now = Instant::now();
    let mut sessions = SESSIONS.lock();
    let transaction = sessions
        .values_mut()
        .find_map(|session| {
            session
                .share
                .as_mut()
                .filter(|transaction| transaction.id == share.id)
        })
        .expect("share transaction exists");
    transaction.attempts_refilled_at = now - SHARE_ATTEMPT_REFILL;
    drop(sessions);
    assert!(verify_share_code(&share.id, &share.code).is_ok());
}

#[test]
fn expired_share_replaces_link_code_and_grant() {
    let dir =
        std::env::temp_dir().join(format!("va-preview-test-rotation-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("rotate.md");
    std::fs::write(&file, "rotate").unwrap();
    let (_, first) = ensure_file(file.clone(), dir.clone(), "rotate".into());
    let (_, first_grant) = verify_share_code(&first.id, &first.code).unwrap();

    let canonical_file = canonical(&file);
    let mut sessions = SESSIONS.lock();
    sessions
        .get_mut(&canonical_file)
        .and_then(|session| session.share.as_mut())
        .expect("share transaction exists")
        .expires_at = Instant::now() - Duration::from_secs(1);
    drop(sessions);

    let (_, rotated) = ensure_file(file, dir, "rotate".into());
    assert_ne!(rotated.id, first.id);
    assert_ne!(rotated.code, first.code);
    assert!(lookup_share_link(&first.id).is_none());
    assert!(authorize_share_grant(&rotated.id, &first_grant).is_none());
    assert!(matches!(
        verify_share_code(&first.id, &first.code),
        Err(ShareCodeError::NotFound)
    ));
    assert!(verify_share_code(&rotated.id, &rotated.code).is_ok());
}
