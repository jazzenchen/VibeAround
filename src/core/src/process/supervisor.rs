//! Process supervision with one lifecycle owner task per child process.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use parking_lot::RwLock;
use tokio::process::Command;
use tokio::sync::{broadcast, oneshot, watch};

use crate::proc_log;
use crate::process::bridge::{BridgeExit, BridgeFactory, StdioPipes};
use crate::process::env;
use crate::process::error::{ProcessError, ProcessResult};
use crate::process::kill;
use crate::process::registry::{ChildRegistry, ProcessKind};

mod generation;
mod model;

use generation::ProcessOwner;
use model::{ProcessCommand, ProcessState, SupervisedProcess};
pub use model::{ProcessEvent, ProcessSnapshot, ProcessStatus, RestartPolicy, SpawnSpec};

pub const TICK_INTERVAL: Duration = Duration::from_secs(5);
#[cfg(not(test))]
const BRIDGE_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(2);
#[cfg(test)]
const BRIDGE_SHUTDOWN_TIMEOUT: Duration = Duration::from_millis(100);
const MAX_RESTART_DELAY: Duration = Duration::from_secs(300);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ProcessId(pub u64);

impl std::fmt::Display for ProcessId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

pub struct Supervisor {
    registry: Arc<ChildRegistry>,
    processes: RwLock<HashMap<ProcessId, Arc<SupervisedProcess>>>,
    next_id: AtomicU64,
    change_tx: broadcast::Sender<ProcessEvent>,
    tick_loop_started: AtomicBool,
}

impl Supervisor {
    pub fn new(registry: Arc<ChildRegistry>) -> Arc<Self> {
        let (change_tx, _) = broadcast::channel(64);
        Arc::new(Self {
            registry,
            processes: RwLock::new(HashMap::new()),
            next_id: AtomicU64::new(1),
            change_tx,
            tick_loop_started: AtomicBool::new(false),
        })
    }

    pub fn global() -> Arc<Self> {
        use std::sync::OnceLock;
        static INSTANCE: OnceLock<Arc<Supervisor>> = OnceLock::new();
        Arc::clone(INSTANCE.get_or_init(|| {
            let supervisor = Supervisor::new(ChildRegistry::global());
            supervisor.spawn_tick_loop();
            supervisor
        }))
    }

    pub fn register(
        self: &Arc<Self>,
        kind: ProcessKind,
        label: impl Into<String>,
        spec: SpawnSpec,
        policy: RestartPolicy,
        factory: BridgeFactory,
    ) -> ProcessId {
        let id = ProcessId(self.next_id.fetch_add(1, Ordering::Relaxed));
        self.insert_process(
            id,
            kind,
            label.into(),
            spec,
            policy,
            factory,
            ProcessStatus::NotStarted,
            true,
        );
        id
    }

    #[allow(clippy::too_many_arguments)]
    fn insert_process(
        self: &Arc<Self>,
        id: ProcessId,
        kind: ProcessKind,
        label: String,
        spec: SpawnSpec,
        policy: RestartPolicy,
        factory: BridgeFactory,
        initial_status: ProcessStatus,
        start_immediately: bool,
    ) -> Arc<SupervisedProcess> {
        let state = ProcessState {
            status: initial_status,
            reason: String::new(),
        };
        let (state_tx, _) = watch::channel(state.clone());
        let (command_tx, command_rx) = tokio::sync::mpsc::unbounded_channel();
        let process = Arc::new(SupervisedProcess {
            id,
            kind,
            label,
            command_tx: command_tx.clone(),
            state_tx,
        });
        self.processes.write().insert(id, Arc::clone(&process));
        self.notify_change(&process);
        let owner = ProcessOwner::new(
            Arc::downgrade(self),
            Arc::clone(&process),
            Arc::clone(&self.registry),
            spec,
            policy,
            factory,
            command_tx,
            command_rx,
            state,
        );
        tokio::spawn(owner.run(start_immediately));
        process
    }

    pub fn touch(&self, id: ProcessId) {
        if let Ok(process) = self.get_process(id) {
            let _ = process.command_tx.send(ProcessCommand::Touch);
        }
    }

    pub async fn force_stop(&self, id: ProcessId) -> ProcessResult<()> {
        let process = self.get_process(id)?;
        self.stop_process(&process, "stopped by user").await;
        Ok(())
    }

    async fn stop_process(&self, process: &SupervisedProcess, reason: &str) {
        let (reply, done) = oneshot::channel();
        if process
            .command_tx
            .send(ProcessCommand::Stop {
                reason: reason.to_string(),
                reply,
            })
            .is_ok()
        {
            let _ = done.await;
        }
    }

    pub async fn unregister(&self, id: ProcessId) -> ProcessResult<()> {
        let process = self
            .processes
            .write()
            .remove(&id)
            .ok_or_else(|| unknown_process(id))?;
        self.shutdown_process(&process, "unregistered").await;
        Ok(())
    }

    pub async fn force_restart(&self, id: ProcessId) -> ProcessResult<()> {
        self.restart(id, false).await
    }

    async fn restart(&self, id: ProcessId, apply_backoff: bool) -> ProcessResult<()> {
        let process = self.get_process(id)?;
        let (reply, done) = oneshot::channel();
        if process
            .command_tx
            .send(ProcessCommand::Restart {
                apply_backoff,
                reply,
            })
            .is_ok()
        {
            let _ = done.await;
        }
        Ok(())
    }

    pub fn force_start(&self, id: ProcessId) -> ProcessResult<()> {
        let process = self.get_process(id)?;
        let _ = process.command_tx.send(ProcessCommand::Start);
        Ok(())
    }

    pub fn snapshot(&self) -> Vec<ProcessSnapshot> {
        let mut snapshots = self
            .processes
            .read()
            .values()
            .map(|process| ProcessSnapshot {
                id: process.id,
                kind: process.kind,
                label: process.label.clone(),
                status: process.status(),
                reason: process.reason(),
            })
            .collect::<Vec<_>>();
        snapshots.sort_by(|left, right| left.label.cmp(&right.label));
        snapshots
    }

    pub fn subscribe(&self) -> broadcast::Receiver<ProcessEvent> {
        self.change_tx.subscribe()
    }

    pub fn spawn_tick_loop(self: &Arc<Self>) {
        if self
            .tick_loop_started
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return;
        }
        let supervisor = Arc::clone(self);
        tokio::spawn(async move { supervisor.run_tick_loop().await });
    }

    async fn run_tick_loop(self: Arc<Self>) {
        let mut ticker = tokio::time::interval(TICK_INTERVAL);
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        ticker.tick().await;
        loop {
            ticker.tick().await;
            self.tick().await;
        }
    }

    async fn tick(&self) {
        let now = now_secs();
        let processes = self.processes.read().values().cloned().collect::<Vec<_>>();
        for process in processes {
            let _ = process.command_tx.send(ProcessCommand::Tick(now));
        }
        tokio::task::yield_now().await;
    }

    pub async fn shutdown_all(&self) {
        let processes = self
            .processes
            .write()
            .drain()
            .map(|(_, process)| process)
            .collect::<Vec<_>>();
        let mut completions = Vec::with_capacity(processes.len());
        for process in processes {
            let (reply, done) = oneshot::channel();
            let _ = process.command_tx.send(ProcessCommand::Shutdown {
                reason: "supervisor shutdown".to_string(),
                reply,
            });
            completions.push(done);
        }
        for completion in completions {
            let _ = completion.await;
        }
    }

    fn get_process(&self, id: ProcessId) -> ProcessResult<Arc<SupervisedProcess>> {
        self.processes
            .read()
            .get(&id)
            .cloned()
            .ok_or_else(|| unknown_process(id))
    }

    async fn shutdown_process(&self, process: &SupervisedProcess, reason: &str) {
        let (reply, done) = oneshot::channel();
        if process
            .command_tx
            .send(ProcessCommand::Shutdown {
                reason: reason.to_string(),
                reply,
            })
            .is_ok()
        {
            let _ = done.await;
        }
    }

    fn remove_process(&self, process: &Arc<SupervisedProcess>) {
        let mut processes = self.processes.write();
        if processes
            .get(&process.id)
            .is_some_and(|current| Arc::ptr_eq(current, process))
        {
            processes.remove(&process.id);
        }
    }

    fn notify_change(&self, process: &SupervisedProcess) {
        let _ = self.change_tx.send(ProcessEvent {
            id: process.id,
            kind: process.kind,
            status: process.status(),
        });
    }
}

fn unknown_process(id: ProcessId) -> ProcessError {
    ProcessError::UnknownProcess {
        label: format!("#{}", id.0),
    }
}

fn now_secs() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests;
