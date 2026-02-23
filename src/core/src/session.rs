//! Session registry: persistent PTY sessions keyed by SessionId.
//! Each session has a ghost reader (see web_server), circular scrollback buffer, and broadcast for live output.

use crate::pty::{PtyBridge, PtyRunState, PtyTool, ResizeSender};
use dashmap::DashMap;
use std::sync::Arc;
use tokio::sync::broadcast;
use bytes::Bytes;

/// Unique session identifier (UUID v4). Used in API and WS query.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(transparent)]
pub struct SessionId(pub uuid::Uuid);

impl SessionId {
    pub fn new() -> Self {
        Self(uuid::Uuid::new_v4())
    }
}

impl std::fmt::Display for SessionId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

/// Metadata for a session (creation time, project path, tool). Returned in list/create API.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SessionMetadata {
    pub created_at: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project_path: Option<String>,
    pub tool: PtyTool,
}

/// Fixed-capacity circular scrollback buffer (bytes). New data appends; when over capacity, oldest bytes are dropped.
const SCROLLBACK_CAP_BYTES: usize = 2 * 1024 * 1024; // 2 MiB

pub struct CircularBuffer {
    data: std::sync::Mutex<Vec<u8>>,
    cap: usize,
}

impl CircularBuffer {
    pub fn new() -> Self {
        Self {
            data: std::sync::Mutex::new(Vec::new()),
            cap: SCROLLBACK_CAP_BYTES,
        }
    }

    /// Append bytes; if over capacity, drop oldest.
    pub fn push(&self, bytes: &[u8]) {
        let mut g = self.data.lock().expect("buffer mutex");
        g.extend_from_slice(bytes);
        if g.len() > self.cap {
            let excess = g.len() - self.cap;
            g.drain(..excess);
        }
    }

    /// Return a copy of current buffer contents (for Dump Buffer on new subscriber).
    pub fn dump(&self) -> Vec<u8> {
        let g = self.data.lock().expect("buffer mutex");
        g.clone()
    }
}

/// Live output broadcast capacity (number of messages to buffer per subscriber).
pub const LIVE_BROADCAST_CAP: usize = 256;

/// One PTY session: bridge, resize, state, metadata, scrollback buffer, and live broadcast sender.
/// The ghost reader (spawned when the session is created) feeds the buffer and broadcast.
pub struct SessionContext {
    pub bridge: PtyBridge,
    pub resize_tx: ResizeSender,
    /// Current run state (Running/Exited). Updated by a task that receives from spawn_pty's state_rx.
    pub state: Arc<std::sync::RwLock<PtyRunState>>,
    pub metadata: SessionMetadata,
    pub buffer: Arc<CircularBuffer>,
    /// Sender for live PTY output. Subscribers receive after connecting (after they get Dump Buffer).
    pub live_tx: broadcast::Sender<Bytes>,
}

impl SessionContext {
    /// Send bytes to all current subscribers (and push to buffer). Called by ghost reader.
    #[allow(dead_code)]
    pub fn broadcast_output(&self, bytes: Bytes) {
        self.buffer.push(bytes.as_ref());
        let _ = self.live_tx.send(bytes);
    }
}

/// Global registry of all active sessions. Injected into routes (e.g. axum::Extension).
pub type Registry = Arc<DashMap<SessionId, SessionContext>>;

/// Unix timestamp for "now" (seconds).
pub fn unix_now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
