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

/// Legacy alias kept for callers that still use `PreviewKind`.
/// New code should prefer [`PreviewTarget`].
pub type PreviewKind = PreviewTarget;

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
    /// When the current share key expires. Owner access lives with the
    /// preview session and therefore has no expiry here.
    pub expires_at: Option<Instant>,
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
    pub share_key: Option<String>,
    /// Unix millis; `null` for owner-only sessions (no share key generated).
    pub share_expires_at_ms: Option<u64>,
    pub created_at_ms: u64,
}
