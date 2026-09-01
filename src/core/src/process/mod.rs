//! Subprocess management.
//!
//! - [`env`]: builds `Command`s with the user's full login-shell environment
//!   injected, so GUI-launched Tauri apps inherit `PATH` / NVM / API keys.
//! - [`registry`]: takes ownership of every spawned `Child` in a global
//!   table so daemon shutdown can synchronously SIGKILL them regardless of
//!   tokio task-poll order. Used by the supervisor + legacy call sites.
//! - [`supervisor`]: the unified lifecycle layer — spawn, restart, status,
//!   structured logging — shared by channel plugins, ACP agents, and
//!   tunnels.
//! - [`bridge`]: manager-side trait for driving a protocol over the stdio
//!   pipes the supervisor hands back.
//! - [`error`]: `ProcessError` at the supervisor boundary.

pub mod acp_transport;
pub mod bridge;
pub mod env;
pub mod error;
pub mod kill;
pub mod log;
pub mod registry;
pub mod supervisor;

pub use bridge::{
    BridgeExit, BridgeFactory, BridgeFuture, CancelSignal, ProcessBridge, StdioPipes,
};
pub use error::{ProcessError, ProcessResult};
pub use kill::{spawn_tree_killable, TreeKillableChild};
pub use registry::{ChildRegistry, ProcessKind};
pub use supervisor::{
    ProcessEvent, ProcessId, ProcessSnapshot, ProcessStatus, RestartPolicy, SpawnSpec, Supervisor,
};
