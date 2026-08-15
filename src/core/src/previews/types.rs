//! Public preview data model.

use std::path::PathBuf;
use std::time::Instant;

/// What the preview serves.
#[derive(Debug, Clone)]
pub enum PreviewTarget {
    /// Reverse proxy to a running local dev server on `port`.
    Server { port: u16 },
    /// Render a file directly (e.g. markdown).
    File,
}

/// Public view of a preview session, returned from lookups.
#[derive(Debug, Clone)]
pub struct PreviewEntry {
    /// Identity of the preview (workspace dir or file path).
    pub id: PathBuf,
    /// Containing workspace (== `id` for `Server`; parent dir for `File`).
    pub workspace: PathBuf,
    /// Human-readable display name.
    pub title: String,
    /// What to serve.
    pub target: PreviewTarget,
    /// When the session was created.
    pub created_at: Instant,
    /// When the current share transaction expires. Owner access lives with
    /// the preview session and therefore has no expiry here.
    pub expires_at: Option<Instant>,
}

/// Public share details returned to the owner who created a preview.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreviewShare {
    /// Opaque identifier carried by the public URL.
    pub id: String,
    /// Six-digit access code entered by viewers.
    pub code: String,
    /// Shared deadline for the link, access code, and browser grant.
    pub expires_at: Instant,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShareCodeError {
    NotFound,
    Invalid,
    RateLimited { retry_after_secs: u64 },
}

/// Serializable snapshot of a session for API responses.
#[derive(Debug, Clone, serde::Serialize)]
pub struct PreviewSnapshot {
    pub slug: String,
    pub id: PathBuf,
    pub workspace: PathBuf,
    pub title: String,
    /// Kind tag + port (for Server previews).
    pub kind: &'static str,
    pub port: Option<u16>,
    pub share_id: Option<String>,
    pub share_code: Option<String>,
    /// Unix millis; `null` after the in-memory share transaction expires.
    pub share_expires_at_ms: Option<u64>,
    pub created_at_ms: u64,
}
