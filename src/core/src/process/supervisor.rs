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
//! the stdio pipes — that is entirely the bridge's concern. It also does
//! NOT take responsibility for SIGKILLing children on abrupt daemon
//! shutdown — that is [`ChildRegistry::kill_all`]'s job. The supervisor
//! only drives the happy-path cancel + drop sequence.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, AtomicU8, Ordering};
use std::sync::Arc;
use std::time::Duration;

use parking_lot::RwLock;
use tokio::process::Command;
use tokio::sync::{broadcast, watch};

use crate::proc_log;
use crate::process::bridge::{BridgeExit, BridgeFactory, StdioPipes};
use crate::process::env;
use crate::process::error::{ProcessError, ProcessResult};
use crate::process::registry::{ChildRegistry, ProcessKind};

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
        });

        self.processes.write().insert(id, Arc::clone(&proc));
        self.notify_change(&proc);

        // Immediate spawn — don't wait for the tick.
        let sup = Arc::clone(self);
        tokio::spawn(async move {
            sup.begin_spawn(proc).await;
        });

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
        let mut stopped_without_bridge = false;
        {
            let _transition = proc.transition_lock.lock();
            match proc.status() {
                ProcessStatus::Running => {
                    proc.intent
                        .store(TransitionIntent::Stop as u8, Ordering::Release);
                    if !self.cancel_current_bridge(&proc) {
                        stopped_without_bridge = true;
                        proc.set_status(ProcessStatus::Stopped);
                        proc.intent
                            .store(TransitionIntent::None as u8, Ordering::Release);
                        proc.next_spawn_at.store(0, Ordering::Relaxed);
                        proc.set_reason("stopped by user");
                    }
                }
                ProcessStatus::Spawning | ProcessStatus::Crashed | ProcessStatus::NotStarted => {
                    stopped_without_bridge = true;
                    proc.set_status(ProcessStatus::Stopped);
                    proc.intent
                        .store(TransitionIntent::None as u8, Ordering::Release);
                    proc.next_spawn_at.store(0, Ordering::Relaxed);
                    proc.set_reason("stopped by user");
                    self.cancel_current_bridge(&proc);
                }
                ProcessStatus::Stopped => {
                    proc.intent
                        .store(TransitionIntent::None as u8, Ordering::Release);
                    proc.next_spawn_at.store(0, Ordering::Relaxed);
                }
            }
        }

        if stopped_without_bridge {
            self.notify_change(&proc);
            self.terminate_and_reap_current_child(&proc).await;
            self.deregister_terminal_process(&proc);
        }
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
            let sup = Arc::clone(self);
            tokio::spawn(async move {
                sup.begin_spawn(proc).await;
            });
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
            let sup = Arc::clone(self);
            tokio::spawn(async move {
                sup.begin_spawn(proc).await;
            });
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
    /// `ChildRegistry::kill_all()` is the hard-kill safety net from
    /// `RunningDaemon::stop`.
    pub async fn shutdown_all(&self) {
        let procs: Vec<Arc<SupervisedProcess>> = self
            .processes
            .write()
            .drain()
            .map(|(_, proc)| proc)
            .collect();
        for proc in procs {
            proc.intent
                .store(TransitionIntent::Stop as u8, Ordering::Release);
            self.cancel_current_bridge(&proc);
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

    async fn run_tick_loop(self: Arc<Self>) {
        let mut ticker = tokio::time::interval(TICK_INTERVAL);
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        ticker.tick().await; // consume the immediate first tick

        tracing::info!(
            tick_secs = TICK_INTERVAL.as_secs(),
            "supervisor loop started"
        );
        loop {
            ticker.tick().await;
            self.tick().await;
        }
    }

    async fn tick(self: &Arc<Self>) {
        let now = now_secs();
        let mut to_spawn: Vec<Arc<SupervisedProcess>> = Vec::new();
        let mut to_watchdog: Vec<Arc<SupervisedProcess>> = Vec::new();

        for proc in self.processes.read().values().cloned() {
            match proc.status() {
                ProcessStatus::NotStarted => to_spawn.push(proc),
                ProcessStatus::Crashed => {
                    let at = proc.next_spawn_at.load(Ordering::Relaxed);
                    if at != 0 && now >= at {
                        to_spawn.push(proc);
                    }
                }
                ProcessStatus::Running => {
                    if let Some(watchdog) = proc.policy.watchdog() {
                        let last = proc.last_heartbeat_ts.load(Ordering::Relaxed);
                        if now.saturating_sub(last) > watchdog.as_secs() {
                            to_watchdog.push(proc);
                        }
                    }
                }
                ProcessStatus::Spawning | ProcessStatus::Stopped => {}
            }
        }

        for proc in to_watchdog {
            let age = now.saturating_sub(proc.last_heartbeat_ts.load(Ordering::Relaxed));
            proc_log!(
                info,
                kind = proc.kind,
                label = proc.label,
                event = "watchdog_fired",
                heartbeat_age_secs = age
            );
            self.cancel_current_bridge(&proc);
        }

        for proc in to_spawn {
            let sup = Arc::clone(self);
            tokio::spawn(async move {
                sup.begin_spawn(proc).await;
            });
        }
    }

    async fn begin_spawn(self: &Arc<Self>, proc: Arc<SupervisedProcess>) {
        // Guard against racing tickers / immediate-spawn.
        {
            let _transition = proc.transition_lock.lock();
            if !matches!(
                proc.status(),
                ProcessStatus::NotStarted | ProcessStatus::Crashed
            ) {
                return;
            }
            proc.set_status(ProcessStatus::Spawning);
            proc.next_spawn_at.store(0, Ordering::Relaxed);
            proc.set_reason("spawning");
        }
        self.notify_change(&proc);

        match self.spawn_child(&proc).await {
            Ok((pipes, cancel_rx)) => {
                // Publish status Running unless force_* landed during the spawn.
                let publish_running = {
                    let _transition = proc.transition_lock.lock();
                    if matches!(proc.status(), ProcessStatus::Spawning) {
                        proc.set_status(ProcessStatus::Running);
                        proc.set_reason("");
                        proc.last_heartbeat_ts.store(now_secs(), Ordering::Relaxed);
                        true
                    } else {
                        false
                    }
                };
                if !publish_running {
                    proc_log!(
                        info,
                        kind = proc.kind,
                        label = proc.label,
                        event = "spawn_superseded"
                    );
                    // No bridge task has been started yet, so cancellation alone
                    // cannot clean up this staged generation.
                    *proc.cancel_tx.write() = None;
                    self.terminate_and_reap_current_child(&proc).await;
                    return;
                }
                proc_log!(
                    info,
                    kind = proc.kind,
                    label = proc.label,
                    event = "running"
                );
                self.notify_change(&proc);

                // Hand pipes to the bridge in a task; when it returns, we
                // transition based on intent.
                let bridge = (proc.factory)();
                let sup = Arc::clone(self);
                let proc_for_task = Arc::clone(&proc);
                tokio::spawn(async move {
                    let exit = bridge.run(pipes, cancel_rx).await;
                    sup.handle_bridge_exit(proc_for_task, exit).await;
                });
            }
            Err(e) => {
                let reason = format!("{}", e);
                let publish_crash = {
                    let _transition = proc.transition_lock.lock();
                    if matches!(proc.status(), ProcessStatus::Spawning) {
                        proc.set_status(ProcessStatus::Crashed);
                        proc.set_reason(format!("spawn failed: {}", reason));
                        if let Some(delay) = proc.policy.restart_delay() {
                            proc.next_spawn_at
                                .store(now_secs() + delay.as_secs(), Ordering::Relaxed);
                        }
                        true
                    } else {
                        false
                    }
                };
                if !publish_crash {
                    proc_log!(
                        error,
                        kind = proc.kind,
                        label = proc.label,
                        event = "spawn_failed_superseded",
                        error = %reason
                    );
                    return;
                }
                proc_log!(
                    error,
                    kind = proc.kind,
                    label = proc.label,
                    event = "spawn_failed",
                    error = %reason
                );
                self.notify_change(&proc);
            }
        }
    }

    async fn spawn_child(
        &self,
        proc: &SupervisedProcess,
    ) -> ProcessResult<(StdioPipes, watch::Receiver<bool>)> {
        let mut cmd: Command = env::command(&proc.spec.program);
        cmd.args(&proc.spec.args)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .kill_on_drop(true);
        if let Some(cwd) = &proc.spec.cwd {
            cmd.current_dir(cwd);
        }
        for (k, v) in &proc.spec.extra_env {
            cmd.env(k, v);
        }

        let mut child = cmd.spawn().map_err(|e| ProcessError::Spawn {
            program: proc.spec.program.clone(),
            source: e,
        })?;

        let stdin = child
            .stdin
            .take()
            .ok_or(ProcessError::StdioUnavailable { what: "stdin" })?;
        let stdout = child
            .stdout
            .take()
            .ok_or(ProcessError::StdioUnavailable { what: "stdout" })?;
        let stderr_raw = child
            .stderr
            .take()
            .ok_or(ProcessError::StdioUnavailable { what: "stderr" })?;
        let pid = child.id();

        // Hand ownership of the Child to the global registry. This is the
        // canonical owner — kill_on_drop alone can't be relied on under
        // abrupt runtime teardown. The id is stashed on the proc so
        // `handle_bridge_exit` can remove + reap the child (otherwise
        // every respawn leaks a registry entry + zombie process).
        let registry_id = self.registry.register(proc.kind, proc.label.clone(), child);
        *proc.current_registry_id.lock() = Some(registry_id);

        proc_log!(
            info,
            kind = proc.kind,
            label = proc.label,
            pid = pid,
            event = "spawned",
            program = %proc.spec.program
        );

        let stderr = if proc.spec.capture_stderr {
            Some(stderr_raw)
        } else {
            let kind = proc.kind;
            let label = proc.label.clone();
            tokio::spawn(async move {
                use tokio::io::AsyncBufReadExt;
                let reader = tokio::io::BufReader::new(stderr_raw);
                let mut lines = reader.lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    proc_log!(
                        info,
                        kind = kind,
                        label = label,
                        event = "stderr",
                        line = %line
                    );
                }
            });
            None
        };

        let (cancel_tx, cancel_rx) = watch::channel(false);
        *proc.cancel_tx.write() = Some(cancel_tx);

        Ok((
            StdioPipes {
                stdin,
                stdout,
                stderr,
            },
            cancel_rx,
        ))
    }

    async fn handle_bridge_exit(self: Arc<Self>, proc: Arc<SupervisedProcess>, exit: BridgeExit) {
        // Clear the cancel channel — this run is done.
        *proc.cancel_tx.write() = None;

        // A bridge can finish while its child is frozen or otherwise still
        // alive. Terminate that generation before waiting so respawn cannot
        // leave a twin process behind, then reap before publishing the next
        // state transition.
        self.terminate_and_reap_current_child(&proc).await;

        let (reason, was_crash) = match exit {
            BridgeExit::Clean => ("clean exit".to_string(), false),
            BridgeExit::Cancelled => ("cancelled".to_string(), false),
            BridgeExit::ProtocolError(e) => (format!("protocol error: {}", e), true),
        };

        let transition = proc.transition_lock.lock();
        if matches!(proc.status(), ProcessStatus::Stopped) {
            // `force_stop` won while this generation was being reaped.
            proc.intent
                .store(TransitionIntent::None as u8, Ordering::Release);
            proc.next_spawn_at.store(0, Ordering::Relaxed);
            drop(transition);
            self.deregister_terminal_process(&proc);
            return;
        }

        // Atomically consume intent so two callers don't both observe Stop.
        let intent = TransitionIntent::from_u8(
            proc.intent
                .swap(TransitionIntent::None as u8, Ordering::AcqRel),
        );

        match intent {
            TransitionIntent::Stop => {
                proc.set_status(ProcessStatus::Stopped);
                proc.next_spawn_at.store(0, Ordering::Relaxed);
                proc.set_reason(&reason);
                proc_log!(
                    info,
                    kind = proc.kind,
                    label = proc.label,
                    event = "stopped",
                    reason = %reason
                );
            }
            TransitionIntent::Restart => {
                proc.set_status(ProcessStatus::Crashed);
                proc.next_spawn_at.store(now_secs(), Ordering::Relaxed);
                proc.set_reason(&reason);
                proc_log!(
                    info,
                    kind = proc.kind,
                    label = proc.label,
                    event = "restart_requested",
                    reason = %reason
                );
            }
            TransitionIntent::None => match proc.policy {
                RestartPolicy::Never => {
                    proc.set_status(ProcessStatus::Stopped);
                    proc.next_spawn_at.store(0, Ordering::Relaxed);
                    proc.set_reason(&reason);
                    if was_crash {
                        proc_log!(
                            warn,
                            kind = proc.kind,
                            label = proc.label,
                            event = "exited_no_restart",
                            reason = %reason
                        );
                    } else {
                        proc_log!(
                            info,
                            kind = proc.kind,
                            label = proc.label,
                            event = "exited",
                            reason = %reason
                        );
                    }
                }
                RestartPolicy::OnCrash { .. } => {
                    proc.set_status(ProcessStatus::Crashed);
                    let delay = proc.policy.restart_delay().unwrap_or(Duration::ZERO);
                    proc.next_spawn_at
                        .store(now_secs() + delay.as_secs(), Ordering::Relaxed);
                    proc.set_reason(&reason);
                    proc_log!(
                        warn,
                        kind = proc.kind,
                        label = proc.label,
                        event = "crashed",
                        reason = %reason,
                        respawn_in_secs = delay.as_secs()
                    );
                }
            },
        }
        drop(transition);
        self.notify_change(&proc);
        self.deregister_terminal_process(&proc);
    }

    async fn terminate_and_reap_current_child(&self, proc: &SupervisedProcess) {
        let Some(id) = proc.current_registry_id.lock().take() else {
            return;
        };
        let Some(mut child) = self.registry.remove(id) else {
            return;
        };

        match child.try_wait() {
            Ok(Some(_)) => return,
            Ok(None) => {
                if let Err(error) = child.start_kill() {
                    proc_log!(
                        warn,
                        kind = proc.kind,
                        label = proc.label,
                        event = "child_kill_failed",
                        error = %error
                    );
                }
            }
            Err(error) => {
                proc_log!(
                    warn,
                    kind = proc.kind,
                    label = proc.label,
                    event = "child_status_failed",
                    error = %error
                );
                let _ = child.start_kill();
            }
        }

        if let Err(error) = child.wait().await {
            proc_log!(
                warn,
                kind = proc.kind,
                label = proc.label,
                event = "child_reap_failed",
                error = %error
            );
        }
    }

    fn deregister_terminal_process(&self, proc: &SupervisedProcess) {
        // Auto-deregister terminal one-shot processes. Without this the
        // `processes` map grows unbounded over daemon lifetime as
        // `RestartPolicy::Never` workloads (chiefly `AcpAgent` spawns
        // tied to one-shot agent launches) accumulate Stopped entries.
        // Keeping `OnCrash` entries around is deliberate: a user-stopped
        // channel plugin can still be resurrected via `force_start`.
        if matches!(proc.policy, RestartPolicy::Never)
            && matches!(proc.status(), ProcessStatus::Stopped)
            && self.processes.write().remove(&proc.id).is_some()
        {
            proc_log!(
                info,
                kind = proc.kind,
                label = proc.label,
                event = "deregistered"
            );
        }
    }
}

fn now_secs() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::process::bridge::{BridgeExit, CancelSignal, ProcessBridge, StdioPipes};
    #[cfg(unix)]
    use std::sync::atomic::AtomicU32;
    use std::sync::atomic::AtomicUsize;
    use std::sync::Arc;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    /// Bridge that waits for the cancel signal, then returns `Cancelled`.
    /// Drains stdout in the background so the child's pipe doesn't fill.
    struct WaitForCancelBridge;

    impl ProcessBridge for WaitForCancelBridge {
        fn run(
            self: Box<Self>,
            mut pipes: StdioPipes,
            mut cancel: CancelSignal,
        ) -> super::super::bridge::BridgeFuture {
            Box::pin(async move {
                // Drain stdout to keep `cat` happy.
                let drain = tokio::spawn(async move {
                    let mut buf = [0u8; 256];
                    loop {
                        match pipes.stdout.read(&mut buf).await {
                            Ok(0) | Err(_) => break,
                            Ok(_) => {}
                        }
                    }
                });
                // Kick one byte in so `cat` has something to echo; not essential.
                let _ = pipes.stdin.write_all(b".\n").await;
                let _ = cancel.wait_for(|v| *v).await;
                drop(pipes.stdin);
                let _ = drain.await;
                BridgeExit::Cancelled
            })
        }
    }

    /// Bridge that immediately returns `ProtocolError` — used to exercise
    /// the crash path without waiting for a real child to die.
    struct InstantErrorBridge;

    impl ProcessBridge for InstantErrorBridge {
        fn run(
            self: Box<Self>,
            _pipes: StdioPipes,
            _cancel: CancelSignal,
        ) -> super::super::bridge::BridgeFuture {
            Box::pin(async move { BridgeExit::ProtocolError(anyhow::anyhow!("synthetic failure")) })
        }
    }

    /// Bridge that immediately returns `Clean`.
    struct InstantCleanBridge;

    impl ProcessBridge for InstantCleanBridge {
        fn run(
            self: Box<Self>,
            _pipes: StdioPipes,
            _cancel: CancelSignal,
        ) -> super::super::bridge::BridgeFuture {
            Box::pin(async move { BridgeExit::Clean })
        }
    }

    #[cfg(unix)]
    struct CapturePidThenCleanBridge {
        pid: Arc<AtomicU32>,
    }

    #[cfg(unix)]
    impl ProcessBridge for CapturePidThenCleanBridge {
        fn run(
            self: Box<Self>,
            pipes: StdioPipes,
            _cancel: CancelSignal,
        ) -> super::super::bridge::BridgeFuture {
            Box::pin(async move {
                use tokio::io::AsyncBufReadExt;

                let mut line = String::new();
                let mut stdout = tokio::io::BufReader::new(pipes.stdout);
                stdout.read_line(&mut line).await.unwrap();
                self.pid
                    .store(line.trim().parse().unwrap(), Ordering::Release);
                BridgeExit::Clean
            })
        }
    }

    async fn wait_for_status(sup: &Arc<Supervisor>, id: ProcessId, target: ProcessStatus) {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        loop {
            let snap = sup.snapshot();
            if let Some(p) = snap.iter().find(|p| p.id == id) {
                if p.status == target {
                    return;
                }
            }
            if std::time::Instant::now() > deadline {
                let snap = sup.snapshot();
                panic!(
                    "timeout waiting for {:?}, got: {:?}",
                    target,
                    snap.iter().find(|p| p.id == id).map(|p| p.status)
                );
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
    }

    /// Wait until `id` disappears from the snapshot — i.e. auto-deregistered
    /// because it hit a terminal state under `RestartPolicy::Never`.
    async fn wait_for_absent(sup: &Arc<Supervisor>, id: ProcessId) {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        loop {
            if !sup.snapshot().iter().any(|p| p.id == id) {
                return;
            }
            if std::time::Instant::now() > deadline {
                panic!("timeout waiting for process {} to be deregistered", id);
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
    }

    fn cat_spec() -> SpawnSpec {
        SpawnSpec::new("cat")
    }

    async fn wait_for_count(counter: &AtomicUsize, target: usize) {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while counter.load(Ordering::Acquire) < target {
            if std::time::Instant::now() > deadline {
                panic!(
                    "timeout waiting for factory count {}, got {}",
                    target,
                    counter.load(Ordering::Acquire)
                );
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
    }

    fn insert_process_with_status(
        sup: &Arc<Supervisor>,
        id: ProcessId,
        status: ProcessStatus,
        factory: BridgeFactory,
    ) -> Arc<SupervisedProcess> {
        let proc = Arc::new(SupervisedProcess {
            id,
            kind: ProcessKind::ChannelPlugin,
            label: format!("test-{}", status.as_str()),
            spec: cat_spec(),
            policy: RestartPolicy::OnCrash {
                restart_delay: Duration::from_secs(30),
                watchdog: None,
            },
            factory,
            status: AtomicU8::new(status as u8),
            intent: AtomicU8::new(TransitionIntent::Stop as u8),
            reason: RwLock::new(String::new()),
            last_heartbeat_ts: AtomicU64::new(now_secs()),
            next_spawn_at: AtomicU64::new(now_secs() + 30),
            cancel_tx: RwLock::new(None),
            current_registry_id: parking_lot::Mutex::new(None),
            transition_lock: parking_lot::Mutex::new(()),
        });
        sup.processes.write().insert(id, Arc::clone(&proc));
        proc
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn register_runs_then_force_stop() {
        let registry = Arc::new(ChildRegistry::new());
        let sup = Supervisor::new(registry);

        let id = sup.register(
            ProcessKind::ChannelPlugin,
            "test-echo",
            cat_spec(),
            RestartPolicy::Never,
            Box::new(|| Box::new(WaitForCancelBridge)),
        );

        wait_for_status(&sup, id, ProcessStatus::Running).await;
        sup.force_stop(id).await.unwrap();
        // Never + Stopped auto-deregisters, so the snapshot entry vanishes.
        wait_for_absent(&sup, id).await;
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn protocol_error_with_never_policy_deregisters() {
        let registry = Arc::new(ChildRegistry::new());
        let sup = Supervisor::new(registry);

        // Subscribe BEFORE register so we capture the transient Stopped
        // event (notify_change fires before auto-deregister).
        let mut rx = sup.subscribe();

        let id = sup.register(
            ProcessKind::AcpAgent,
            "test-fail",
            cat_spec(),
            RestartPolicy::Never,
            Box::new(|| Box::new(InstantErrorBridge)),
        );

        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        let mut saw_stopped = false;
        while std::time::Instant::now() < deadline {
            match tokio::time::timeout(std::time::Duration::from_millis(200), rx.recv()).await {
                Ok(Ok(ev)) if ev.id == id && ev.status == ProcessStatus::Stopped => {
                    saw_stopped = true;
                    break;
                }
                _ => {}
            }
        }
        assert!(saw_stopped, "should have observed Stopped event");

        wait_for_absent(&sup, id).await;
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn on_crash_policy_marks_process_crashed() {
        let registry = Arc::new(ChildRegistry::new());
        let sup = Supervisor::new(registry);

        let id = sup.register(
            ProcessKind::ChannelPlugin,
            "test-crasher",
            cat_spec(),
            RestartPolicy::OnCrash {
                restart_delay: Duration::from_secs(30),
                watchdog: None,
            },
            Box::new(|| Box::new(InstantErrorBridge)),
        );

        wait_for_status(&sup, id, ProcessStatus::Crashed).await;
        let snap = sup.snapshot();
        let p = snap.iter().find(|p| p.id == id).unwrap();
        assert_eq!(p.status, ProcessStatus::Crashed);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn force_stop_without_bridge_stops_and_clears_pending_spawn() {
        let registry = Arc::new(ChildRegistry::new());
        let sup = Supervisor::new(Arc::clone(&registry));
        let counter = Arc::new(AtomicUsize::new(0));

        for (raw_id, status) in [ProcessStatus::NotStarted, ProcessStatus::Spawning]
            .into_iter()
            .enumerate()
        {
            let count = Arc::clone(&counter);
            let proc = insert_process_with_status(
                &sup,
                ProcessId(100 + raw_id as u64),
                status,
                Box::new(move || {
                    count.fetch_add(1, Ordering::Release);
                    Box::new(InstantCleanBridge)
                }),
            );
            let staged_generation = if matches!(status, ProcessStatus::Spawning) {
                Some(sup.spawn_child(&proc).await.unwrap())
            } else {
                None
            };

            sup.force_stop(proc.id).await.unwrap();

            assert_eq!(proc.status(), ProcessStatus::Stopped);
            assert_eq!(
                TransitionIntent::from_u8(proc.intent.load(Ordering::Acquire)),
                TransitionIntent::None
            );
            assert_eq!(proc.next_spawn_at.load(Ordering::Acquire), 0);
            assert!(proc.current_registry_id.lock().is_none());
            assert_eq!(registry.len(), 0);

            // A pending immediate-spawn task or tick must not resurrect a
            // process after force_stop won the transition.
            sup.begin_spawn(Arc::clone(&proc)).await;
            assert_eq!(counter.load(Ordering::Acquire), 0);
            drop(staged_generation);
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn force_stop_while_crashed_prevents_respawn_and_clears_intent() {
        let registry = Arc::new(ChildRegistry::new());
        let sup = Supervisor::new(registry);
        let counter = Arc::new(AtomicUsize::new(0));
        let count = Arc::clone(&counter);

        let id = sup.register(
            ProcessKind::ChannelPlugin,
            "crashed-stop",
            cat_spec(),
            RestartPolicy::OnCrash {
                restart_delay: Duration::from_secs(30),
                watchdog: None,
            },
            Box::new(move || {
                count.fetch_add(1, Ordering::Release);
                Box::new(InstantErrorBridge)
            }),
        );

        wait_for_status(&sup, id, ProcessStatus::Crashed).await;
        sup.force_stop(id).await.unwrap();
        wait_for_status(&sup, id, ProcessStatus::Stopped).await;

        sup.tick().await;
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert_eq!(counter.load(Ordering::Acquire), 1);

        // Starting it again must not leave the old Stop intent armed for the
        // next natural bridge exit.
        sup.force_start(id).unwrap();
        sup.tick().await;
        wait_for_count(&counter, 2).await;
        wait_for_status(&sup, id, ProcessStatus::Crashed).await;
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn force_restart_without_bridge_spawns_and_clears_stale_intent() {
        let registry = Arc::new(ChildRegistry::new());
        let sup = Supervisor::new(registry);

        for (raw_id, status) in [ProcessStatus::Crashed, ProcessStatus::Stopped]
            .into_iter()
            .enumerate()
        {
            let proc = insert_process_with_status(
                &sup,
                ProcessId(200 + raw_id as u64),
                status,
                Box::new(|| Box::new(WaitForCancelBridge)),
            );

            sup.force_restart(proc.id).await.unwrap();
            wait_for_status(&sup, proc.id, ProcessStatus::Running).await;
            assert_eq!(
                TransitionIntent::from_u8(proc.intent.load(Ordering::Acquire)),
                TransitionIntent::None
            );

            sup.force_stop(proc.id).await.unwrap();
            wait_for_status(&sup, proc.id, ProcessStatus::Stopped).await;
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn force_restart_while_spawning_reaps_staged_generation() {
        let registry = Arc::new(ChildRegistry::new());
        let sup = Supervisor::new(Arc::clone(&registry));
        let proc = insert_process_with_status(
            &sup,
            ProcessId(220),
            ProcessStatus::Spawning,
            Box::new(|| Box::new(WaitForCancelBridge)),
        );
        let staged_generation = sup.spawn_child(&proc).await.unwrap();

        sup.force_restart(proc.id).await.unwrap();

        assert_eq!(proc.status(), ProcessStatus::Crashed);
        assert_eq!(
            TransitionIntent::from_u8(proc.intent.load(Ordering::Acquire)),
            TransitionIntent::None
        );
        assert!(proc.next_spawn_at.load(Ordering::Acquire) <= now_secs());
        assert!(proc.current_registry_id.lock().is_none());
        assert_eq!(registry.len(), 0);
        drop(staged_generation);

        sup.tick().await;
        wait_for_status(&sup, proc.id, ProcessStatus::Running).await;
        sup.force_stop(proc.id).await.unwrap();
        wait_for_status(&sup, proc.id, ProcessStatus::Stopped).await;
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn force_start_does_not_wait_for_tick() {
        let registry = Arc::new(ChildRegistry::new());
        let sup = Supervisor::new(registry);
        let proc = insert_process_with_status(
            &sup,
            ProcessId(230),
            ProcessStatus::Stopped,
            Box::new(|| Box::new(WaitForCancelBridge)),
        );

        sup.force_start(proc.id).unwrap();
        wait_for_status(&sup, proc.id, ProcessStatus::Running).await;

        sup.force_stop(proc.id).await.unwrap();
        wait_for_status(&sup, proc.id, ProcessStatus::Stopped).await;
    }

    #[cfg(unix)]
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn bridge_exit_terminates_live_child_before_reaping() {
        let registry = Arc::new(ChildRegistry::new());
        let sup = Supervisor::new(Arc::clone(&registry));
        let pid = Arc::new(AtomicU32::new(0));
        let captured_pid = Arc::clone(&pid);

        let id = sup.register(
            ProcessKind::ChannelPlugin,
            "live-child-reap",
            SpawnSpec::new("sh").args(["-c", "echo $$; exec sleep 60"]),
            RestartPolicy::Never,
            Box::new(move || {
                Box::new(CapturePidThenCleanBridge {
                    pid: Arc::clone(&captured_pid),
                })
            }),
        );

        wait_for_absent(&sup, id).await;
        let child_pid = pid.load(Ordering::Acquire);
        assert_ne!(child_pid, 0, "bridge should capture the live child pid");
        assert_eq!(registry.len(), 0);

        use sysinfo::{Pid, ProcessRefreshKind, RefreshKind, System};
        let mut alive = true;
        for _ in 0..50 {
            let mut sys = System::new_with_specifics(
                RefreshKind::nothing().with_processes(ProcessRefreshKind::everything()),
            );
            sys.refresh_processes(sysinfo::ProcessesToUpdate::All, true);
            alive = sys.process(Pid::from_u32(child_pid)).is_some();
            if !alive {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }

        if alive {
            let _ = std::process::Command::new("kill")
                .args(["-9", &child_pid.to_string()])
                .status();
        }
        assert!(!alive, "bridge exit left child pid {child_pid} alive");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn subscribe_receives_events() {
        let registry = Arc::new(ChildRegistry::new());
        let sup = Supervisor::new(registry);
        let mut rx = sup.subscribe();

        let id = sup.register(
            ProcessKind::Tunnel,
            "test-events",
            cat_spec(),
            RestartPolicy::Never,
            Box::new(|| Box::new(InstantCleanBridge)),
        );

        // Expect at least one event where status == Stopped for this id.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        let mut saw_stopped = false;
        while std::time::Instant::now() < deadline {
            match tokio::time::timeout(std::time::Duration::from_millis(200), rx.recv()).await {
                Ok(Ok(ev)) => {
                    if ev.id == id && ev.status == ProcessStatus::Stopped {
                        saw_stopped = true;
                        break;
                    }
                }
                _ => {}
            }
        }
        assert!(saw_stopped, "should have observed Stopped event");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn shutdown_all_drains_but_keeps_loop_alive() {
        // Regression: `shutdown_all` used to take the tick-loop shutdown
        // sender and exit the loop, so after a daemon restart no new
        // OnCrash restarts or watchdog checks would ever fire. The fix
        // keeps the loop alive for the process lifetime and only drains
        // the process table.
        let registry = Arc::new(ChildRegistry::new());
        let sup = Supervisor::new(registry);
        sup.spawn_tick_loop();

        let id = sup.register(
            ProcessKind::ChannelPlugin,
            "pre-shutdown",
            cat_spec(),
            RestartPolicy::Never,
            Box::new(|| Box::new(WaitForCancelBridge)),
        );
        wait_for_status(&sup, id, ProcessStatus::Running).await;

        sup.shutdown_all().await;
        wait_for_absent(&sup, id).await;

        // Post-shutdown register: the tick loop must still be alive to
        // drive this new process to Running.
        let id2 = sup.register(
            ProcessKind::ChannelPlugin,
            "post-shutdown",
            cat_spec(),
            RestartPolicy::Never,
            Box::new(|| Box::new(WaitForCancelBridge)),
        );
        wait_for_status(&sup, id2, ProcessStatus::Running).await;
        sup.force_stop(id2).await.unwrap();
        wait_for_absent(&sup, id2).await;
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn bridge_exit_removes_from_registry() {
        // Regression: the supervisor used to discard the registry_id
        // returned by ChildRegistry::register on every spawn, leaving
        // the entry and a zombie process behind on every respawn.
        let registry = Arc::new(ChildRegistry::new());
        let sup = Supervisor::new(Arc::clone(&registry));

        let id = sup.register(
            ProcessKind::AcpAgent,
            "reap-test",
            cat_spec(),
            RestartPolicy::Never,
            Box::new(|| Box::new(InstantCleanBridge)),
        );

        wait_for_absent(&sup, id).await;

        // Registry must be empty — otherwise every respawn leaks a Child
        // handle (and, on Unix, an unreaped zombie). The reap happens on
        // a background task so give it a beat to settle.
        for _ in 0..50 {
            if registry.len() == 0 {
                return;
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
        panic!(
            "registry leaked {} entries after Never-policy exit",
            registry.len()
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn factory_is_called_per_spawn() {
        let registry = Arc::new(ChildRegistry::new());
        let sup = Supervisor::new(registry);

        let counter = Arc::new(AtomicUsize::new(0));
        let c = Arc::clone(&counter);

        // Factory increments a counter each time it's called.
        let factory: BridgeFactory = Box::new(move || {
            c.fetch_add(1, Ordering::Relaxed);
            Box::new(InstantCleanBridge)
        });

        let id = sup.register(
            ProcessKind::ChannelPlugin,
            "test-factory",
            cat_spec(),
            RestartPolicy::Never,
            factory,
        );

        // Never + Clean-exit auto-deregisters; by the time the entry is
        // gone, the factory has run exactly once.
        wait_for_absent(&sup, id).await;
        assert_eq!(counter.load(Ordering::Relaxed), 1);
    }
}
