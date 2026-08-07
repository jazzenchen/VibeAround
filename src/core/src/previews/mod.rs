//! Preview sessions — one per (workspace, port) for server or file path
//! for file.
//!
//! A unified `PreviewSession` models both live dev-server previews and
//! static file previews (e.g. rendered markdown). Each session has:
//!
//! - `id`        — the canonical path that identifies this preview
//!   (workspace dir + synthetic `:port:N` segment for
//!   `Server`; file path for `File`).
//! - `target`    — what to serve: `Server { port }` or `File`.
//! - `slug`      — stable, readable URL segment derived from `id`.
//!   Full-path-based (slashes → `-`), so slugs are globally
//!   unique and collision-proof.
//! - `share_key` — ephemeral random token with 10-min TTL. Regenerated
//!   once the previous key expires.
//!
//! URL structure (all routes under `/va/`):
//!
//! - Owner: `/preview/u/{slug}`        — permanent for the daemon lifetime
//! - Share: `/preview/s/{share_key}`   — 10-minute rotating token
//!
//! One `HashMap<PathBuf, PreviewSession>` backs everything. Lookups by
//! `slug` or `share_key` scan values — `n` is tiny (<20 typical).
//!
//! On daemon shutdown, [`shutdown_kill_all_ports`] SIGKILLs any process
//! listening on a tracked `Server` port so dev servers don't leak.
//!
//! ## Module layout
//!
//! - [`types`] — public data model (`PreviewTarget`, `PreviewEntry`, …).
//! - [`store`] — internal session storage + slug/share-key generation.
//! - [`kill`]  — port-driven process-group SIGKILL helpers.

mod kill;
mod store;
mod types;

use std::path::PathBuf;
use std::time::Instant;

pub use store::SHARE_TTL_SECS;
pub use types::{PreviewEntry, PreviewKind, PreviewSnapshot, PreviewTarget};

use store::{
    canonical, entry_from, generate_share_key, slug_from_path, PreviewSession, SESSIONS, SHARE_TTL,
};

// ---------------------------------------------------------------------------
// Public API — create / refresh
// ---------------------------------------------------------------------------

/// Ensure a Server preview session exists for `(workspace, port)`.
/// Returns `(owner_slug, share_key)`. Calling twice for the same
/// `(workspace, port)` reuses the owner slug; the share key is refreshed
/// if expired. Different ports under the same workspace coexist as
/// independent sessions.
pub fn ensure_server(
    port: u16,
    workspace: PathBuf,
    title: String,
    owner_session: Option<String>,
) -> (String, String) {
    let workspace = canonical(&workspace);
    let id = workspace.join(format!(":port:{port}"));
    ensure_session(
        id,
        workspace,
        title,
        PreviewTarget::Server { port },
        owner_session,
    )
}

/// Ensure a File preview session exists for `file`. Returns
/// `(owner_slug, share_key)`. Same file → same owner slug.
pub fn ensure_file(file: PathBuf, workspace: PathBuf, title: String) -> (String, String) {
    let file = canonical(&file);
    let workspace = canonical(&workspace);
    ensure_session(file, workspace, title, PreviewTarget::File, None)
}

fn ensure_session(
    id: PathBuf,
    workspace: PathBuf,
    title: String,
    target: PreviewTarget,
    owner_session: Option<String>,
) -> (String, String) {
    let slug = slug_from_path(&id);
    let now = Instant::now();

    let mut sessions = SESSIONS.lock();
    let session = sessions
        .entry(id.clone())
        .or_insert_with(|| PreviewSession {
            id: id.clone(),
            workspace: workspace.clone(),
            title: title.clone(),
            target: target.clone(),
            slug: slug.clone(),
            share_key: None,
            share_expires_at: None,
            owner_session: owner_session.clone(),
            created_at: now,
        });

    // Refresh mutable fields on every call.
    session.workspace = workspace;
    session.title = title;
    session.target = target;
    if owner_session.is_some() {
        session.owner_session = owner_session;
    }

    // Reuse share key if still valid; otherwise rotate.
    let share_key = match (&session.share_key, session.share_expires_at) {
        (Some(k), Some(exp)) if exp > now => k.clone(),
        _ => {
            let k = generate_share_key();
            session.share_key = Some(k.clone());
            session.share_expires_at = Some(now + SHARE_TTL);
            k
        }
    };

    (slug, share_key)
}

// ---------------------------------------------------------------------------
// Public API — lookup
// ---------------------------------------------------------------------------

/// Look up a session by its permanent owner slug.
pub fn lookup_owner(slug: &str) -> Option<PreviewEntry> {
    let sessions = SESSIONS.lock();
    sessions
        .values()
        .find(|s| s.slug == slug)
        .map(|s| entry_from(s, None))
}

/// Look up a session by its ephemeral share key. Expired keys return `None`.
pub fn lookup_share(key: &str) -> Option<PreviewEntry> {
    let sessions = SESSIONS.lock();
    let now = Instant::now();
    sessions
        .values()
        .find(|s| match (&s.share_key, s.share_expires_at) {
            (Some(k), Some(exp)) => k == key && exp > now,
            _ => false,
        })
        .map(|s| entry_from(s, s.share_expires_at))
}

// ---------------------------------------------------------------------------
// Listing + removal
// ---------------------------------------------------------------------------

/// Snapshot every live session for UI display.
pub fn list_snapshots() -> Vec<PreviewSnapshot> {
    let sessions = SESSIONS.lock();
    let now_inst = Instant::now();
    let now_sys = std::time::SystemTime::now();

    sessions
        .values()
        .map(|s| {
            let (kind, port) = match s.target {
                PreviewTarget::Server { port } => ("server", Some(port)),
                PreviewTarget::File => ("file", None),
            };
            let share_expires_at_ms = match (&s.share_key, s.share_expires_at) {
                (Some(_), Some(exp)) if exp > now_inst => {
                    Some(instant_to_unix_ms(exp, now_inst, now_sys))
                }
                _ => None,
            };
            let share_key = match (&s.share_key, s.share_expires_at) {
                (Some(k), Some(exp)) if exp > now_inst => Some(k.clone()),
                _ => None,
            };
            let created_at_ms = instant_to_unix_ms(s.created_at, now_inst, now_sys);
            PreviewSnapshot {
                slug: s.slug.clone(),
                id: s.id.clone(),
                workspace: s.workspace.clone(),
                title: s.title.clone(),
                kind,
                port,
                share_key,
                share_expires_at_ms,
                created_at_ms,
            }
        })
        .collect()
}

/// Convert an `Instant` to unix-epoch milliseconds, using `now` as the pivot.
fn instant_to_unix_ms(point: Instant, now_inst: Instant, now_sys: std::time::SystemTime) -> u64 {
    let unix_now_ms = now_sys
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;

    if point >= now_inst {
        unix_now_ms + (point - now_inst).as_millis() as u64
    } else {
        unix_now_ms.saturating_sub((now_inst - point).as_millis() as u64)
    }
}

/// Kill all preview sessions owned by a specific agent session.
/// Called from pod.close() when a route is shut down. Kills Server
/// ports (if not shared) and removes matching sessions.
pub fn kill_by_session(session_id: &str) {
    let to_remove: Vec<(PathBuf, Option<u16>)> = {
        let sessions = SESSIONS.lock();
        sessions
            .iter()
            .filter(|(_, s)| s.owner_session.as_deref() == Some(session_id))
            .map(|(k, s)| {
                let port = match s.target {
                    PreviewTarget::Server { port } => Some(port),
                    PreviewTarget::File => None,
                };
                (k.clone(), port)
            })
            .collect()
    };

    if to_remove.is_empty() {
        return;
    }

    tracing::info!(
        "[preview] kill_by_session session={} count={}",
        session_id,
        to_remove.len()
    );

    let mut sessions = SESSIONS.lock();
    for (key, _port) in &to_remove {
        sessions.remove(key);
    }
    drop(sessions); // release lock before killing

    for (_, port) in to_remove {
        if let Some(p) = port {
            // Only kill if no remaining session uses this port.
            let still_used = SESSIONS
                .lock()
                .values()
                .any(|s| matches!(s.target, PreviewTarget::Server { port: pp } if pp == p));
            if !still_used {
                kill::kill_port(p);
            }
        }
    }
}

/// Close a single preview session: if it's a Server target, SIGKILL the
/// process currently listening on its port (via `lsof` + `sysinfo::kill`).
/// Then remove the session from the store. Returns `true` when a matching
/// slug was found and removed.
pub fn delete_session(slug: &str) -> bool {
    // Find and remove the matching session.
    let removed = {
        let mut sessions = SESSIONS.lock();
        let key = sessions
            .iter()
            .find(|(_, s)| s.slug == slug)
            .map(|(k, _)| k.clone());
        match key {
            Some(k) => sessions.remove(&k),
            None => None,
        }
    };

    let Some(session) = removed else {
        return false;
    };

    // Kill the port if Server — best effort.
    if let PreviewTarget::Server { port } = session.target {
        kill::kill_port(port);
    }
    true
}

// ---------------------------------------------------------------------------
// Shutdown — kill tracked dev-server ports
// ---------------------------------------------------------------------------

/// Snapshot of ports held by Server-kind sessions.
pub fn tracked_ports() -> Vec<u16> {
    SESSIONS
        .lock()
        .values()
        .filter_map(|s| match s.target {
            PreviewTarget::Server { port } => Some(port),
            PreviewTarget::File => None,
        })
        .collect()
}

/// Send SIGKILL to every process listening on a tracked Server port.
/// Best-effort; failures are logged. Clears the session map.
pub fn shutdown_kill_all_ports() {
    let ports = tracked_ports();
    if !ports.is_empty() {
        tracing::info!(
            "[preview] shutdown: killing dev servers on ports {:?}",
            ports
        );
        kill::kill_pids_on_ports(&ports);
    }
    SESSIONS.lock().clear();
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn slug_from_full_path_is_stable_and_unique() {
        assert_eq!(slug_from_path(Path::new("/tmp/my-app")), "tmp-my-app");
        assert_eq!(
            slug_from_path(Path::new("/tmp/my-app/README.md")),
            "tmp-my-app-readme-md"
        );
        // Two different paths never produce the same slug.
        assert_ne!(
            slug_from_path(Path::new("/a/readme.md")),
            slug_from_path(Path::new("/b/readme.md")),
        );
    }

    #[test]
    fn ensure_server_is_idempotent() {
        let path = std::env::temp_dir().join("va-preview-test-server");
        std::fs::create_dir_all(&path).unwrap();

        let (slug_a, share_a) = ensure_server(3000, path.clone(), "t".into(), None);
        let (slug_b, share_b) = ensure_server(3000, path.clone(), "t".into(), None);
        assert_eq!(slug_a, slug_b);
        assert_eq!(share_a, share_b);
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
    fn ensure_file_is_idempotent_and_independent_of_server() {
        let dir = std::env::temp_dir().join("va-preview-test-file");
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("README.md");
        std::fs::write(&file, "hi").unwrap();

        let (srv_slug, _) = ensure_server(4000, dir.clone(), "srv".into(), None);
        let (file_slug_a, file_share_a) = ensure_file(file.clone(), dir.clone(), "md".into());
        let (file_slug_b, file_share_b) = ensure_file(file.clone(), dir.clone(), "md".into());

        assert_ne!(srv_slug, file_slug_a, "server and file share different ids");
        assert_eq!(file_slug_a, file_slug_b);
        assert_eq!(file_share_a, file_share_b);
    }

    #[test]
    fn lookups_preserve_owner_and_share_boundaries() {
        let path = std::env::temp_dir().join("va-preview-test-lookup");
        std::fs::create_dir_all(&path).unwrap();

        let (slug, share) = ensure_server(4100, path.clone(), "x".into(), None);
        assert!(lookup_owner(&slug).is_some());
        assert!(lookup_share(&share).is_some());
        assert!(lookup_owner(&share).is_none());
        assert!(lookup_share(&slug).is_none());
    }
}
