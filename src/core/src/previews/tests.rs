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

    let slug_a = ensure_server(3000, path.clone(), "t".into(), None);
    let slug_b = ensure_server(3000, path.clone(), "t".into(), None);
    assert_eq!(slug_a, slug_b);

    let snapshot = list_snapshots()
        .into_iter()
        .find(|preview| preview.slug == slug_a)
        .expect("server preview is listed");
    assert!(snapshot.share_id.is_none());
    assert!(snapshot.share_code.is_none());
    assert!(snapshot.share_expires_at_ms.is_none());
}

#[test]
fn ensure_server_keeps_different_ports_separate() {
    let path = std::env::temp_dir().join("va-preview-test-multiport");
    std::fs::create_dir_all(&path).unwrap();

    let slug_a = ensure_server(3456, path.clone(), "liquid".into(), None);
    let slug_b = ensure_server(5000, path.clone(), "python".into(), None);
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
fn ensure_file_is_idempotent_and_independent_of_server() {
    let dir = std::env::temp_dir().join("va-preview-test-file");
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("README.md");
    std::fs::write(&file, "hi").unwrap();

    let srv_slug = ensure_server(4000, dir.clone(), "srv".into(), None);
    let (file_slug_a, file_share_a) = ensure_file(file.clone(), dir.clone(), "md".into());
    let (file_slug_b, file_share_b) = ensure_file(file.clone(), dir.clone(), "md".into());

    assert_ne!(srv_slug, file_slug_a, "server and file share different ids");
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

    let server_slug = ensure_server(4100, dir.clone(), "server".into(), None);
    let (file_slug, share) = ensure_file(file, dir, "file".into());

    assert!(lookup_owner(&server_slug).is_some());
    assert!(lookup_owner(&file_slug).is_some());
    assert!(lookup_share_link(&share.id).is_some());
    assert!(lookup_owner(&share.id).is_none());
    assert!(lookup_share_link(&server_slug).is_none());
    assert!(lookup_share_link(&file_slug).is_none());
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
