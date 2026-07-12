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

/// Bridge that treats child stdout EOF as a clean protocol exit. This lets
/// tests exercise real OS exit statuses instead of synthesizing BridgeExit.
struct WaitForEofBridge;

impl ProcessBridge for WaitForEofBridge {
    fn run(
        self: Box<Self>,
        mut pipes: StdioPipes,
        _cancel: CancelSignal,
    ) -> super::super::bridge::BridgeFuture {
        Box::pin(async move {
            let mut sink = Vec::new();
            let _ = pipes.stdout.read_to_end(&mut sink).await;
            BridgeExit::Clean
        })
    }
}

struct DelayedCancelBridge {
    cancel_seen: Arc<tokio::sync::Semaphore>,
    release: Arc<tokio::sync::Semaphore>,
}

impl ProcessBridge for DelayedCancelBridge {
    fn run(
        self: Box<Self>,
        _pipes: StdioPipes,
        mut cancel: CancelSignal,
    ) -> super::super::bridge::BridgeFuture {
        Box::pin(async move {
            let _ = cancel.wait_for(|value| *value).await;
            self.cancel_seen.add_permits(1);
            let _ = self.release.acquire().await;
            BridgeExit::Cancelled
        })
    }
}

struct PendingBridge {
    dropped: Arc<AtomicUsize>,
}

impl ProcessBridge for PendingBridge {
    fn run(
        self: Box<Self>,
        _pipes: StdioPipes,
        _cancel: CancelSignal,
    ) -> super::super::bridge::BridgeFuture {
        struct DropGuard(Arc<AtomicUsize>);
        impl Drop for DropGuard {
            fn drop(&mut self) {
                self.0.fetch_add(1, Ordering::Release);
            }
        }

        Box::pin(async move {
            let _guard = DropGuard(Arc::clone(&self.dropped));
            std::future::pending::<()>().await;
            BridgeExit::Cancelled
        })
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
        reason: RwLock::new(String::new()),
        last_heartbeat_ts: AtomicU64::new(now_secs()),
        next_spawn_at: AtomicU64::new(now_secs() + 30),
        next_generation: AtomicU64::new(1),
        consecutive_failures: AtomicU32::new(0),
        stopping: std::sync::atomic::AtomicBool::new(false),
        stop_completed: tokio::sync::Notify::new(),
        active_generation: parking_lot::Mutex::new(None),
        pending_child: parking_lot::Mutex::new(None),
        transition_lock: parking_lot::Mutex::new(()),
        spawn_task: parking_lot::Mutex::new(None),
    });
    sup.processes.write().insert(id, Arc::clone(&proc));
    proc
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn register_runs_then_force_stop() {
    let registry = Arc::new(ChildRegistry::new());
    let sup = Supervisor::new(Arc::clone(&registry));

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
    assert_eq!(registry.len(), 0, "force_stop must reap before returning");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn force_stop_waits_for_old_bridge_before_allowing_new_generation() {
    let registry = Arc::new(ChildRegistry::new());
    let sup = Supervisor::new(Arc::clone(&registry));
    let factory_count = Arc::new(AtomicUsize::new(0));
    let cancel_seen = Arc::new(tokio::sync::Semaphore::new(0));
    let release = Arc::new(tokio::sync::Semaphore::new(0));
    let count_for_factory = Arc::clone(&factory_count);
    let cancel_for_factory = Arc::clone(&cancel_seen);
    let release_for_factory = Arc::clone(&release);

    let id = sup.register(
        ProcessKind::ChannelPlugin,
        "generation-stop-gate",
        cat_spec(),
        RestartPolicy::OnCrash {
            restart_delay: Duration::from_secs(30),
            watchdog: None,
        },
        Box::new(move || {
            if count_for_factory.fetch_add(1, Ordering::AcqRel) == 0 {
                Box::new(DelayedCancelBridge {
                    cancel_seen: Arc::clone(&cancel_for_factory),
                    release: Arc::clone(&release_for_factory),
                })
            } else {
                Box::new(WaitForCancelBridge)
            }
        }),
    );
    wait_for_status(&sup, id, ProcessStatus::Running).await;

    let sup_for_stop = Arc::clone(&sup);
    let stop = tokio::spawn(async move { sup_for_stop.force_stop(id).await });
    tokio::time::timeout(Duration::from_secs(1), cancel_seen.acquire())
        .await
        .expect("old bridge did not receive cancellation")
        .unwrap()
        .forget();

    // A concurrent start during the stop barrier must not publish generation 2.
    sup.force_start(id).unwrap();
    tokio::time::sleep(Duration::from_millis(30)).await;
    assert_eq!(factory_count.load(Ordering::Acquire), 1);
    assert_eq!(registry.len(), 0, "old child was not reaped promptly");

    release.add_permits(1);
    stop.await.unwrap().unwrap();
    sup.force_start(id).unwrap();
    wait_for_status(&sup, id, ProcessStatus::Running).await;
    assert_eq!(factory_count.load(Ordering::Acquire), 2);
    assert_eq!(registry.len(), 1, "new generation was not registered");

    sup.force_stop(id).await.unwrap();
    assert_eq!(registry.len(), 0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn force_stop_aborts_a_bridge_that_ignores_cancel_and_eof() {
    let registry = Arc::new(ChildRegistry::new());
    let sup = Supervisor::new(Arc::clone(&registry));
    let dropped = Arc::new(AtomicUsize::new(0));
    let dropped_for_factory = Arc::clone(&dropped);
    let id = sup.register(
        ProcessKind::ChannelPlugin,
        "stubborn-bridge",
        cat_spec(),
        RestartPolicy::Never,
        Box::new(move || {
            Box::new(PendingBridge {
                dropped: Arc::clone(&dropped_for_factory),
            })
        }),
    );
    wait_for_status(&sup, id, ProcessStatus::Running).await;

    tokio::time::timeout(Duration::from_secs(1), sup.force_stop(id))
        .await
        .expect("force_stop hung on a stubborn bridge")
        .unwrap();

    assert_eq!(dropped.load(Ordering::Acquire), 1);
    assert_eq!(registry.len(), 0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn unregister_removes_restartable_process() {
    let registry = Arc::new(ChildRegistry::new());
    let sup = Supervisor::new(Arc::clone(&registry));
    let id = sup.register(
        ProcessKind::ChannelPlugin,
        "restartable-remove",
        cat_spec(),
        RestartPolicy::OnCrash {
            restart_delay: Duration::from_secs(30),
            watchdog: None,
        },
        Box::new(|| Box::new(WaitForCancelBridge)),
    );

    wait_for_status(&sup, id, ProcessStatus::Running).await;
    sup.unregister(id).await.unwrap();

    wait_for_absent(&sup, id).await;
    assert_eq!(registry.len(), 0);
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

#[test]
fn restart_delay_backs_off_and_heartbeat_resets_the_budget() {
    let registry = Arc::new(ChildRegistry::new());
    let sup = Supervisor::new(registry);
    let proc = insert_process_with_status(
        &sup,
        ProcessId(90),
        ProcessStatus::Crashed,
        Box::new(|| Box::new(WaitForCancelBridge)),
    );

    assert_eq!(proc.next_restart_delay(), Duration::from_secs(30));
    assert_eq!(proc.next_restart_delay(), Duration::from_secs(60));
    assert_eq!(proc.next_restart_delay(), Duration::from_secs(120));
    assert_eq!(proc.next_restart_delay(), Duration::from_secs(240));
    assert_eq!(proc.next_restart_delay(), MAX_RESTART_DELAY);
    assert_eq!(proc.next_restart_delay(), MAX_RESTART_DELAY);

    sup.touch(proc.id);
    assert_eq!(proc.next_restart_delay(), Duration::from_secs(30));
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
        assert_eq!(proc.next_spawn_at.load(Ordering::Acquire), 0);
        assert!(proc.active_generation.lock().is_none());
        assert_eq!(registry.len(), 0);

        // A pending immediate-spawn task or tick must not resurrect a
        // process after force_stop won the transition.
        sup.begin_spawn(Arc::clone(&proc)).await;
        assert_eq!(counter.load(Ordering::Acquire), 0);
        drop(staged_generation);
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn force_stop_while_crashed_prevents_respawn() {
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

    // Starting it again must create a fresh generation that follows the
    // normal crash policy.
    sup.force_start(id).unwrap();
    sup.tick().await;
    wait_for_count(&counter, 2).await;
    wait_for_status(&sup, id, ProcessStatus::Crashed).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn force_restart_without_bridge_spawns_fresh_generation() {
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

        sup.force_stop(proc.id).await.unwrap();
        wait_for_status(&sup, proc.id, ProcessStatus::Stopped).await;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn late_tick_spawn_is_ignored_while_restart_owns_stop_barrier() {
    let registry = Arc::new(ChildRegistry::new());
    let sup = Supervisor::new(registry);
    let proc = insert_process_with_status(
        &sup,
        ProcessId(210),
        ProcessStatus::Crashed,
        Box::new(|| Box::new(WaitForCancelBridge)),
    );

    // Model a tick that retained this Crashed process immediately before a
    // concurrent restart acquired the lifecycle stop barrier.
    proc.stopping.store(true, Ordering::Release);
    sup.schedule_spawn(Arc::clone(&proc));

    assert!(proc.spawn_task.lock().is_none());
    assert_eq!(proc.status(), ProcessStatus::Crashed);

    sup.finish_stop(&proc);
    sup.schedule_spawn(Arc::clone(&proc));
    wait_for_status(&sup, proc.id, ProcessStatus::Running).await;
    sup.force_stop(proc.id).await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn published_spawn_is_ignored_after_restart_owns_stop_barrier() {
    let registry = Arc::new(ChildRegistry::new());
    let sup = Supervisor::new(Arc::clone(&registry));
    let proc = insert_process_with_status(
        &sup,
        ProcessId(215),
        ProcessStatus::Crashed,
        Box::new(|| Box::new(WaitForCancelBridge)),
    );

    // Model a spawn task that was published before restart acquired lifecycle
    // ownership, but did not begin consuming its stale Crashed state until now.
    proc.stopping.store(true, Ordering::Release);
    sup.begin_spawn(Arc::clone(&proc)).await;

    assert_eq!(proc.status(), ProcessStatus::Crashed);
    assert_eq!(registry.len(), 0);
    sup.finish_stop(&proc);
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
    assert!(proc.next_spawn_at.load(Ordering::Acquire) <= now_secs());
    assert!(proc.active_generation.lock().is_none());
    assert_eq!(registry.len(), 0);
    drop(staged_generation);

    sup.tick().await;
    wait_for_status(&sup, proc.id, ProcessStatus::Running).await;
    sup.force_stop(proc.id).await.unwrap();
    wait_for_status(&sup, proc.id, ProcessStatus::Stopped).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn force_restart_aborts_stubborn_bridge_before_publishing_replacement() {
    let registry = Arc::new(ChildRegistry::new());
    let sup = Supervisor::new(Arc::clone(&registry));
    let factory_count = Arc::new(AtomicUsize::new(0));
    let dropped = Arc::new(AtomicUsize::new(0));
    let count_for_factory = Arc::clone(&factory_count);
    let dropped_for_factory = Arc::clone(&dropped);

    let id = sup.register(
        ProcessKind::ChannelPlugin,
        "stubborn-restart",
        cat_spec(),
        RestartPolicy::OnCrash {
            restart_delay: Duration::ZERO,
            watchdog: None,
        },
        Box::new(move || {
            if count_for_factory.fetch_add(1, Ordering::AcqRel) == 0 {
                Box::new(PendingBridge {
                    dropped: Arc::clone(&dropped_for_factory),
                })
            } else {
                Box::new(WaitForCancelBridge)
            }
        }),
    );
    wait_for_status(&sup, id, ProcessStatus::Running).await;

    tokio::time::timeout(Duration::from_secs(1), sup.force_restart(id))
        .await
        .expect("force_restart hung on a stubborn bridge")
        .unwrap();
    wait_for_status(&sup, id, ProcessStatus::Running).await;

    assert_eq!(factory_count.load(Ordering::Acquire), 2);
    assert_eq!(dropped.load(Ordering::Acquire), 1);
    assert_eq!(registry.len(), 1, "only the replacement child may remain");

    sup.force_stop(id).await.unwrap();
    assert_eq!(registry.len(), 0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn force_stop_retries_after_a_concurrent_restart_owner() {
    let registry = Arc::new(ChildRegistry::new());
    let sup = Supervisor::new(Arc::clone(&registry));
    let cancel_seen = Arc::new(tokio::sync::Semaphore::new(0));
    let release = Arc::new(tokio::sync::Semaphore::new(0));
    let cancel_for_factory = Arc::clone(&cancel_seen);
    let release_for_factory = Arc::clone(&release);

    let id = sup.register(
        ProcessKind::ChannelPlugin,
        "restart-then-stop",
        cat_spec(),
        RestartPolicy::OnCrash {
            restart_delay: Duration::ZERO,
            watchdog: None,
        },
        Box::new(move || {
            Box::new(DelayedCancelBridge {
                cancel_seen: Arc::clone(&cancel_for_factory),
                release: Arc::clone(&release_for_factory),
            })
        }),
    );
    wait_for_status(&sup, id, ProcessStatus::Running).await;

    let restart_sup = Arc::clone(&sup);
    let restart = tokio::spawn(async move { restart_sup.force_restart(id).await });
    cancel_seen.acquire().await.unwrap().forget();

    let stop_sup = Arc::clone(&sup);
    let stop = tokio::spawn(async move { stop_sup.force_stop(id).await });
    tokio::task::yield_now().await;
    assert!(
        !stop.is_finished(),
        "stop returned while restart owned cleanup"
    );

    release.add_permits(1);
    restart.await.unwrap().unwrap();
    stop.await.unwrap().unwrap();

    wait_for_status(&sup, id, ProcessStatus::Stopped).await;
    assert!(sup.get_proc(id).unwrap().active_generation.lock().is_none());
    assert_eq!(registry.len(), 0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn shutdown_publishes_stopped_while_restart_owns_cleanup() {
    let registry = Arc::new(ChildRegistry::new());
    let sup = Supervisor::new(Arc::clone(&registry));
    let cancel_seen = Arc::new(tokio::sync::Semaphore::new(0));
    let release = Arc::new(tokio::sync::Semaphore::new(0));
    let cancel_for_factory = Arc::clone(&cancel_seen);
    let release_for_factory = Arc::clone(&release);

    let id = sup.register(
        ProcessKind::ChannelPlugin,
        "restart-then-shutdown",
        cat_spec(),
        RestartPolicy::OnCrash {
            restart_delay: Duration::ZERO,
            watchdog: None,
        },
        Box::new(move || {
            Box::new(DelayedCancelBridge {
                cancel_seen: Arc::clone(&cancel_for_factory),
                release: Arc::clone(&release_for_factory),
            })
        }),
    );
    wait_for_status(&sup, id, ProcessStatus::Running).await;

    let restart_sup = Arc::clone(&sup);
    let restart = tokio::spawn(async move { restart_sup.force_restart(id).await });
    cancel_seen.acquire().await.unwrap().forget();

    let shutdown_sup = Arc::clone(&sup);
    let shutdown = tokio::spawn(async move { shutdown_sup.shutdown_all().await });
    tokio::task::yield_now().await;
    release.add_permits(1);

    restart.await.unwrap().unwrap();
    shutdown.await.unwrap();
    tokio::time::sleep(Duration::from_millis(20)).await;

    assert!(sup.snapshot().is_empty());
    assert_eq!(registry.len(), 0, "restart spawned after supervisor drain");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn watchdog_escalates_stubborn_bridge_and_restarts_generation() {
    let registry = Arc::new(ChildRegistry::new());
    let sup = Supervisor::new(Arc::clone(&registry));
    let factory_count = Arc::new(AtomicUsize::new(0));
    let dropped = Arc::new(AtomicUsize::new(0));
    let count_for_factory = Arc::clone(&factory_count);
    let dropped_for_factory = Arc::clone(&dropped);

    let id = sup.register(
        ProcessKind::ChannelPlugin,
        "watchdog-stubborn",
        cat_spec(),
        RestartPolicy::OnCrash {
            restart_delay: Duration::ZERO,
            watchdog: Some(Duration::ZERO),
        },
        Box::new(move || {
            if count_for_factory.fetch_add(1, Ordering::AcqRel) == 0 {
                Box::new(PendingBridge {
                    dropped: Arc::clone(&dropped_for_factory),
                })
            } else {
                Box::new(WaitForCancelBridge)
            }
        }),
    );
    wait_for_status(&sup, id, ProcessStatus::Running).await;
    sup.get_proc(id)
        .unwrap()
        .last_heartbeat_ts
        .store(0, Ordering::Release);

    tokio::time::timeout(Duration::from_secs(1), sup.tick())
        .await
        .expect("watchdog restart hung");
    wait_for_status(&sup, id, ProcessStatus::Running).await;

    assert_eq!(factory_count.load(Ordering::Acquire), 2);
    assert_eq!(dropped.load(Ordering::Acquire), 1);
    assert_eq!(registry.len(), 1);

    sup.force_stop(id).await.unwrap();
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn real_zero_and_nonzero_exits_are_classified_and_respawned() {
    for (exit_code, expected_reason) in [(0, "process exited successfully"), (7, "exit status: 7")]
    {
        let registry = Arc::new(ChildRegistry::new());
        let sup = Supervisor::new(Arc::clone(&registry));
        let factory_count = Arc::new(AtomicUsize::new(0));
        let count_for_factory = Arc::clone(&factory_count);
        let marker = std::env::temp_dir().join(format!(
            "vibearound-supervisor-exit-{exit_code}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&marker);
        let script = format!(
            "if [ -f '{}' ]; then exec cat; else : > '{}'; exit {exit_code}; fi",
            marker.display(),
            marker.display()
        );

        let id = sup.register(
            ProcessKind::ChannelPlugin,
            format!("real-exit-{exit_code}"),
            SpawnSpec::new("sh").args(["-c", script.as_str()]),
            RestartPolicy::OnCrash {
                restart_delay: Duration::ZERO,
                watchdog: None,
            },
            Box::new(move || {
                if count_for_factory.fetch_add(1, Ordering::AcqRel) == 0 {
                    Box::new(WaitForEofBridge)
                } else {
                    Box::new(WaitForCancelBridge)
                }
            }),
        );

        wait_for_status(&sup, id, ProcessStatus::Crashed).await;
        let crashed = sup
            .snapshot()
            .into_iter()
            .find(|process| process.id == id)
            .unwrap();
        assert!(
            crashed.reason.contains(expected_reason),
            "unexpected reason for exit {exit_code}: {}",
            crashed.reason
        );
        assert_eq!(registry.len(), 0, "exited child was not reaped");

        sup.tick().await;
        wait_for_status(&sup, id, ProcessStatus::Running).await;
        assert_eq!(factory_count.load(Ordering::Acquire), 2);
        assert_eq!(registry.len(), 1);

        sup.force_stop(id).await.unwrap();
        let _ = std::fs::remove_file(marker);
    }
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

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn bridge_exit_terminates_helper_process_group() {
    let registry = Arc::new(ChildRegistry::new());
    let sup = Supervisor::new(Arc::clone(&registry));
    let helper_pid = Arc::new(AtomicU32::new(0));
    let captured_pid = Arc::clone(&helper_pid);

    let id = sup.register(
        ProcessKind::ChannelPlugin,
        "helper-tree-reap",
        SpawnSpec::new("sh").args(["-c", "sleep 60 & echo $!; wait"]),
        RestartPolicy::Never,
        Box::new(move || {
            Box::new(CapturePidThenCleanBridge {
                pid: Arc::clone(&captured_pid),
            })
        }),
    );

    wait_for_absent(&sup, id).await;
    let child_pid = helper_pid.load(Ordering::Acquire);
    assert_ne!(child_pid, 0, "bridge should capture the helper pid");
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
    assert!(!alive, "bridge exit left helper pid {child_pid} alive");
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
        if let Ok(Ok(ev)) =
            tokio::time::timeout(std::time::Duration::from_millis(200), rx.recv()).await
        {
            if ev.id == id && ev.status == ProcessStatus::Stopped {
                saw_stopped = true;
                break;
            }
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
async fn shutdown_all_joins_spawn_before_final_reap() {
    let registry = Arc::new(ChildRegistry::new());
    let sup = Supervisor::new(Arc::clone(&registry));
    let proc = insert_process_with_status(
        &sup,
        ProcessId(240),
        ProcessStatus::Spawning,
        Box::new(|| Box::new(WaitForCancelBridge)),
    );
    let release_spawn = Arc::new(tokio::sync::Barrier::new(2));
    let release_from_task = Arc::clone(&release_spawn);
    let sup_for_task = Arc::clone(&sup);
    let proc_for_task = Arc::clone(&proc);
    *proc.spawn_task.lock() = Some(tokio::spawn(async move {
        release_from_task.wait().await;
        let _staged_generation = sup_for_task.spawn_child(&proc_for_task).await.unwrap();
    }));

    let sup_for_shutdown = Arc::clone(&sup);
    let shutdown = tokio::spawn(async move {
        sup_for_shutdown.shutdown_all().await;
    });
    wait_for_absent(&sup, proc.id).await;

    // Let the already-owned spawn attempt register its child only after
    // shutdown has drained the public process table.
    release_spawn.wait().await;
    shutdown.await.unwrap();

    assert_eq!(proc.status(), ProcessStatus::Stopped);
    assert!(proc.active_generation.lock().is_none());
    assert_eq!(registry.len(), 0, "shutdown returned with a late child");
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
