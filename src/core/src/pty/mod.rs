//! PTY domain: runtime process layer, PTY session types, and PTY session manager.

pub mod runtime;
pub mod manager;
pub mod session;

pub use manager::{PtyAttachHandles, PtySessionCreated, PtySessionManager, PtySessionSummary};
pub use runtime::{list_tmux_sessions, spawn_pty, tmux_available, PtyBridge, PtyRunState, PtyTool, ResizeSender};
pub use session::{unix_now_secs, CircularBuffer, Registry, SessionContext, SessionId, SessionMetadata, LIVE_BROADCAST_CAP};
