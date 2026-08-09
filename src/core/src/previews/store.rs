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

use crate::workspace::threads::WorkspaceThreadId;

use super::types::{PreviewEntry, PreviewTarget};

#[derive(Debug, Clone)]
pub(super) struct PreviewSession {
    pub(super) id: PathBuf,
    pub(super) workspace: PathBuf,
    pub(super) title: String,
    pub(super) target: PreviewTarget,
    pub(super) slug: String,
    pub(super) share: Option<ShareTransaction>,
    pub(super) conversation_thread_id: Option<WorkspaceThreadId>,
    /// Agent session ID that registered this preview. Used for cleanup
    /// when the session closes. `None` if the agent didn't provide it.
    pub(super) owner_session: Option<String>,
    pub(super) created_at: Instant,
}

#[derive(Debug, Clone)]
pub(super) struct ShareTransaction {
    /// Opaque public identifier carried by the share URL.
    pub(super) id: String,
    /// Human-entered reusable access code.
    pub(super) code: String,
    /// High-entropy browser credential issued after code verification.
    pub(super) grant: String,
    pub(super) expires_at: Instant,
    pub(super) attempt_tokens: u8,
    pub(super) attempts_refilled_at: Instant,
}

/// Lifetime of one reusable access-code transaction, in seconds.
pub const SHARE_TTL_SECS: u64 = 600;
pub const SHARE_CODE_LENGTH: usize = 6;
pub const SHARE_CODE_ATTEMPT_BURST: u8 = 10;
pub(super) const SHARE_TTL: Duration = Duration::from_secs(SHARE_TTL_SECS);
pub(super) const SHARE_ATTEMPT_REFILL: Duration = Duration::from_secs(6);

const ACCESS_CODE_SPACE: u32 = 10_u32.pow(SHARE_CODE_LENGTH as u32);

pub(super) static SESSIONS: LazyLock<Mutex<HashMap<PathBuf, PreviewSession>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

pub(super) fn generate_share_id() -> String {
    uuid::Uuid::new_v4().simple().to_string()
}

/// Random six-digit access code, including leading zeroes.
pub(super) fn generate_share_code(previous: Option<&str>) -> String {
    let mut rng = OsRng;
    loop {
        let code = format!(
            "{:0width$}",
            rng.gen_range(0..ACCESS_CODE_SPACE),
            width = SHARE_CODE_LENGTH
        );
        if previous != Some(code.as_str()) {
            return code;
        }
    }
}

pub(super) fn generate_share_grant() -> String {
    use std::fmt::Write;

    let mut bytes = [0_u8; 32];
    rand::RngCore::fill_bytes(&mut OsRng, &mut bytes);
    let mut grant = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        write!(&mut grant, "{byte:02x}").expect("writing to a String cannot fail");
    }
    grant
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
