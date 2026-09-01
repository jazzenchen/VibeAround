use std::time::Duration;

use tokio::sync::{mpsc, oneshot, watch};

use crate::process::registry::ProcessKind;

use super::ProcessId;

/// Recipe for spawning the child process. The supervisor uses this on every
/// (re)spawn — the bridge factory is invoked fresh each time.
#[derive(Debug, Clone)]
pub struct SpawnSpec {
    pub program: String,
    pub args: Vec<String>,
    pub cwd: Option<std::path::PathBuf>,
    pub extra_env: Vec<(String, String)>,
    /// If `true`, the bridge receives `stderr` via
    /// [`StdioPipes`](crate::process::bridge::StdioPipes). If
    /// `false` (default), the supervisor logs each line with process fields.
    pub capture_stderr: bool,
}

impl SpawnSpec {
    pub fn new(program: impl Into<String>) -> Self {
        Self {
            program: program.into(),
            args: Vec::new(),
            cwd: None,
            extra_env: Vec::new(),
            capture_stderr: false,
        }
    }

    pub fn arg(mut self, a: impl Into<String>) -> Self {
        self.args.push(a.into());
        self
    }

    pub fn args<I, S>(mut self, a: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.args.extend(a.into_iter().map(|s| s.into()));
        self
    }

    pub fn cwd(mut self, p: impl Into<std::path::PathBuf>) -> Self {
        self.cwd = Some(p.into());
        self
    }

    pub fn env(mut self, k: impl Into<String>, v: impl Into<String>) -> Self {
        self.extra_env.push((k.into(), v.into()));
        self
    }

    pub fn capture_stderr(mut self, on: bool) -> Self {
        self.capture_stderr = on;
        self
    }
}

/// What to do when a supervised process dies.
#[derive(Debug, Clone, Copy)]
pub enum RestartPolicy {
    /// Move to `Stopped` on any exit or spawn failure — terminal either
    /// way, and the owner task ends so the bridge factory is dropped. The
    /// owning manager decides whether to re-register. Used by `AcpAgent`.
    Never,
    /// On unintended exit, schedule a respawn after `restart_delay`.
    OnCrash {
        restart_delay: Duration,
        watchdog: Option<Duration>,
    },
}

impl RestartPolicy {
    pub(super) fn restart_delay(&self) -> Option<Duration> {
        match self {
            RestartPolicy::Never => None,
            RestartPolicy::OnCrash { restart_delay, .. } => Some(*restart_delay),
        }
    }

    pub(super) fn watchdog(&self) -> Option<Duration> {
        match self {
            RestartPolicy::Never => None,
            RestartPolicy::OnCrash { watchdog, .. } => *watchdog,
        }
    }
}

/// Lifecycle status of a supervised process.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProcessStatus {
    NotStarted = 0,
    Spawning = 1,
    Running = 2,
    Crashed = 3,
    Stopped = 4,
}

impl ProcessStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::NotStarted => "not_started",
            Self::Spawning => "spawning",
            Self::Running => "running",
            Self::Crashed => "crashed",
            Self::Stopped => "stopped",
        }
    }
}

/// Public read-only snapshot safe to expose via HTTP and dashboards.
#[derive(Debug, Clone)]
pub struct ProcessSnapshot {
    pub id: ProcessId,
    pub kind: ProcessKind,
    pub label: String,
    pub status: ProcessStatus,
    pub reason: String,
}

/// Broadcast payload for status changes. Subscribers can re-read details via
/// [`Supervisor::snapshot`](super::Supervisor::snapshot).
#[derive(Debug, Clone)]
pub struct ProcessEvent {
    pub id: ProcessId,
    pub kind: ProcessKind,
    pub status: ProcessStatus,
}

#[derive(Debug, Clone)]
pub(super) struct ProcessState {
    pub(super) status: ProcessStatus,
    pub(super) reason: String,
}

pub(super) enum ProcessCommand {
    Touch,
    Tick(u64),
    Start,
    Stop {
        reason: String,
        reply: oneshot::Sender<()>,
    },
    Shutdown {
        reason: String,
        reply: oneshot::Sender<()>,
    },
    Restart {
        apply_backoff: bool,
        reply: oneshot::Sender<super::ProcessResult<()>>,
    },
    BridgeExited {
        generation_id: u64,
        exit: crate::process::bridge::BridgeExit,
    },
}

pub(super) struct SupervisedProcess {
    pub(super) id: ProcessId,
    pub(super) kind: ProcessKind,
    pub(super) label: String,
    pub(super) command_tx: mpsc::UnboundedSender<ProcessCommand>,
    pub(super) state_tx: watch::Sender<ProcessState>,
}

impl SupervisedProcess {
    pub(super) fn status(&self) -> ProcessStatus {
        self.state_tx.borrow().status
    }

    pub(super) fn reason(&self) -> String {
        self.state_tx.borrow().reason.clone()
    }
}
