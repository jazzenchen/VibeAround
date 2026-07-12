//! Process supervision with one lifecycle owner task per child process.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use tokio::process::Command;
use tokio::sync::{broadcast, mpsc, oneshot, watch};

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
const CHILD_EXIT_OBSERVATION_TIMEOUT: Duration = Duration::from_millis(50);
const MAX_RESTART_DELAY: Duration = Duration::from_secs(300);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ProcessId(pub u64);

impl std::fmt::Display for ProcessId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

pub struct Supervisor {
    manager_tx: mpsc::UnboundedSender<ManagerCommand>,
    snapshots: watch::Receiver<Vec<ProcessSnapshot>>,
    next_id: AtomicU64,
    change_tx: broadcast::Sender<ProcessEvent>,
    tick_loop_started: AtomicBool,
}

struct ProcessRegistration {
    id: ProcessId,
    kind: ProcessKind,
    label: String,
    spec: SpawnSpec,
    policy: RestartPolicy,
    factory: BridgeFactory,
}

enum ManagerCommand {
    Register(ProcessRegistration),
    Touch(ProcessId),
    Stop {
        id: ProcessId,
        reason: String,
        reply: oneshot::Sender<ProcessResult<()>>,
    },
    Unregister {
        id: ProcessId,
        reply: oneshot::Sender<ProcessResult<()>>,
    },
    Restart {
        id: ProcessId,
        apply_backoff: bool,
        reply: oneshot::Sender<ProcessResult<()>>,
    },
    Start {
        id: ProcessId,
        reply: oneshot::Sender<ProcessResult<()>>,
    },
    Tick {
        now: u64,
        reply: oneshot::Sender<()>,
    },
    ShutdownAll(oneshot::Sender<()>),
    ProcessChanged(Arc<SupervisedProcess>),
    Remove(Arc<SupervisedProcess>),
}

struct ProcessManager {
    registry: Arc<ChildRegistry>,
    processes: HashMap<ProcessId, Arc<SupervisedProcess>>,
    command_tx: mpsc::UnboundedSender<ManagerCommand>,
    command_rx: mpsc::UnboundedReceiver<ManagerCommand>,
    snapshot_tx: watch::Sender<Vec<ProcessSnapshot>>,
    change_tx: broadcast::Sender<ProcessEvent>,
}

impl Supervisor {
    pub fn new(registry: Arc<ChildRegistry>) -> Arc<Self> {
        let (change_tx, _) = broadcast::channel(64);
        let (snapshot_tx, snapshots) = watch::channel(Vec::new());
        let (manager_tx, manager_rx) = mpsc::unbounded_channel();
        let supervisor = Arc::new(Self {
            manager_tx: manager_tx.clone(),
            snapshots,
            next_id: AtomicU64::new(1),
            change_tx: change_tx.clone(),
            tick_loop_started: AtomicBool::new(false),
        });
        tokio::spawn(
            ProcessManager {
                registry,
                processes: HashMap::new(),
                command_tx: manager_tx,
                command_rx: manager_rx,
                snapshot_tx,
                change_tx,
            }
            .run(),
        );
        supervisor
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
        let _ = self
            .manager_tx
            .send(ManagerCommand::Register(ProcessRegistration {
                id,
                kind,
                label: label.into(),
                spec,
                policy,
                factory,
            }));
        id
    }

    pub fn touch(&self, id: ProcessId) {
        let _ = self.manager_tx.send(ManagerCommand::Touch(id));
    }

    pub async fn force_stop(&self, id: ProcessId) -> ProcessResult<()> {
        let (reply, done) = oneshot::channel();
        self.manager_tx
            .send(ManagerCommand::Stop {
                id,
                reason: "stopped by user".to_string(),
                reply,
            })
            .map_err(|_| unknown_process(id))?;
        done.await.unwrap_or_else(|_| Err(unknown_process(id)))
    }

    pub async fn unregister(&self, id: ProcessId) -> ProcessResult<()> {
        let (reply, done) = oneshot::channel();
        self.manager_tx
            .send(ManagerCommand::Unregister { id, reply })
            .map_err(|_| unknown_process(id))?;
        done.await.unwrap_or_else(|_| Err(unknown_process(id)))
    }

    pub async fn force_restart(&self, id: ProcessId) -> ProcessResult<()> {
        self.restart(id, false).await
    }

    async fn restart(&self, id: ProcessId, apply_backoff: bool) -> ProcessResult<()> {
        let (reply, done) = oneshot::channel();
        self.manager_tx
            .send(ManagerCommand::Restart {
                id,
                apply_backoff,
                reply,
            })
            .map_err(|_| unknown_process(id))?;
        done.await.unwrap_or_else(|_| Err(unknown_process(id)))
    }

    pub async fn force_start(&self, id: ProcessId) -> ProcessResult<()> {
        let (reply, done) = oneshot::channel();
        self.manager_tx
            .send(ManagerCommand::Start { id, reply })
            .map_err(|_| unknown_process(id))?;
        done.await.unwrap_or_else(|_| Err(unknown_process(id)))
    }

    pub fn snapshot(&self) -> Vec<ProcessSnapshot> {
        self.snapshots.borrow().clone()
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
        let (reply, done) = oneshot::channel();
        let _ = self.manager_tx.send(ManagerCommand::Tick {
            now: now_secs(),
            reply,
        });
        let _ = done.await;
        tokio::task::yield_now().await;
    }

    pub async fn shutdown_all(&self) {
        let (reply, done) = oneshot::channel();
        let _ = self.manager_tx.send(ManagerCommand::ShutdownAll(reply));
        let _ = done.await;
    }
}

impl ProcessManager {
    async fn run(mut self) {
        while let Some(command) = self.command_rx.recv().await {
            match command {
                ManagerCommand::Register(registration) => self.register(registration),
                ManagerCommand::Touch(id) => {
                    if let Some(process) = self.processes.get(&id) {
                        let _ = process.command_tx.send(ProcessCommand::Touch);
                    }
                }
                ManagerCommand::Stop { id, reason, reply } => {
                    let Some(process) = self.processes.get(&id) else {
                        let _ = reply.send(Err(unknown_process(id)));
                        continue;
                    };
                    let (process_reply, done) = oneshot::channel();
                    if process
                        .command_tx
                        .send(ProcessCommand::Stop {
                            reason,
                            reply: process_reply,
                        })
                        .is_err()
                    {
                        let _ = reply.send(Err(unknown_process(id)));
                    } else {
                        tokio::spawn(async move {
                            let _ = done.await;
                            let _ = reply.send(Ok(()));
                        });
                    }
                }
                ManagerCommand::Unregister { id, reply } => {
                    let Some(process) = self.processes.remove(&id) else {
                        let _ = reply.send(Err(unknown_process(id)));
                        continue;
                    };
                    self.publish_snapshots();
                    let (process_reply, done) = oneshot::channel();
                    let _ = process.command_tx.send(ProcessCommand::Shutdown {
                        reason: "unregistered".to_string(),
                        reply: process_reply,
                    });
                    tokio::spawn(async move {
                        let _ = done.await;
                        let _ = reply.send(Ok(()));
                    });
                }
                ManagerCommand::Restart {
                    id,
                    apply_backoff,
                    reply,
                } => {
                    let Some(process) = self.processes.get(&id) else {
                        let _ = reply.send(Err(unknown_process(id)));
                        continue;
                    };
                    let (process_reply, done) = oneshot::channel();
                    if process
                        .command_tx
                        .send(ProcessCommand::Restart {
                            apply_backoff,
                            reply: process_reply,
                        })
                        .is_err()
                    {
                        let _ = reply.send(Err(unknown_process(id)));
                    } else {
                        tokio::spawn(async move {
                            let _ = done.await;
                            let _ = reply.send(Ok(()));
                        });
                    }
                }
                ManagerCommand::Start { id, reply } => {
                    let result = self
                        .processes
                        .get(&id)
                        .ok_or_else(|| unknown_process(id))
                        .and_then(|process| {
                            process
                                .command_tx
                                .send(ProcessCommand::Start)
                                .map_err(|_| unknown_process(id))
                        });
                    let _ = reply.send(result);
                }
                ManagerCommand::Tick { now, reply } => {
                    for process in self.processes.values() {
                        let _ = process.command_tx.send(ProcessCommand::Tick(now));
                    }
                    let _ = reply.send(());
                }
                ManagerCommand::ShutdownAll(reply) => {
                    let processes = self.processes.drain().map(|(_, process)| process);
                    let mut completions = Vec::new();
                    for process in processes {
                        let (process_reply, done) = oneshot::channel();
                        let _ = process.command_tx.send(ProcessCommand::Shutdown {
                            reason: "supervisor shutdown".to_string(),
                            reply: process_reply,
                        });
                        completions.push(done);
                    }
                    self.publish_snapshots();
                    tokio::spawn(async move {
                        for completion in completions {
                            let _ = completion.await;
                        }
                        let _ = reply.send(());
                    });
                }
                ManagerCommand::ProcessChanged(process) => {
                    if self
                        .processes
                        .get(&process.id)
                        .is_some_and(|current| Arc::ptr_eq(current, &process))
                    {
                        let _ = self.change_tx.send(ProcessEvent {
                            id: process.id,
                            kind: process.kind,
                            status: process.status(),
                        });
                        self.publish_snapshots();
                    }
                }
                ManagerCommand::Remove(process) => {
                    if self
                        .processes
                        .get(&process.id)
                        .is_some_and(|current| Arc::ptr_eq(current, &process))
                    {
                        self.processes.remove(&process.id);
                        self.publish_snapshots();
                    }
                }
            }
        }
    }

    fn register(&mut self, registration: ProcessRegistration) {
        let ProcessRegistration {
            id,
            kind,
            label,
            spec,
            policy,
            factory,
        } = registration;
        let state = ProcessState {
            status: ProcessStatus::NotStarted,
            reason: String::new(),
        };
        let (state_tx, _) = watch::channel(state.clone());
        let (command_tx, command_rx) = mpsc::unbounded_channel();
        let process = Arc::new(SupervisedProcess {
            id,
            kind,
            label,
            command_tx: command_tx.clone(),
            state_tx,
        });
        self.processes.insert(id, Arc::clone(&process));
        let _ = self.change_tx.send(ProcessEvent {
            id,
            kind,
            status: ProcessStatus::NotStarted,
        });
        self.publish_snapshots();
        tokio::spawn(
            ProcessOwner::new(
                self.command_tx.clone(),
                process,
                Arc::clone(&self.registry),
                spec,
                policy,
                factory,
                command_tx,
                command_rx,
                state,
            )
            .run(true),
        );
    }

    fn publish_snapshots(&self) {
        let mut snapshots = self
            .processes
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
        self.snapshot_tx.send_replace(snapshots);
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
