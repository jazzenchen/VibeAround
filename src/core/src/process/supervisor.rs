//! `Supervisor` — owns the lifecycle of every supervised subprocess.
//!
//! This replaces the ad-hoc spawn/kill/restart paths that previously lived
//! inside `channels::monitor`, `agent::runtime`, and friends. Managers hand
//! the supervisor a `SpawnSpec` plus a `BridgeFactory` at `register()`
//! time, and from then on the supervisor:
//!
//! - Spawns the child process (via `process::env::command`, which injects
//!   the enriched login-shell env) and transfers the `Child` to the global
//!   [`ChildRegistry`].
//! - Invokes the factory on every (re)spawn to build a fresh
//!   [`ProcessBridge`], hands the bridge the stdio pipes, and runs it to
//!   completion in a task.
//! - Drives a state machine (`NotStarted` → `Spawning` → `Running` →
//!   `Crashed` → `Spawning` …) on a single 5-second tick loop that honors
//!   the [`RestartPolicy`] attached to each process.
//! - Broadcasts every status change on a `tokio::sync::broadcast` channel
//!   so dashboards, HTTP handlers, and other subscribers only subscribe
//!   once instead of polling per-module monitors.
//!
//! The supervisor does NOT know anything about the protocol spoken over
//! the stdio pipes — that is entirely the bridge's concern. It does own
//! direct-child termination and reaping for normal stop/restart/shutdown;
//! [`ChildRegistry::kill_all`] remains the abrupt-runtime safety net.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, AtomicU8, Ordering};
use std::sync::Arc;
use std::time::Duration;

use parking_lot::RwLock;
use tokio::process::Command;
use tokio::sync::{broadcast, watch};
use tokio::task::JoinHandle;

use crate::proc_log;
use crate::process::bridge::{BridgeExit, BridgeFactory, StdioPipes};
use crate::process::env;
use crate::process::error::{ProcessError, ProcessResult};
use crate::process::kill;
use crate::process::registry::{ChildRegistry, ProcessKind};

mod generation;

/// Tick interval for the supervisor's scan loop.
pub const TICK_INTERVAL: Duration = Duration::from_secs(5);

/// Unique id for a supervised process within one Supervisor instance.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ProcessId(pub u64);

impl std::fmt::Display for ProcessId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

// ---------------------------------------------------------------------------
// SpawnSpec + RestartPolicy (caller-provided)
// ---------------------------------------------------------------------------

/// Recipe for spawning the child process. The supervisor uses this on every
/// (re)spawn — the bridge factory is invoked fresh each time.
#[derive(Debug, Clone)]
pub struct SpawnSpec {
    pub program: String,
    pub args: Vec<String>,
    pub cwd: Option<std::path::PathBuf>,
    pub extra_env: Vec<(String, String)>,
    /// If `true`, the bridge receives `stderr` via [`StdioPipes`]. If
    /// `false` (default), the supervisor spawns a task that logs each line
    /// via [`tracing::info!`] with the process's kind+label fields.
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
    /// Move to `Stopped` on any exit. The owning manager decides whether
    /// to re-register. Used by `AcpAgent` and `Pty`.
    Never,
    /// On unintended exit (crash / protocol error), schedule a respawn
    /// after `restart_delay`. If `watchdog` is `Some`, the supervisor kills
    /// processes whose `touch()` timestamp is older than the watchdog
    /// window — this catches frozen plugins that aren't emitting
    /// heartbeats. Used by `ChannelPlugin`.
    OnCrash {
        restart_delay: Duration,
        watchdog: Option<Duration>,
    },
}

impl RestartPolicy {
    fn restart_delay(&self) -> Option<Duration> {
        match self {
            RestartPolicy::Never => None,
            RestartPolicy::OnCrash { restart_delay, .. } => Some(*restart_delay),
        }
    }

    fn watchdog(&self) -> Option<Duration> {
        match self {
            RestartPolicy::Never => None,
            RestartPolicy::OnCrash { watchdog, .. } => *watchdog,
        }
    }
}

// ---------------------------------------------------------------------------
// Status + events
// ---------------------------------------------------------------------------

/// Lifecycle status of a supervised process. Stored as an `AtomicU8`.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProcessStatus {
    NotStarted = 0,
    Spawning = 1,
    Running = 2,
    Crashed = 3,
    Stopped = 4,
}

impl ProcessStatus {
    fn from_u8(v: u8) -> Self {
        match v {
            0 => Self::NotStarted,
            1 => Self::Spawning,
            2 => Self::Running,
            3 => Self::Crashed,
            _ => Self::Stopped,
        }
    }

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

/// Distinguishes a user action from an actual crash so that force_stop and
/// force_restart survive the race with a bridge-thread `mark_exit`.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TransitionIntent {
    None = 0,
    Stop = 1,
    Restart = 2,
}

impl TransitionIntent {
    fn from_u8(v: u8) -> Self {
        match v {
            1 => Self::Stop,
            2 => Self::Restart,
            _ => Self::None,
        }
    }
}

/// Public read-only snapshot of a supervised process — safe to expose via
/// HTTP / dashboard without leaking internal state.
#[derive(Debug, Clone)]
pub struct ProcessSnapshot {
    pub id: ProcessId,
    pub kind: ProcessKind,
    pub label: String,
    pub status: ProcessStatus,
    pub reason: String,
}

/// Broadcast payload for status changes. Subscribers receive which process
/// changed and re-read via [`Supervisor::snapshot`] if they need details —
/// matching the existing `StateSource` convention.
#[derive(Debug, Clone)]
pub struct ProcessEvent {
    pub id: ProcessId,
    pub kind: ProcessKind,
    pub status: ProcessStatus,
}

// ---------------------------------------------------------------------------
// Internal per-process state
// ---------------------------------------------------------------------------

struct SupervisedProcess {
    id: ProcessId,
    kind: ProcessKind,
    label: String,
    spec: SpawnSpec,
    policy: RestartPolicy,
    factory: BridgeFactory,

    status: AtomicU8,
    intent: AtomicU8,
    reason: RwLock<String>,

    last_heartbeat_ts: AtomicU64,
    next_spawn_at: AtomicU64,

    /// Cancel signal for the currently-running bridge. `None` between runs.
    cancel_tx: RwLock<Option<watch::Sender<bool>>>,

    /// `ChildRegistry` id for the currently-running spawn. Cleared on
    /// exit so the `Child` gets removed from the registry and reaped by
    /// [`Supervisor::handle_bridge_exit`] — otherwise every respawn
    /// leaks an entry and, on Unix, an unreaped zombie.
    current_registry_id: parking_lot::Mutex<Option<u64>>,

    /// Serializes lifecycle transitions that update status together with
    /// intent / restart scheduling. Process I/O and child reaping happen
    /// outside this lock.
    transition_lock: parking_lot::Mutex<()>,

    /// The currently scheduled spawn attempt. Shutdown takes and awaits this
    /// handle so a child cannot be registered after the process table has
    /// already been drained.
    spawn_task: parking_lot::Mutex<Option<JoinHandle<()>>>,
}

impl SupervisedProcess {
    fn set_status(&self, s: ProcessStatus) {
        self.status.store(s as u8, Ordering::Release);
    }

    fn status(&self) -> ProcessStatus {
        ProcessStatus::from_u8(self.status.load(Ordering::Acquire))
    }

    fn set_reason(&self, r: impl Into<String>) {
        *self.reason.write() = r.into();
    }
}

// ---------------------------------------------------------------------------
// Supervisor
// ---------------------------------------------------------------------------

pub struct Supervisor {
    registry: Arc<ChildRegistry>,
    processes: RwLock<HashMap<ProcessId, Arc<SupervisedProcess>>>,
    next_id: parking_lot::Mutex<u64>,
    change_tx: broadcast::Sender<ProcessEvent>,
    tick_loop_started: parking_lot::Mutex<bool>,
}

impl Supervisor {
    pub fn new(registry: Arc<ChildRegistry>) -> Arc<Self> {
        let (change_tx, _) = broadcast::channel(64);
        Arc::new(Self {
            registry,
            processes: RwLock::new(HashMap::new()),
            next_id: parking_lot::Mutex::new(1),
            change_tx,
            tick_loop_started: parking_lot::Mutex::new(false),
        })
    }

    /// Process-wide singleton. Bound to `ChildRegistry::global()`; the
    /// tick loop is auto-started on first access and runs for the
    /// remainder of the process lifetime — [`shutdown_all`] only drains
    /// the current process table so a subsequent daemon start gets a
    /// clean slate while the loop keeps ticking. Must be called from
    /// inside a tokio runtime.
    ///
    /// [`shutdown_all`]: Supervisor::shutdown_all
    pub fn global() -> Arc<Self> {
        use std::sync::OnceLock;
        static INSTANCE: OnceLock<Arc<Supervisor>> = OnceLock::new();
        Arc::clone(INSTANCE.get_or_init(|| {
            let sup = Supervisor::new(ChildRegistry::global());
            sup.spawn_tick_loop();
            sup
        }))
    }

    /// Register a new supervised process. Returns an opaque `ProcessId`
    /// that the caller uses for later `force_*`, `touch`, and status calls.
    /// The first spawn attempt is kicked off immediately (not waiting for
    /// the next tick).
    pub fn register(
        self: &Arc<Self>,
        kind: ProcessKind,
        label: impl Into<String>,
        spec: SpawnSpec,
        policy: RestartPolicy,
        factory: BridgeFactory,
    ) -> ProcessId {
        let label = label.into();
        let id = {
            let mut next = self.next_id.lock();
            let id = *next;
            *next = next.wrapping_add(1);
            ProcessId(id)
        };

        let proc = Arc::new(SupervisedProcess {
            id,
            kind,
            label: label.clone(),
            spec,
            policy,
            factory,
            status: AtomicU8::new(ProcessStatus::NotStarted as u8),
            intent: AtomicU8::new(TransitionIntent::None as u8),
            reason: RwLock::new(String::new()),
            last_heartbeat_ts: AtomicU64::new(now_secs()),
            next_spawn_at: AtomicU64::new(0),
            cancel_tx: RwLock::new(None),
            current_registry_id: parking_lot::Mutex::new(None),
            transition_lock: parking_lot::Mutex::new(()),
            spawn_task: parking_lot::Mutex::new(None),
        });

        self.processes.write().insert(id, Arc::clone(&proc));
        self.notify_change(&proc);

        // Immediate spawn — don't wait for the tick.
        self.schedule_spawn(proc);

        id
    }

    /// Bump the heartbeat timestamp. Managers call this on every heartbeat
    /// or keepalive from the remote end of the bridge — channel plugins
    /// on `_va/heartbeat`, ACP agents on any notification, etc.
    pub fn touch(&self, id: ProcessId) {
        if let Some(proc) = self.processes.read().get(&id).cloned() {
            proc.last_heartbeat_ts.store(now_secs(), Ordering::Relaxed);
        }
    }

    /// Stop the process. Cancels the current bridge and leaves the process
    /// in `Stopped` — no respawn regardless of policy.
    pub async fn force_stop(&self, id: ProcessId) -> ProcessResult<()> {
        let proc = self.get_proc(id)?;
        {
            let _transition = proc.transition_lock.lock();
            proc.set_status(ProcessStatus::Stopped);
            proc.intent
                .store(TransitionIntent::None as u8, Ordering::Release);
            proc.next_spawn_at.store(0, Ordering::Relaxed);
            proc.set_reason("stopped by user");
            self.cancel_current_bridge(&proc);
        }

        self.notify_change(&proc);
        let spawn_task = proc.spawn_task.lock().take();
        if let Some(spawn_task) = spawn_task {
            let _ = spawn_task.await;
        }
        self.terminate_and_reap_current_child(&proc).await;
        self.deregister_terminal_process(&proc);
        Ok(())
    }

    /// Stop, reap, and permanently remove one registered process.
    pub async fn unregister(&self, id: ProcessId) -> ProcessResult<()> {
        self.force_stop(id).await?;
        self.processes.write().remove(&id);
        Ok(())
    }

    /// Cancel the current generation and schedule an immediate respawn.
    /// No-op if policy is `Never` and the process is already stopped.
    pub async fn force_restart(self: &Arc<Self>, id: ProcessId) -> ProcessResult<()> {
        let proc = self.get_proc(id)?;
        let mut spawn_now = false;
        let mut terminate_current_generation = false;
        {
            let _transition = proc.transition_lock.lock();
            match proc.status() {
                ProcessStatus::Running => {
                    proc.intent
                        .store(TransitionIntent::Restart as u8, Ordering::Release);
                    terminate_current_generation = !self.cancel_current_bridge(&proc);
                    proc.set_reason("restart requested");
                }
                ProcessStatus::Spawning => {
                    // Supersede the staged generation. `begin_spawn` will
                    // observe the changed status and reap any child it has
                    // already created; the tick loop starts the replacement
                    // only after that cleanup path has had a chance to run.
                    proc.set_status(ProcessStatus::Crashed);
                    proc.intent
                        .store(TransitionIntent::None as u8, Ordering::Release);
                    proc.next_spawn_at.store(now_secs(), Ordering::Relaxed);
                    proc.set_reason("restart requested while spawning");
                    self.cancel_current_bridge(&proc);
                    terminate_current_generation = true;
                }
                ProcessStatus::Crashed | ProcessStatus::NotStarted => {
                    proc.set_status(ProcessStatus::Crashed);
                    proc.intent
                        .store(TransitionIntent::None as u8, Ordering::Release);
                    proc.next_spawn_at.store(now_secs(), Ordering::Relaxed);
                    proc.set_reason("restart requested");
                    spawn_now = true;
                }
                ProcessStatus::Stopped if matches!(proc.policy, RestartPolicy::OnCrash { .. }) => {
                    proc.set_status(ProcessStatus::Crashed);
                    proc.intent
                        .store(TransitionIntent::None as u8, Ordering::Release);
                    proc.next_spawn_at.store(now_secs(), Ordering::Relaxed);
                    proc.set_reason("restart requested");
                    spawn_now = true;
                }
                ProcessStatus::Stopped => {
                    proc.intent
                        .store(TransitionIntent::None as u8, Ordering::Release);
                    proc.next_spawn_at.store(0, Ordering::Relaxed);
                }
            }
        }

        if terminate_current_generation {
            self.terminate_and_reap_current_child(&proc).await;
        }
        self.notify_change(&proc);
        if spawn_now {
            self.schedule_spawn(proc);
        }
        Ok(())
    }

    /// If the process is `Stopped` / `Crashed` / `NotStarted`, schedule an
    /// immediate respawn. Ignored in `Running` / `Spawning`.
    pub fn force_start(self: &Arc<Self>, id: ProcessId) -> ProcessResult<()> {
        let proc = self.get_proc(id)?;
        let should_spawn = {
            let _transition = proc.transition_lock.lock();
            if matches!(
                proc.status(),
                ProcessStatus::Stopped | ProcessStatus::Crashed | ProcessStatus::NotStarted
            ) {
                proc.intent
                    .store(TransitionIntent::None as u8, Ordering::Release);
                proc.set_status(ProcessStatus::Crashed);
                proc.set_reason("started by user");
                proc.next_spawn_at.store(now_secs(), Ordering::Relaxed);
                true
            } else {
                false
            }
        };
        if should_spawn {
            self.notify_change(&proc);
            self.schedule_spawn(proc);
        }
        Ok(())
    }

    /// Snapshot of every registered process, sorted by label.
    pub fn snapshot(&self) -> Vec<ProcessSnapshot> {
        let mut out: Vec<_> = self
            .processes
            .read()
            .values()
            .map(|proc| ProcessSnapshot {
                id: proc.id,
                kind: proc.kind,
                label: proc.label.clone(),
                status: proc.status(),
                reason: proc.reason.read().clone(),
            })
            .collect();
        out.sort_by(|a, b| a.label.cmp(&b.label));
        out
    }

    /// Subscribe to per-process status change events.
    pub fn subscribe(&self) -> broadcast::Receiver<ProcessEvent> {
        self.change_tx.subscribe()
    }

    /// Start the supervisor's 5-second scan loop. Idempotent — a second
    /// call is a no-op. The loop runs for the process lifetime; daemon
    /// stop/restart cycles just drain the process table via
    /// [`shutdown_all`].
    ///
    /// [`shutdown_all`]: Supervisor::shutdown_all
    pub fn spawn_tick_loop(self: &Arc<Self>) {
        let mut started = self.tick_loop_started.lock();
        if *started {
            return;
        }
        *started = true;
        drop(started);
        let sup = Arc::clone(self);
        tokio::spawn(async move {
            sup.run_tick_loop().await;
        });
    }

    /// Cancel every active bridge and drain the process table so a
    /// subsequent daemon start gets a clean slate. The tick loop keeps
    /// running — it's process-wide and survives daemon restart.
    /// Every in-flight spawn is joined and every registered child is reaped
    /// before this method returns. `ChildRegistry::kill_all()` remains the
    /// abrupt-runtime safety net in `RunningDaemon::stop`.
    pub async fn shutdown_all(&self) {
        let procs: Vec<Arc<SupervisedProcess>> = self
            .processes
            .write()
            .drain()
            .map(|(_, proc)| proc)
            .collect();
        for proc in &procs {
            let _transition = proc.transition_lock.lock();
            proc.set_status(ProcessStatus::Stopped);
            proc.intent
                .store(TransitionIntent::None as u8, Ordering::Release);
            proc.next_spawn_at.store(0, Ordering::Relaxed);
            proc.set_reason("supervisor shutdown");
            self.cancel_current_bridge(proc);
            self.notify_change(proc);
        }
        for proc in procs {
            let spawn_task = proc.spawn_task.lock().take();
            if let Some(spawn_task) = spawn_task {
                let _ = spawn_task.await;
            }
            self.terminate_and_reap_current_child(&proc).await;
        }
    }

    // -----------------------------------------------------------------------
    // Internals
    // -----------------------------------------------------------------------

    fn get_proc(&self, id: ProcessId) -> ProcessResult<Arc<SupervisedProcess>> {
        self.processes
            .read()
            .get(&id)
            .cloned()
            .ok_or_else(|| ProcessError::UnknownProcess {
                label: format!("#{}", id.0),
            })
    }

    fn cancel_current_bridge(&self, proc: &SupervisedProcess) -> bool {
        if let Some(tx) = proc.cancel_tx.read().as_ref() {
            return tx.send(true).is_ok();
        }
        false
    }

    fn notify_change(&self, proc: &SupervisedProcess) {
        let _ = self.change_tx.send(ProcessEvent {
            id: proc.id,
            kind: proc.kind,
            status: proc.status(),
        });
    }
}

fn now_secs() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}
// Tests live beside the implementation so production lifecycle code remains
// reviewable without a 600-line test tail.
#[cfg(test)]
mod tests;
