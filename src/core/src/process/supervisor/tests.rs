use super::*;
use crate::process::bridge::{BridgeExit, CancelSignal, ProcessBridge, StdioPipes};
use std::sync::atomic::{AtomicU32, AtomicUsize, Ordering};
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

#[test]
fn restart_delay_backs_off_and_caps() {
    let base = Duration::from_secs(30);
    assert_eq!(
        generation::restart_delay_for_failure(base, 1),
        Duration::from_secs(30)
    );
    assert_eq!(
        generation::restart_delay_for_failure(base, 2),
        Duration::from_secs(60)
    );
    assert_eq!(
        generation::restart_delay_for_failure(base, 4),
        Duration::from_secs(240)
    );
    assert_eq!(
        generation::restart_delay_for_failure(base, 5),
        MAX_RESTART_DELAY
    );
    assert_eq!(
        generation::restart_delay_for_failure(base, u32::MAX),
        MAX_RESTART_DELAY
    );
}

#[tokio::test]
async fn spawn_failure_event_carries_the_real_reason() {
    let supervisor = Supervisor::new();
    let mut events = supervisor.subscribe();
    let id = supervisor
        .register(
            ProcessKind::Tunnel,
            "missing-tunnel-binary",
            SpawnSpec::new("vibearound-program-that-does-not-exist"),
            RestartPolicy::Never,
            Box::new(|| Box::new(WaitForCancelBridge)),
        )
        .await;

    let stopped = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            match events.recv().await {
                Ok(event) if event.id == id && event.status == ProcessStatus::Stopped => {
                    break event;
                }
                Ok(_) | Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {}
                Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                    panic!("supervisor events closed before Stopped")
                }
            }
        }
    })
    .await
    .expect("spawn failure must publish a Stopped event");

    assert!(
        stopped.reason.contains("failed to spawn"),
        "reason should carry the spawn error, got: {}",
        stopped.reason
    );
    assert!(stopped
        .reason
        .contains("vibearound-program-that-does-not-exist"));
}

#[tokio::test]
async fn force_restart_reports_spawn_failure() {
    let supervisor = Supervisor::new();
    let id = supervisor
        .register(
            ProcessKind::ChannelPlugin,
            "missing-program",
            SpawnSpec::new("vibearound-program-that-does-not-exist"),
            RestartPolicy::OnCrash {
                restart_delay: Duration::from_secs(30),
                watchdog: None,
            },
            Box::new(|| Box::new(WaitForCancelBridge)),
        )
        .await;
    wait_for_status(&supervisor, id, ProcessStatus::Crashed).await;

    let error = supervisor.force_restart(id).await.unwrap_err();

    assert!(error.to_string().contains("failed to spawn"));
}

#[tokio::test]
async fn never_policy_spawn_failure_drops_factory_and_deregisters() {
    let supervisor = Supervisor::new();
    let (probe_tx, probe_rx) = oneshot::channel::<()>();
    let mut probe_tx = Some(probe_tx);
    let id = supervisor
        .register(
            ProcessKind::AcpAgent,
            "spawn-fail-never",
            SpawnSpec::new("vibearound-program-that-does-not-exist"),
            RestartPolicy::Never,
            Box::new(move || {
                // Models the ACP ready handshake: the sender must be dropped
                // when the owner dies, or `await_agent_ready` hangs forever.
                let _leaked_handshake = probe_tx.take();
                unreachable!("factory must not run for a failed spawn");
            }),
        )
        .await;

    tokio::time::timeout(Duration::from_secs(2), probe_rx)
        .await
        .expect("spawn failure leaked the bridge factory (ready handshake would hang)")
        .expect_err("factory must never fire the probe");
    wait_for_absent(&supervisor, id).await;
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn bridge_exit_terminates_helper_process_group() {
    let supervisor = Supervisor::new();
    let helper_pid = Arc::new(AtomicU32::new(0));
    let captured_pid = Arc::clone(&helper_pid);
    let id = supervisor
        .register(
            ProcessKind::ChannelPlugin,
            "helper-tree-reap",
            SpawnSpec::new("sh").args(["-c", "sleep 60 & echo $!; wait"]),
            RestartPolicy::Never,
            Box::new(move || {
                Box::new(CapturePidThenCleanBridge {
                    pid: Arc::clone(&captured_pid),
                })
            }),
        )
        .await;

    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    let child_pid = loop {
        let child_pid = helper_pid.load(Ordering::Acquire);
        if child_pid != 0 {
            break child_pid;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "bridge did not report the helper pid"
        );
        tokio::task::yield_now().await;
    };
    wait_for_absent(&supervisor, id).await;
    assert_ne!(child_pid, 0, "bridge should capture the helper pid");

    use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, RefreshKind, System};
    let mut alive = true;
    for _ in 0..50 {
        let mut system = System::new_with_specifics(
            RefreshKind::nothing().with_processes(ProcessRefreshKind::everything()),
        );
        system.refresh_processes(ProcessesToUpdate::All, true);
        alive = system.process(Pid::from_u32(child_pid)).is_some();
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
    let supervisor = Supervisor::new();
    let id = supervisor
        .register(
            ProcessKind::ChannelPlugin,
            "cat-test",
            cat_spec(),
            RestartPolicy::OnCrash {
                restart_delay: Duration::from_secs(30),
                watchdog: None,
            },
            Box::new(|| Box::new(WaitForCancelBridge)),
        )
        .await;
    wait_for_status(&supervisor, id, ProcessStatus::Running).await;

    supervisor.force_stop(id).await.unwrap();

    wait_for_status(&supervisor, id, ProcessStatus::Stopped).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn lifecycle_commands_are_consumed_in_order() {
    let supervisor = Supervisor::new();
    let cancel_seen = Arc::new(tokio::sync::Semaphore::new(0));
    let release = Arc::new(tokio::sync::Semaphore::new(0));
    let factory_count = Arc::new(AtomicUsize::new(0));
    let id = supervisor
        .register(
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
        )
        .await;
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
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn force_stop_aborts_a_stubborn_bridge() {
    let supervisor = Supervisor::new();
    let dropped = Arc::new(AtomicUsize::new(0));
    let id = supervisor
        .register(
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
        )
        .await;
    wait_for_status(&supervisor, id, ProcessStatus::Running).await;

    tokio::time::timeout(Duration::from_secs(1), supervisor.force_stop(id))
        .await
        .expect("stubborn bridge stop hung")
        .unwrap();

    assert_eq!(dropped.load(Ordering::Acquire), 1);
}

#[tokio::test]
async fn unregister_stops_and_removes_restartable_process() {
    let supervisor = Supervisor::new();
    let id = supervisor
        .register(
            ProcessKind::ChannelPlugin,
            "unregister",
            cat_spec(),
            RestartPolicy::OnCrash {
                restart_delay: Duration::ZERO,
                watchdog: None,
            },
            Box::new(|| Box::new(WaitForCancelBridge)),
        )
        .await;
    wait_for_status(&supervisor, id, ProcessStatus::Running).await;

    supervisor.unregister(id).await.unwrap();

    assert!(!supervisor.snapshot().iter().any(|process| process.id == id));
}

#[tokio::test]
async fn never_policy_auto_deregisters_terminal_process() {
    let supervisor = Supervisor::new();
    let id = supervisor
        .register(
            ProcessKind::AcpAgent,
            "one-shot",
            cat_spec(),
            RestartPolicy::Never,
            Box::new(|| Box::new(InstantErrorBridge)),
        )
        .await;

    wait_for_absent(&supervisor, id).await;
}

#[tokio::test]
async fn crash_policy_marks_process_crashed() {
    let supervisor = Supervisor::new();
    let id = supervisor
        .register(
            ProcessKind::ChannelPlugin,
            "crash",
            cat_spec(),
            RestartPolicy::OnCrash {
                restart_delay: Duration::from_secs(30),
                watchdog: None,
            },
            Box::new(|| Box::new(InstantErrorBridge)),
        )
        .await;

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
    let supervisor = Supervisor::new();
    let count = Arc::new(AtomicUsize::new(0));
    let id = supervisor
        .register(
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
        )
        .await;
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
    let supervisor = Supervisor::new();
    let slow_cancel = Arc::new(tokio::sync::Semaphore::new(0));
    let slow_release = Arc::new(tokio::sync::Semaphore::new(0));
    let slow_id = supervisor
        .register(
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
        )
        .await;
    let fast_count = Arc::new(AtomicUsize::new(0));
    let fast_id = supervisor
        .register(
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
        )
        .await;
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
    let supervisor = Supervisor::new();
    let mut events = supervisor.subscribe();
    let id = supervisor
        .register(
            ProcessKind::ChannelPlugin,
            "events",
            cat_spec(),
            RestartPolicy::OnCrash {
                restart_delay: Duration::ZERO,
                watchdog: None,
            },
            Box::new(|| Box::new(WaitForCancelBridge)),
        )
        .await;

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
    let supervisor = Supervisor::new();
    for label in ["first", "second"] {
        let id = supervisor
            .register(
                ProcessKind::ChannelPlugin,
                label,
                cat_spec(),
                RestartPolicy::OnCrash {
                    restart_delay: Duration::ZERO,
                    watchdog: None,
                },
                Box::new(|| Box::new(WaitForCancelBridge)),
            )
            .await;
        wait_for_status(&supervisor, id, ProcessStatus::Running).await;
    }

    supervisor.shutdown_all().await;

    assert!(supervisor.snapshot().is_empty());
    let id = supervisor
        .register(
            ProcessKind::ChannelPlugin,
            "replacement",
            cat_spec(),
            RestartPolicy::OnCrash {
                restart_delay: Duration::ZERO,
                watchdog: None,
            },
            Box::new(|| Box::new(WaitForCancelBridge)),
        )
        .await;
    wait_for_status(&supervisor, id, ProcessStatus::Running).await;
    supervisor.force_stop(id).await.unwrap();
}

/// Bridge for the hard-kill harness: relays the child's own pid to the
/// harness stdout, then tells the child to start its grandchild (the
/// bridge only runs once the child is leased, so the grandchild is born
/// covered), relays that pid too on Unix, and waits to be cancelled.
struct ReportPidsBridge;

impl ProcessBridge for ReportPidsBridge {
    fn run(
        self: Box<Self>,
        mut pipes: StdioPipes,
        mut cancel: CancelSignal,
    ) -> super::super::bridge::BridgeFuture {
        Box::pin(async move {
            use tokio::io::AsyncBufReadExt;

            let mut stdout = tokio::io::BufReader::new(pipes.stdout).lines();
            for label in ["child", "grandchild"] {
                let line = tokio::select! {
                    line = stdout.next_line() => line,
                    _ = cancel.wait_for(|cancelled| *cancelled) => break,
                };
                match line {
                    Ok(Some(line)) => println!("{label} {}", line.trim()),
                    _ => break,
                }
                if label == "child" {
                    let _ = pipes.stdin.write_all(b"go\n").await;
                }
            }
            let _ = cancel.wait_for(|cancelled| *cancelled).await;
            BridgeExit::Cancelled
        })
    }
}

/// A child that prints its own pid, waits for a line on stdin, then starts
/// a grandchild that ignores SIGTERM, prints its pid, and waits forever.
#[cfg(unix)]
fn harness_child_spec() -> SpawnSpec {
    SpawnSpec::new("sh").args([
        "-c",
        "trap '' TERM; echo $$; read _; sleep 60 & echo $!; wait",
    ])
}

/// Same shape on Windows: PowerShell prints its pid, waits for a line, then
/// runs `cmd /C ping` with ping's output on NUL — not on a pipe back to the
/// harness, so a broken pipe after the kill cannot end it by itself.
#[cfg(windows)]
fn harness_child_spec() -> SpawnSpec {
    SpawnSpec::new("powershell").args([
        "-NoProfile",
        "-Command",
        "$PID; $null = [Console]::In.ReadLine(); cmd /C 'ping -t 127.0.0.1 >NUL'",
    ])
}

/// Body of the hard-kill harness: a daemon stand-in that supervises one
/// child. Only runs when spawned by `hard_killed_daemon_takes_every_child_down`;
/// otherwise it is a no-op.
#[tokio::test]
async fn harness_supervised_children_for_hard_kill_test() {
    if std::env::var_os("VA_LEASE_HARNESS").is_none() {
        return;
    }
    let supervisor = Supervisor::new();
    supervisor
        .register(
            ProcessKind::ChannelPlugin,
            "hard-kill-harness",
            harness_child_spec(),
            RestartPolicy::Never,
            Box::new(|| Box::new(ReportPidsBridge)),
        )
        .await;
    std::future::pending::<()>().await;
}

fn snapshot_processes() -> sysinfo::System {
    use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, RefreshKind, System};

    let mut system = System::new_with_specifics(
        RefreshKind::nothing().with_processes(ProcessRefreshKind::everything()),
    );
    system.refresh_processes(ProcessesToUpdate::All, true);
    system
}

/// Direct children of `parent`: (pid, executable name, command line).
fn children_of(system: &sysinfo::System, parent: u32) -> Vec<(u32, String, String)> {
    system
        .processes()
        .values()
        .filter(|process| process.parent() == Some(sysinfo::Pid::from_u32(parent)))
        .map(|process| {
            let command = process
                .cmd()
                .iter()
                .map(|part| part.to_string_lossy().into_owned())
                .collect::<Vec<_>>()
                .join(" ");
            (
                process.pid().as_u32(),
                process.name().to_string_lossy().into_owned(),
                command,
            )
        })
        .collect()
}

/// Every descendant of `root`, breadth first.
#[cfg(windows)]
fn descendants_of(system: &sysinfo::System, root: u32) -> Vec<(u32, String, String)> {
    let mut queue = vec![root];
    let mut found = Vec::new();
    while let Some(parent) = queue.pop() {
        for child in children_of(system, parent) {
            queue.push(child.0);
            found.push(child);
        }
    }
    found
}

#[cfg(unix)]
fn process_exists(pid: u32) -> bool {
    // SAFETY: signal 0 only checks for existence.
    if unsafe { libc::kill(pid as libc::pid_t, 0) } == 0 {
        return true;
    }
    std::io::Error::last_os_error().raw_os_error() != Some(libc::ESRCH)
}

#[cfg(windows)]
fn process_exists(pid: u32) -> bool {
    snapshot_processes()
        .process(sysinfo::Pid::from_u32(pid))
        .is_some()
}

/// The real thing: a separate daemon process is killed the hard way
/// (SIGKILL / TerminateProcess) and its child, grandchild, and — on Unix —
/// reaper must all be gone shortly after, with nobody in the daemon having
/// run any cleanup code.
#[test]
fn hard_killed_daemon_takes_every_child_down() {
    use std::io::BufRead;
    use std::sync::mpsc;

    struct KillOnDrop(std::process::Child);
    impl Drop for KillOnDrop {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }

    let harness = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "process::supervisor::tests::harness_supervised_children_for_hard_kill_test",
            "--nocapture",
            "--test-threads=1",
        ])
        .env("VA_LEASE_HARNESS", "1")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .expect("spawn harness");
    let mut harness = KillOnDrop(harness);
    let harness_pid = harness.0.id();
    let stdout = harness.0.stdout.take().expect("harness stdout");
    let (lines_tx, lines_rx) = mpsc::channel();
    std::thread::spawn(move || {
        for line in std::io::BufReader::new(stdout)
            .lines()
            .map_while(Result::ok)
        {
            if lines_tx.send(line).is_err() {
                break;
            }
        }
    });

    // Collect the pids the harness reports. With `--nocapture` libtest
    // prints its own `test … ...` prefix on the same line as the first
    // report, so scan word pairs anywhere in each line.
    let mut child = None;
    let mut grandchild = None;
    let deadline = std::time::Instant::now() + Duration::from_secs(20);
    while child.is_none() || (cfg!(unix) && grandchild.is_none()) {
        let remaining = deadline.saturating_duration_since(std::time::Instant::now());
        let line = lines_rx
            .recv_timeout(remaining)
            .expect("harness did not report its pids in time");
        let words: Vec<&str> = line.split_whitespace().collect();
        for pair in words.windows(2) {
            let Ok(pid) = pair[1].parse::<u32>() else {
                continue;
            };
            match pair[0] {
                "child" => child = Some(pid),
                "grandchild" => grandchild = Some(pid),
                _ => {}
            }
        }
    }
    let child = child.unwrap();

    // Everything the kill must take down: the child, its descendants, and
    // on Unix the reaper (found as the harness's `sh` child). On Windows
    // the descendants are discovered by parent pid and must include ping.
    let mut doomed = vec![child];
    doomed.extend(grandchild);
    let deadline = std::time::Instant::now() + Duration::from_secs(20);
    loop {
        let system = snapshot_processes();
        let mut found = Vec::new();
        #[cfg(unix)]
        found.extend(
            children_of(&system, harness_pid)
                .into_iter()
                .filter(|(_, _, command)| command.contains("trap '' HUP INT"))
                .map(|(pid, _, _)| pid),
        );
        #[cfg(windows)]
        {
            let descendants = descendants_of(&system, child);
            if descendants
                .iter()
                .any(|(_, name, _)| name.to_ascii_lowercase().contains("ping"))
            {
                found.extend(descendants.into_iter().map(|(pid, _, _)| pid));
            }
        }
        if !found.is_empty() {
            doomed.extend(found);
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "could not find the reaper / grandchild of the harness"
        );
        std::thread::sleep(Duration::from_millis(50));
    }
    for pid in &doomed {
        assert!(
            process_exists(*pid),
            "pid {pid} should be alive before the kill"
        );
    }

    hard_kill(&mut harness.0);
    let _ = harness.0.wait();

    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    while doomed.iter().any(|pid| process_exists(*pid)) {
        if std::time::Instant::now() >= deadline {
            let survivors: Vec<u32> = doomed
                .iter()
                .copied()
                .filter(|pid| process_exists(*pid))
                .collect();
            for pid in &survivors {
                hard_kill_pid(*pid);
            }
            panic!("processes outlived the hard-killed daemon: {survivors:?} (of {doomed:?})");
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

#[cfg(unix)]
fn hard_kill(process: &mut std::process::Child) {
    hard_kill_pid(process.id());
}

#[cfg(unix)]
fn hard_kill_pid(pid: u32) {
    // SAFETY: signalling a pid this test created.
    unsafe { libc::kill(pid as libc::pid_t, libc::SIGKILL) };
}

/// `Child::kill` is `TerminateProcess`: no handler runs in the target.
#[cfg(windows)]
fn hard_kill(process: &mut std::process::Child) {
    let _ = process.kill();
}

#[cfg(windows)]
fn hard_kill_pid(pid: u32) {
    let _ = std::process::Command::new("taskkill")
        .args(["/PID", &pid.to_string(), "/T", "/F"])
        .status();
}

#[cfg(unix)]
#[tokio::test]
async fn real_exit_status_is_preserved() {
    let supervisor = Supervisor::new();
    let id = supervisor
        .register(
            ProcessKind::ChannelPlugin,
            "exit-seven",
            SpawnSpec::new("sh").args(["-c", "exit 7"]),
            RestartPolicy::OnCrash {
                restart_delay: Duration::from_secs(30),
                watchdog: None,
            },
            Box::new(|| Box::new(WaitForEofBridge)),
        )
        .await;

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
