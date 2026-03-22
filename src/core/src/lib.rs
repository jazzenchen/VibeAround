//! VibeAround core: PTY, session registry, tunnels, IM, workspace. No UI, no HTTP.

pub mod agent;
pub mod config;
pub mod session_hub;
pub mod agent_manager;
pub mod channel_manager;
pub mod pty;
pub mod service;
pub mod session;
pub mod session_store;
pub mod tunnels;
pub mod workspace;
