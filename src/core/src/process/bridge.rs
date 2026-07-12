//! `ProcessBridge` — the manager-owned half of a supervised subprocess.
//!
//! The supervisor owns the `Child` and everything lifecycle-related (spawn,
//! kill, restart, status broadcast). When a child is spawned, the supervisor
//! calls a caller-supplied `BridgeFactory` to build a fresh `ProcessBridge`,
//! then hands over the stdio pipes by calling `run()`. The bridge runs the
//! protocol (ACP, URL parsing, whatever) until the pipes close or the cancel
//! token fires, then returns a `BridgeExit`.
//!
//! A bridge is **one-shot**: the factory is invoked again for each respawn.
//! Protocol-level state that must survive restart (e.g. ACP session_id) is
//! the manager's job to thread through the factory closure.

use std::future::Future;
use std::pin::Pin;

use tokio::process::{ChildStderr, ChildStdin, ChildStdout};
use tokio::sync::watch;

/// Stdio pipes handed from the supervisor to the bridge.
///
/// `stderr` is `None` if the supervisor has opted to log it itself (default
/// for `ChannelPlugin` / `AcpAgent`). Bridges that want to parse stderr
/// (e.g. cloudflared extracting its URL) set `capture_stderr` on
/// [`super::SpawnSpec`] and receive `Some`.
pub struct StdioPipes {
    pub stdin: ChildStdin,
    pub stdout: ChildStdout,
    pub stderr: Option<ChildStderr>,
}

/// How a bridge run ended. Drives the supervisor's restart decision.
#[derive(Debug)]
pub enum BridgeExit {
    /// Pipes closed cleanly (EOF). Child process likely exited on its own.
    Clean,
    /// Bridge saw a protocol-level failure — malformed frame, handshake
    /// timeout, etc. Counts as a crash for restart policies.
    ProtocolError(anyhow::Error),
    /// Supervisor asked us to stop via the cancel token.
    Cancelled,
}

/// A cancellation signal the supervisor shares with the currently-running
/// bridge. When `*rx.borrow() == true`, the bridge should drain + return
/// `BridgeExit::Cancelled` ASAP.
pub type CancelSignal = watch::Receiver<bool>;

/// Future type returned by `ProcessBridge::run`. Boxed so bridges can return
/// heterogeneous future shapes (`async move { ... }`, `spawn_local` bridges,
/// etc.).
pub type BridgeFuture = Pin<Box<dyn Future<Output = BridgeExit> + Send + 'static>>;

/// The manager's contract with the supervisor. Implement this on a type
/// that knows how to drive one protocol over one pair of pipes.
///
/// One bridge per spawn. Not reused across restart.
pub trait ProcessBridge: Send + 'static {
    /// Run the bridge until the pipes close, a protocol error occurs, or
    /// the cancel signal fires. Must not panic.
    fn run(self: Box<Self>, pipes: StdioPipes, cancel: CancelSignal) -> BridgeFuture;
}

/// Factory the supervisor invokes on each (re)spawn to build a fresh bridge.
/// Typically a closure that captures manager-side channels / config.
pub type BridgeFactory = Box<dyn FnMut() -> Box<dyn ProcessBridge> + Send + 'static>;
