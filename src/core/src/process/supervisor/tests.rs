use super::*;
use crate::process::bridge::{BridgeExit, CancelSignal, ProcessBridge, StdioPipes};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

struct WaitForCancelBridge;

impl ProcessBridge for WaitForCancelBridge {
    fn run(
        self: Box<Self>,
        mut pipes: StdioPipes,
        mut cancel: CancelSignal,
    ) -> super::super::bridge::BridgeFuture {
        Box::pin(async move {
            let drain = tokio::spawn(async move {
                let mut buffer = [0_u8; 256];
                while matches!(pipes.stdout.read(&mut buffer).await, Ok(read) if read > 0) {}
            });
            let _ = pipes.stdin.write_all(b".\n").await;
            let _ = cancel.wait_for(|cancelled| *cancelled).await;
            drop(pipes.stdin);
            let _ = drain.await;
            BridgeExit::Cancelled
        })
    }
}

struct InstantErrorBridge;

impl ProcessBridge for InstantErrorBridge {
    fn run(
        self: Box<Self>,
        _pipes: StdioPipes,
        _cancel: CancelSignal,
    ) -> super::super::bridge::BridgeFuture {
        Box::pin(async { BridgeExit::ProtocolError(anyhow::anyhow!("synthetic failure")) })
    }
}

struct WaitForEofBridge;

impl ProcessBridge for WaitForEofBridge {
    fn run(
        self: Box<Self>,
        mut pipes: StdioPipes,
        _cancel: CancelSignal,
    ) -> super::super::bridge::BridgeFuture {
        Box::pin(async move {
            let mut output = Vec::new();
            let _ = pipes.stdout.read_to_end(&mut output).await;
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
            let _ = cancel.wait_for(|cancelled| *cancelled).await;
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
            let _guard = DropGuard(self.dropped);
            std::future::pending::<()>().await;
            BridgeExit::Cancelled
        })
    }
}

fn cat_spec() -> SpawnSpec {
    SpawnSpec::new("cat")
}

#[tokio::test]
async fn force_restart_reports_spawn_failure() {
    let registry = Arc::new(ChildRegistry::new());
    let supervisor = Supervisor::new(registry);
    let id = supervisor.register(
        ProcessKind::ChannelPlugin,
        "missing-program",
        SpawnSpec::new("vibearound-program-that-does-not-exist"),
        RestartPolicy::OnCrash {
            restart_delay: Duration::from_secs(30),
            watchdog: None,
        },
        Box::new(|| Box::new(WaitForCancelBridge)),
    );
    wait_for_status(&supervisor, id, ProcessStatus::Crashed).await;

    let error = supervisor.force_restart(id).await.unwrap_err();

    assert!(error.to_string().contains("failed to spawn"));
}

async fn wait_for_status(supervisor: &Supervisor, id: ProcessId, expected: ProcessStatus) {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    loop {
        if supervisor
            .snapshot()
            .iter()
            .find(|process| process.id == id)
            .is_some_and(|process| process.status == expected)
        {
            return;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "timeout waiting for {expected:?}: {:?}",
            supervisor.snapshot()
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

async fn wait_for_absent(supervisor: &Supervisor, id: ProcessId) {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    while supervisor.snapshot().iter().any(|process| process.id == id) {
        assert!(tokio::time::Instant::now() < deadline);
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

async fn wait_for_count(counter: &AtomicUsize, expected: usize) {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    while counter.load(Ordering::Acquire) < expected {
        assert!(tokio::time::Instant::now() < deadline);
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn register_runs_then_force_stop() {
    let registry = Arc::new(ChildRegistry::new());
    let supervisor = Supervisor::new(Arc::clone(&registry));
    let id = supervisor.register(
        ProcessKind::ChannelPlugin,
        "cat-test",
        cat_spec(),
        RestartPolicy::OnCrash {
            restart_delay: Duration::from_secs(30),
            watchdog: None,
        },
        Box::new(|| Box::new(WaitForCancelBridge)),
    );
    wait_for_status(&supervisor, id, ProcessStatus::Running).await;

    supervisor.force_stop(id).await.unwrap();

    wait_for_status(&supervisor, id, ProcessStatus::Stopped).await;
    assert_eq!(registry.len(), 0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn lifecycle_commands_are_consumed_in_order() {
    let registry = Arc::new(ChildRegistry::new());
    let supervisor = Supervisor::new(Arc::clone(&registry));
    let cancel_seen = Arc::new(tokio::sync::Semaphore::new(0));
    let release = Arc::new(tokio::sync::Semaphore::new(0));
    let factory_count = Arc::new(AtomicUsize::new(0));
    let id = supervisor.register(
        ProcessKind::ChannelPlugin,
        "ordered-lifecycle",
        cat_spec(),
        RestartPolicy::OnCrash {
            restart_delay: Duration::ZERO,
            watchdog: None,
        },
        {
            let cancel_seen = Arc::clone(&cancel_seen);
            let release = Arc::clone(&release);
            let factory_count = Arc::clone(&factory_count);
            Box::new(move || {
                factory_count.fetch_add(1, Ordering::AcqRel);
                Box::new(DelayedCancelBridge {
                    cancel_seen: Arc::clone(&cancel_seen),
                    release: Arc::clone(&release),
                })
            })
        },
    );
    wait_for_status(&supervisor, id, ProcessStatus::Running).await;

    let restarting = {
        let supervisor = Arc::clone(&supervisor);
        tokio::spawn(async move { supervisor.force_restart(id).await })
    };
    cancel_seen.acquire().await.unwrap().forget();
    let stopping = {
        let supervisor = Arc::clone(&supervisor);
        tokio::spawn(async move { supervisor.force_stop(id).await })
    };
    release.add_permits(1);
    restarting.await.unwrap().unwrap();
    cancel_seen.acquire().await.unwrap().forget();
    release.add_permits(1);
    stopping.await.unwrap().unwrap();

    wait_for_status(&supervisor, id, ProcessStatus::Stopped).await;
    assert_eq!(factory_count.load(Ordering::Acquire), 2);
    assert_eq!(registry.len(), 0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn force_stop_aborts_a_stubborn_bridge() {
    let registry = Arc::new(ChildRegistry::new());
    let supervisor = Supervisor::new(Arc::clone(&registry));
    let dropped = Arc::new(AtomicUsize::new(0));
    let id = supervisor.register(
        ProcessKind::ChannelPlugin,
        "stubborn",
        cat_spec(),
        RestartPolicy::OnCrash {
            restart_delay: Duration::ZERO,
            watchdog: None,
        },
        {
            let dropped = Arc::clone(&dropped);
            Box::new(move || {
                Box::new(PendingBridge {
                    dropped: Arc::clone(&dropped),
                })
            })
        },
    );
    wait_for_status(&supervisor, id, ProcessStatus::Running).await;

    tokio::time::timeout(Duration::from_secs(1), supervisor.force_stop(id))
        .await
        .expect("stubborn bridge stop hung")
        .unwrap();

    assert_eq!(dropped.load(Ordering::Acquire), 1);
    assert_eq!(registry.len(), 0);
}

#[tokio::test]
async fn unregister_stops_and_removes_restartable_process() {
    let registry = Arc::new(ChildRegistry::new());
    let supervisor = Supervisor::new(Arc::clone(&registry));
    let id = supervisor.register(
        ProcessKind::ChannelPlugin,
        "unregister",
        cat_spec(),
        RestartPolicy::OnCrash {
            restart_delay: Duration::ZERO,
            watchdog: None,
        },
        Box::new(|| Box::new(WaitForCancelBridge)),
    );
    wait_for_status(&supervisor, id, ProcessStatus::Running).await;

    supervisor.unregister(id).await.unwrap();

    assert!(!supervisor.snapshot().iter().any(|process| process.id == id));
    assert_eq!(registry.len(), 0);
}

#[tokio::test]
async fn never_policy_auto_deregisters_terminal_process() {
    let registry = Arc::new(ChildRegistry::new());
    let supervisor = Supervisor::new(Arc::clone(&registry));
    let id = supervisor.register(
        ProcessKind::AcpAgent,
        "one-shot",
        cat_spec(),
        RestartPolicy::Never,
        Box::new(|| Box::new(InstantErrorBridge)),
    );

    wait_for_absent(&supervisor, id).await;
    assert_eq!(registry.len(), 0);
}

#[tokio::test]
async fn crash_policy_marks_process_crashed() {
    let registry = Arc::new(ChildRegistry::new());
    let supervisor = Supervisor::new(registry);
    let id = supervisor.register(
        ProcessKind::ChannelPlugin,
        "crash",
        cat_spec(),
        RestartPolicy::OnCrash {
            restart_delay: Duration::from_secs(30),
            watchdog: None,
        },
        Box::new(|| Box::new(InstantErrorBridge)),
    );

    wait_for_status(&supervisor, id, ProcessStatus::Crashed).await;
    let reason = supervisor
        .snapshot()
        .into_iter()
        .find(|process| process.id == id)
        .unwrap()
        .reason;
    assert!(reason.contains("synthetic failure"));
    supervisor.force_stop(id).await.unwrap();
}

#[tokio::test]
async fn stopped_process_does_not_respawn_until_started() {
    let registry = Arc::new(ChildRegistry::new());
    let supervisor = Supervisor::new(registry);
    let count = Arc::new(AtomicUsize::new(0));
    let id = supervisor.register(
        ProcessKind::ChannelPlugin,
        "start-stop",
        cat_spec(),
        RestartPolicy::OnCrash {
            restart_delay: Duration::ZERO,
            watchdog: None,
        },
        {
            let count = Arc::clone(&count);
            Box::new(move || {
                count.fetch_add(1, Ordering::AcqRel);
                Box::new(WaitForCancelBridge)
            })
        },
    );
    wait_for_status(&supervisor, id, ProcessStatus::Running).await;
    supervisor.force_stop(id).await.unwrap();
    supervisor.tick().await;
    tokio::time::sleep(Duration::from_millis(20)).await;
    assert_eq!(count.load(Ordering::Acquire), 1);

    supervisor.force_start(id).await.unwrap();
    wait_for_count(&count, 2).await;
    wait_for_status(&supervisor, id, ProcessStatus::Running).await;
    supervisor.force_stop(id).await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn watchdog_restart_does_not_block_other_processes() {
    let registry = Arc::new(ChildRegistry::new());
    let supervisor = Supervisor::new(registry);
    let slow_cancel = Arc::new(tokio::sync::Semaphore::new(0));
    let slow_release = Arc::new(tokio::sync::Semaphore::new(0));
    let slow_id = supervisor.register(
        ProcessKind::ChannelPlugin,
        "slow-watchdog",
        cat_spec(),
        RestartPolicy::OnCrash {
            restart_delay: Duration::ZERO,
            watchdog: Some(Duration::ZERO),
        },
        {
            let cancel_seen = Arc::clone(&slow_cancel);
            let release = Arc::clone(&slow_release);
            Box::new(move || {
                Box::new(DelayedCancelBridge {
                    cancel_seen: Arc::clone(&cancel_seen),
                    release: Arc::clone(&release),
                })
            })
        },
    );
    let fast_count = Arc::new(AtomicUsize::new(0));
    let fast_id = supervisor.register(
        ProcessKind::ChannelPlugin,
        "fast-watchdog",
        cat_spec(),
        RestartPolicy::OnCrash {
            restart_delay: Duration::ZERO,
            watchdog: Some(Duration::ZERO),
        },
        {
            let fast_count = Arc::clone(&fast_count);
            Box::new(move || {
                fast_count.fetch_add(1, Ordering::AcqRel);
                Box::new(WaitForCancelBridge)
            })
        },
    );
    wait_for_status(&supervisor, slow_id, ProcessStatus::Running).await;
    wait_for_status(&supervisor, fast_id, ProcessStatus::Running).await;

    tokio::time::sleep(Duration::from_secs(1)).await;
    supervisor.tick().await;
    slow_cancel.acquire().await.unwrap().forget();
    wait_for_count(&fast_count, 2).await;

    slow_release.add_permits(1);
    supervisor.force_stop(slow_id).await.unwrap();
    supervisor.force_stop(fast_id).await.unwrap();
}

#[tokio::test]
async fn subscribe_receives_status_events() {
    let registry = Arc::new(ChildRegistry::new());
    let supervisor = Supervisor::new(registry);
    let mut events = supervisor.subscribe();
    let id = supervisor.register(
        ProcessKind::ChannelPlugin,
        "events",
        cat_spec(),
        RestartPolicy::OnCrash {
            restart_delay: Duration::ZERO,
            watchdog: None,
        },
        Box::new(|| Box::new(WaitForCancelBridge)),
    );

    let event = tokio::time::timeout(Duration::from_secs(1), async {
        loop {
            let event = events.recv().await.unwrap();
            if event.id == id && event.status == ProcessStatus::Running {
                return event;
            }
        }
    })
    .await
    .unwrap();
    assert_eq!(event.kind, ProcessKind::ChannelPlugin);
    supervisor.force_stop(id).await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn shutdown_all_stops_processes_in_parallel_and_allows_reuse() {
    let registry = Arc::new(ChildRegistry::new());
    let supervisor = Supervisor::new(Arc::clone(&registry));
    for label in ["first", "second"] {
        let id = supervisor.register(
            ProcessKind::ChannelPlugin,
            label,
            cat_spec(),
            RestartPolicy::OnCrash {
                restart_delay: Duration::ZERO,
                watchdog: None,
            },
            Box::new(|| Box::new(WaitForCancelBridge)),
        );
        wait_for_status(&supervisor, id, ProcessStatus::Running).await;
    }

    supervisor.shutdown_all().await;

    assert!(supervisor.snapshot().is_empty());
    assert_eq!(registry.len(), 0);
    let id = supervisor.register(
        ProcessKind::ChannelPlugin,
        "replacement",
        cat_spec(),
        RestartPolicy::OnCrash {
            restart_delay: Duration::ZERO,
            watchdog: None,
        },
        Box::new(|| Box::new(WaitForCancelBridge)),
    );
    wait_for_status(&supervisor, id, ProcessStatus::Running).await;
    supervisor.force_stop(id).await.unwrap();
}

#[cfg(unix)]
#[tokio::test]
async fn real_exit_status_is_preserved() {
    let registry = Arc::new(ChildRegistry::new());
    let supervisor = Supervisor::new(registry);
    let id = supervisor.register(
        ProcessKind::ChannelPlugin,
        "exit-seven",
        SpawnSpec::new("sh").args(["-c", "exit 7"]),
        RestartPolicy::OnCrash {
            restart_delay: Duration::from_secs(30),
            watchdog: None,
        },
        Box::new(|| Box::new(WaitForEofBridge)),
    );

    wait_for_status(&supervisor, id, ProcessStatus::Crashed).await;
    let reason = supervisor
        .snapshot()
        .into_iter()
        .find(|process| process.id == id)
        .unwrap()
        .reason;
    assert!(reason.contains("exit status: 7"), "{reason}");
    supervisor.force_stop(id).await.unwrap();
}
