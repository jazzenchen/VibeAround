//! IM (instant messaging) adapters.
//! All channels run as external plugin processes (stdio JSON-RPC).
//! Slash commands are handled in worker.rs, dispatched by the worker loop.

pub mod channels;
pub mod daemon;
pub mod log;
pub mod message_hub;
pub mod session_store;
pub mod spec;
pub mod transport;
// worker is deprecated — MessageHub replaces it. Kept for reference during Phase 2.
// pub mod worker;

/// Re-export plugin channel.
pub use channels::plugin;
/// Channel kind for management and dispatch.
pub use spec::ImChannelKind;
