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
//! - `share`     — one 10-minute File-preview transaction containing the
//!   public link ID, human access code, and browser grant.
//!   Server previews stay local-only and load from their own localhost origin.
//!
//! URL structure (all routes under `/va/`):
//!
//! - Owner: `/preview/u/{slug}`        — permanent for the daemon lifetime
//! - File share: `/preview/s/{share_id}` — access-code gate
//!
//! One `HashMap<PathBuf, PreviewSession>` backs everything. Lookups by
//! `slug` or `share_id` scan values — `n` is tiny (<20 typical).
//!
//! On daemon shutdown, [`shutdown_kill_all_ports`] kills a tracked `Server`
//! listener only while its PID and process start time still match registration.
//!
//! ## Module layout
//!
//! - [`types`] — public data model (`PreviewTarget`, `PreviewEntry`, …).
//! - [`store`] — internal session storage + slug/share-key generation.
//! - [`kill`]  — port-driven process-group SIGKILL helpers.
//! - [`lease`] — durable Server-listener projection for crash recovery.

mod kill;
mod lease;
mod store;
mod types;

use std::path::PathBuf;
use std::time::Instant;

pub use store::{SHARE_CODE_ATTEMPT_BURST, SHARE_CODE_LENGTH, SHARE_TTL_SECS};
pub use types::{
    PreviewConversationBindError, PreviewEntry, PreviewKind, PreviewShare, PreviewSnapshot,
    PreviewTarget, ShareCodeError,
};

use store::{
    canonical, entry_from, generate_share_code, generate_share_grant, generate_share_id,
    slug_from_path, ListenerProcess, PreviewSession, ShareTransaction, SESSIONS,
    SHARE_ATTEMPT_REFILL, SHARE_TTL,
};

// ---------------------------------------------------------------------------
// Public API — create / refresh
// ---------------------------------------------------------------------------

/// Ensure a local-only Server preview session exists for `(workspace, port)`.
/// Calling twice for the same `(workspace, port)` reuses the owner slug.
/// Different ports under the same workspace coexist as independent sessions.
pub fn ensure_server(
    port: u16,
    workspace: PathBuf,
    title: String,
    owner_session: Option<String>,
) -> String {
    let workspace = canonical(&workspace);
    let id = workspace.join(format!(":port:{port}"));
    let listener = kill::listener_process(port);
    let (slug, share) = ensure_session(
        id,
        workspace,
        title,
        PreviewTarget::Server { port },
        owner_session,
        listener,
    );
    debug_assert!(share.is_none());
    slug
}

/// Ensure a File preview session exists for `file`. Returns
/// `(owner_slug, share)`. Same file reuses a live share transaction.
pub fn ensure_file(file: PathBuf, workspace: PathBuf, title: String) -> (String, PreviewShare) {
    let file = canonical(&file);
    let workspace = canonical(&workspace);
    let (slug, share) = ensure_session(file, workspace, title, PreviewTarget::File, None, None);
    (slug, share.expect("File previews always receive a share"))
}

fn ensure_session(
    id: PathBuf,
    workspace: PathBuf,
    title: String,
    target: PreviewTarget,
    owner_session: Option<String>,
    listener: Option<ListenerProcess>,
) -> (String, Option<PreviewShare>) {
    let persist_server = matches!(target, PreviewTarget::Server { .. });
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
            listener,
            slug: slug.clone(),
            share: None,
            conversation_thread_id: None,
            owner_session: owner_session.clone(),
            created_at: now,
        });

    // Refresh mutable fields on every call.
    session.workspace = workspace;
    session.title = title;
    session.target = target;
    session.listener = match session.target {
        PreviewTarget::Server { .. } => listener.or(session.listener),
        PreviewTarget::File => None,
    };
    if owner_session.is_some() {
        session.owner_session = owner_session;
    }

    // Only File previews are shareable. Reuse a live transaction or replace
    // the entire expired transaction so an old shared URL cannot revive.
    let share = if target_is_shareable(&session.target) {
        if session
            .share
            .as_ref()
            .is_none_or(|share| share.expires_at <= now)
        {
            let previous_code = session.share.as_ref().map(|share| share.code.as_str());
            session.share = Some(ShareTransaction {
                id: generate_share_id(),
                code: generate_share_code(previous_code),
                grant: generate_share_grant(),
                expires_at: now + SHARE_TTL,
                attempt_tokens: SHARE_CODE_ATTEMPT_BURST,
                attempts_refilled_at: now,
            });
        }
        session.share.as_ref().map(|share| PreviewShare {
            id: share.id.clone(),
            code: share.code.clone(),
            expires_at: share.expires_at,
        })
    } else {
        session.share = None;
        None
    };

    if persist_server {
        lease::persist_active(&sessions);
    }
    (slug, share)
}

fn target_is_shareable(target: &PreviewTarget) -> bool {
    matches!(target, PreviewTarget::File)
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

/// Bind an owner Preview to one conversation task. Repeating the same bind is
/// idempotent; rebinding an existing Preview to a different task is rejected.
pub fn bind_owner_conversation(
    slug: &str,
    thread_id: crate::workspace::threads::WorkspaceThreadId,
) -> Result<(), PreviewConversationBindError> {
    let mut sessions = SESSIONS.lock();
    let session = sessions
        .values_mut()
        .find(|session| session.slug == slug)
        .ok_or(PreviewConversationBindError::NotFound)?;
    match session.conversation_thread_id.as_ref() {
        Some(existing) if existing != &thread_id => Err(PreviewConversationBindError::Conflict {
            existing_thread_id: existing.clone(),
        }),
        Some(_) => Ok(()),
        None => {
            session.conversation_thread_id = Some(thread_id);
            Ok(())
        }
    }
}

/// Replace the latest conversation for an existing owner Preview. A closed
/// child remains here until the next message creates its successor.
pub fn replace_owner_conversation(
    slug: &str,
    thread_id: crate::workspace::threads::WorkspaceThreadId,
) -> Result<(), PreviewConversationBindError> {
    let mut sessions = SESSIONS.lock();
    let session = sessions
        .values_mut()
        .find(|session| session.slug == slug)
        .ok_or(PreviewConversationBindError::NotFound)?;
    session.conversation_thread_id = Some(thread_id);
    Ok(())
}

/// Return the conversation task bound to this owner slug. Public share IDs do
/// not resolve through this owner-only lookup.
pub fn owner_conversation_thread_id(
    slug: &str,
) -> Option<crate::workspace::threads::WorkspaceThreadId> {
    SESSIONS
        .lock()
        .values()
        .find(|session| session.slug == slug)
        .and_then(|session| session.conversation_thread_id.clone())
}

/// Look up an active File share by its opaque public link ID.
pub fn lookup_share_link(id: &str) -> Option<PreviewEntry> {
    let sessions = SESSIONS.lock();
    let now = Instant::now();
    sessions
        .values()
        .find(|s| {
            target_is_shareable(&s.target)
                && s.share
                    .as_ref()
                    .is_some_and(|share| share.id == id && share.expires_at > now)
        })
        .map(|s| entry_from(s, s.share.as_ref().map(|share| share.expires_at)))
}

/// Verify a human access code and return the high-entropy browser grant.
/// Codes are reusable until the shared transaction expires.
pub fn verify_share_code(id: &str, code: &str) -> Result<(PreviewEntry, String), ShareCodeError> {
    let now = Instant::now();
    let mut sessions = SESSIONS.lock();
    let session = sessions
        .values_mut()
        .find(|session| {
            target_is_shareable(&session.target)
                && session
                    .share
                    .as_ref()
                    .is_some_and(|share| share.id == id && share.expires_at > now)
        })
        .ok_or(ShareCodeError::NotFound)?;

    let share = session.share.as_mut().expect("matched an active share");
    refill_share_attempts(share, now);
    if share.attempt_tokens == 0 {
        return Err(ShareCodeError::RateLimited {
            retry_after_secs: retry_after_secs(share, now),
        });
    }
    if share.code != code {
        share.attempt_tokens -= 1;
        return Err(ShareCodeError::Invalid);
    }

    let expires_at = share.expires_at;
    let grant = share.grant.clone();
    Ok((entry_from(session, Some(expires_at)), grant))
}

/// Revalidate the long browser grant on every shared Markdown request.
pub fn authorize_share_grant(id: &str, grant: &str) -> Option<PreviewEntry> {
    let sessions = SESSIONS.lock();
    let now = Instant::now();
    sessions
        .values()
        .find(|session| {
            target_is_shareable(&session.target)
                && session.share.as_ref().is_some_and(|share| {
                    share.id == id && share.grant == grant && share.expires_at > now
                })
        })
        .map(|session| {
            entry_from(
                session,
                session.share.as_ref().map(|share| share.expires_at),
            )
        })
}

fn refill_share_attempts(share: &mut ShareTransaction, now: Instant) {
    let elapsed = now.saturating_duration_since(share.attempts_refilled_at);
    let refill_count = elapsed.as_secs() / SHARE_ATTEMPT_REFILL.as_secs();
    if refill_count == 0 {
        return;
    }
    share.attempt_tokens = share
        .attempt_tokens
        .saturating_add(refill_count.min(u8::MAX as u64) as u8)
        .min(SHARE_CODE_ATTEMPT_BURST);
    share.attempts_refilled_at += SHARE_ATTEMPT_REFILL * refill_count as u32;
}

fn retry_after_secs(share: &ShareTransaction, now: Instant) -> u64 {
    SHARE_ATTEMPT_REFILL
        .saturating_sub(now.saturating_duration_since(share.attempts_refilled_at))
        .as_secs()
        .max(1)
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
            let active_share = s
                .share
                .as_ref()
                .filter(|share| target_is_shareable(&s.target) && share.expires_at > now_inst);
            let (share_id, share_code, share_expires_at_ms) = match active_share {
                Some(share) => (
                    Some(share.id.clone()),
                    Some(share.code.clone()),
                    Some(instant_to_unix_ms(share.expires_at, now_inst, now_sys)),
                ),
                None => (None, None, None),
            };
            let created_at_ms = instant_to_unix_ms(s.created_at, now_inst, now_sys);
            PreviewSnapshot {
                slug: s.slug.clone(),
                id: s.id.clone(),
                workspace: s.workspace.clone(),
                title: s.title.clone(),
                kind,
                port,
                share_id,
                share_code,
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
    let to_remove: Vec<(PathBuf, Option<(u16, ListenerProcess)>)> = {
        let sessions = SESSIONS.lock();
        sessions
            .iter()
            .filter(|(_, s)| s.owner_session.as_deref() == Some(session_id))
            .map(|(k, s)| {
                let listener = match (&s.target, s.listener) {
                    (PreviewTarget::Server { port }, Some(listener)) => Some((*port, listener)),
                    _ => None,
                };
                (k.clone(), listener)
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

    for (_, listener) in to_remove {
        if let Some((port, listener)) = listener {
            // Only kill if no remaining session uses this port.
            let still_used = SESSIONS
                .lock()
                .values()
                .any(|s| matches!(s.target, PreviewTarget::Server { port: pp } if pp == port));
            if !still_used {
                kill::kill_registered_listener(port, listener);
            }
        }
    }

    lease::persist_active(&SESSIONS.lock());
}

/// Close a single preview session. A Server listener is killed only when its
/// current PID and process start time still match registration.
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
        let removed = match key {
            Some(k) => sessions.remove(&k),
            None => None,
        };
        removed
    };

    let Some(session) = removed else {
        return false;
    };

    // Kill the port if Server — best effort.
    let was_server = matches!(&session.target, PreviewTarget::Server { .. });
    if let (PreviewTarget::Server { port }, Some(listener)) = (session.target, session.listener) {
        kill::kill_registered_listener(port, listener);
    }
    if was_server {
        lease::persist_active(&SESSIONS.lock());
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

/// Restore Server previews left by an unclean daemon exit. A lease is restored
/// only when the current listener has the same PID and process start time.
pub fn reconcile_server_leases() {
    let path = lease::activate_path();
    let mut sessions = SESSIONS.lock();
    match lease::reconcile_at(path, &mut sessions, kill::listener_process) {
        Ok(0) => {}
        Ok(count) => tracing::info!("[preview] restored {} Server preview lease(s)", count),
        Err(error) => {
            tracing::warn!(path = ?path, error = %error, "failed to reconcile Server preview leases");
            lease::persist_active(&sessions);
        }
    }
}

/// Kill each registered Server listener whose PID and start time still match.
/// Best-effort; failures are logged. Clears the session map.
pub fn shutdown_kill_all_ports() {
    let listeners: Vec<(u16, ListenerProcess)> = SESSIONS
        .lock()
        .values()
        .filter_map(|session| match (&session.target, session.listener) {
            (PreviewTarget::Server { port }, Some(listener)) => Some((*port, listener)),
            _ => None,
        })
        .collect();
    if !listeners.is_empty() {
        tracing::info!(
            "[preview] shutdown: checking {} registered dev-server listener(s)",
            listeners.len()
        );
        for (port, listener) in listeners {
            kill::kill_registered_listener(port, listener);
        }
    }
    let mut sessions = SESSIONS.lock();
    sessions.clear();
    lease::persist_active(&sessions);
}

#[cfg(test)]
#[path = "tests.rs"]
mod tests;
