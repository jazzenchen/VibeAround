//! Subprocess management.
//!
//! - [`env`]: builds `Command`s with the user's full login-shell environment
//!   injected, so GUI-launched Tauri apps inherit `PATH` / NVM / API keys.
//! - [`supervisor`]: the single owner of every long-lived child — spawn,
//!   restart, status, structured logging, and the synchronous emergency
//!   shutdown — shared by channel plugins, ACP agents, tunnels, and the
//!   search provider.
//! - [`orphan`]: startup sweep that kills children left over from a
//!   previous daemon crash.
//! - [`bridge`]: manager-side trait for driving a protocol over the stdio
//!   pipes the supervisor hands back.
//! - [`error`]: `ProcessError` at the supervisor boundary.

pub mod acp_transport;
pub mod bridge;
pub mod env;
pub mod error;
pub mod kill;
pub mod log;
pub mod orphan;
pub mod supervisor;

pub use bridge::{
    BridgeExit, BridgeFactory, BridgeFuture, CancelSignal, ProcessBridge, StdioPipes,
};
pub use error::{ProcessError, ProcessResult};
pub use kill::{spawn_tree_killable, TreeKillableChild};
pub use orphan::orphan_sweep;
pub use supervisor::{
    ProcessEvent, ProcessId, ProcessSnapshot, ProcessStatus, RestartPolicy, SpawnSpec, Supervisor,
};

/// Classification used for orphan detection and structured logging.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProcessKind {
    /// Channel plugin process (node running from a discovered plugin directory).
    ChannelPlugin,
    /// ACP coding-agent child (node running the ACP bridge package).
    AcpAgent,
    /// Tunnel provider subprocess (cloudflared, lt, tailscale, …). Not ngrok (SDK).
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
