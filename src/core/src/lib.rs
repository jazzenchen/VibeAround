//! VibeAround core: conversation manager, agents, channels, PTY, tunnels, workspace. No UI, no HTTP.

pub mod agent;
pub mod agent_availability;
pub mod agent_detection;
pub mod agent_state;
pub mod archive;
pub mod auth;
pub mod channels;
pub mod config;
pub mod file_replace;
pub mod launch_sessions;
pub mod logging;
pub mod plugins;
pub mod previews;
pub mod process;
pub mod profiles;
pub mod pty;
pub mod resources;
pub mod routing;
pub mod search;
pub mod state;
pub mod storage;
pub mod toolchain;
pub mod tunnels;
pub mod workspace;
