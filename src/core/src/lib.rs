//! VibeAround core: PTY, session registry, tunnels, IM, workspace. No UI, no HTTP.

pub mod agent;
pub mod channels;
pub mod config;
pub mod hub;
pub mod message_hub {} // deleted — replaced by hub architecture
pub mod pty;
pub mod service;
pub mod session;
pub mod session_store;
pub mod tunnels;
pub mod workspace;
