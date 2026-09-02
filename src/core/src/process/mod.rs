//! Subprocess management.
//!
//! - [`env`]: builds `Command`s with the user's full login-shell environment
//!   injected, so GUI-launched Tauri apps inherit `PATH` / NVM / API keys.
//! - [`supervisor`]: the single owner of every long-lived child — spawn,
//!   restart, status, structured logging, orderly stop — shared by channel
//!   plugins, ACP agents, tunnels, and the search provider.
//! - [`lease`]: the kernel-held guarantee that no child outlives the
//!   daemon, however the daemon dies (a pipe-bound reaper on Unix; an
//!   interim pid roster on Windows until its Job Object lands).
//! - [`orphan`] (Windows only): startup sweep that kills children left
//!   over from a previous daemon crash, until the Windows lease is real.
//! - [`bridge`]: manager-side trait for driving a protocol over the stdio
//!   pipes the supervisor hands back.
//! - [`error`]: `ProcessError` at the supervisor boundary.

pub mod acp_transport;
pub mod bridge;
pub mod env;
pub mod error;
pub mod kill;
pub mod lease;
pub mod log;
#[cfg(windows)]
pub mod orphan;
pub mod supervisor;

pub use bridge::{
    BridgeExit, BridgeFactory, BridgeFuture, CancelSignal, ProcessBridge, StdioPipes,
};
pub use error::{ProcessError, ProcessResult};
pub use kill::{spawn_tree_killable, TreeKillableChild};
pub use lease::Lease;
#[cfg(windows)]
pub use orphan::orphan_sweep;
pub use supervisor::{
    ProcessEvent, ProcessId, ProcessSnapshot, ProcessStatus, RestartPolicy, SpawnSpec, Supervisor,
};

/// Classification used for structured logging and status surfaces.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProcessKind {
    /// Channel plugin process (node running from a discovered plugin directory).
    ChannelPlugin,
    /// ACP coding-agent child (node running the ACP bridge package).
    AcpAgent,
    /// Tunnel provider subprocess (cloudflared, ngrok, tailscale, …).
    Tunnel,
    /// Host-side search provider subprocess (va-search-tool stdio).
    SearchProvider,
}

impl ProcessKind {
    /// Short lowercase tag used in structured logs (`kind=channel_plugin`).
    pub fn as_str(&self) -> &'static str {
        match self {
            ProcessKind::ChannelPlugin => "channel_plugin",
            ProcessKind::AcpAgent => "acp_agent",
            ProcessKind::Tunnel => "tunnel",
            ProcessKind::SearchProvider => "search_provider",
        }
    }
}
