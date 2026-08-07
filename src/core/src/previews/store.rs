//! Internal session storage + slug/key generation.
//!
//! Private to the `preview_entries` module. Exposes only `pub(super)`
//! helpers so `mod.rs` can implement the public API on top.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use rand::rngs::OsRng;
use rand::Rng;

use super::types::{PreviewEntry, PreviewTarget};

#[derive(Debug, Clone)]
pub(super) struct PreviewSession {
    pub(super) id: PathBuf,
    pub(super) workspace: PathBuf,
    pub(super) title: String,
    pub(super) target: PreviewTarget,
    pub(super) slug: String,
    pub(super) share_key: Option<String>,
    pub(super) share_expires_at: Option<Instant>,
    /// Agent session ID that registered this preview. Used for cleanup
    /// when the session closes. `None` if the agent didn't provide it.
    pub(super) owner_session: Option<String>,
    pub(super) created_at: Instant,
}

/// TTL for `/s/{key}` preview share links, in seconds.
pub const SHARE_TTL_SECS: u64 = 600;
pub(super) const SHARE_TTL: Duration = Duration::from_secs(SHARE_TTL_SECS);

/// Alphabet for random share keys: uppercase + digits, with ambiguous
/// I/O/0/1 removed.
const CHARSET: &[u8] = b"ABCDEFGHJKLMNPQRSTUVWXYZ23456789";

pub(super) static SESSIONS: LazyLock<Mutex<HashMap<PathBuf, PreviewSession>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// Random 8-char share key (for `/s/{key}` URLs).
pub(super) fn generate_share_key() -> String {
    let mut rng = OsRng;
    (0..8)
        .map(|_| CHARSET[rng.gen_range(0..CHARSET.len())] as char)
        .collect()
}

/// Derive a stable, collision-free owner slug from a full path.
///
/// Strategy: lowercase the path, replace every non-alphanumeric character
/// with `-`, and collapse repeated dashes. Because the full path is
/// unique per session, two sessions can never share a slug.
///
/// Examples:
///
/// - `/Users/foo/my-app`              → `users-foo-my-app`
/// - `/Users/foo/my-app/README.md`    → `users-foo-my-app-readme-md`
pub(super) fn slug_from_path(path: &Path) -> String {
    let raw = path.to_string_lossy();
    let mut out = String::with_capacity(raw.len());
    let mut last_dash = true; // drops leading '-'
    for c in raw.chars() {
        if c.is_ascii_alphanumeric() {
            out.extend(c.to_lowercase());
            last_dash = false;
        } else if !last_dash {
            out.push('-');
            last_dash = true;
        }
    }
    let trimmed = out.trim_matches('-').to_string();
    if trimmed.is_empty() {
        "preview".to_string()
    } else {
        trimmed
    }
}

pub(super) fn canonical(p: &Path) -> PathBuf {
    p.canonicalize().unwrap_or_else(|_| p.to_path_buf())
}

pub(super) fn entry_from(session: &PreviewSession, expires_at: Option<Instant>) -> PreviewEntry {
    PreviewEntry {
        id: session.id.clone(),
        workspace: session.workspace.clone(),
        title: session.title.clone(),
        target: session.target.clone(),
        created_at: session.created_at,
        expires_at,
    }
}
