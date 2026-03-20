//! Hub architecture: ChannelHub ↔ SessionHub ↔ AgentHub
//!
//! Three singletons connected via direct method calls (Arc<T>):
//!   ChannelHub — plugin process I/O, protocol parsing
//!   SessionHub — session lifecycle, per-session message queue, slash commands
//!   AgentHub   — agent process lifecycle, profile loading, CLI spawn/kill
//!
//! Shared event types live in this module.

pub mod agent_hub;
pub mod channel_hub;
pub mod session_hub;
pub mod types;
