//! Registered-listener cleanup for tracked dev-server ports.
//!
//! Registration records the listener PID plus its `sysinfo` start time. Before
//! cleanup, the port is resolved again and both values must still match. This
//! avoids killing an unrelated process that later reuses the same port or PID.
//!
//! Only the listener PID is killed. A process group can include the launching
//! agent or other unrelated work, so ownership cannot be inferred from PGID.

use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, RefreshKind, System};

use super::store::ListenerProcess;

pub(super) fn listener_process(port: u16) -> Option<ListenerProcess> {
    let pid = single_listener_pid(port)?;
    let system = process_system();
    fingerprint(&system, pid)
}

pub(super) fn kill_registered_listener(port: u16, expected: ListenerProcess) {
    let Some(pid) = single_listener_pid(port) else {
        tracing::info!("[preview] kill: port {} has no single listener", port);
        return;
    };
    let system = process_system();
    let current = fingerprint(&system, pid);
    if !listener_matches(expected, current) {
        tracing::info!(
            "[preview] kill: listener changed on port {} expected={:?} current={:?}",
            port,
            expected,
            current
        );
        return;
    }

    let killed = system
        .process(Pid::from_u32(expected.pid))
        .is_some_and(|process| process.kill());
    tracing::info!(
        "[preview] kill: listener pid={} port={} killed={}",
        expected.pid,
        port,
        killed
    );
}

fn listener_matches(expected: ListenerProcess, current: Option<ListenerProcess>) -> bool {
    current == Some(expected)
}

#[cfg(unix)]
fn single_listener_pid(port: u16) -> Option<u32> {
    use std::process::Command;
    let out = Command::new("lsof")
        .args(["-nP", "-ti", &format!("tcp:{port}"), "-sTCP:LISTEN"])
        .output()
        .ok()?;
    let mut pids = String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter_map(|line| line.trim().parse::<u32>().ok())
        .collect::<Vec<_>>();
    pids.sort_unstable();
    pids.dedup();
    match pids.as_slice() {
        [pid] => Some(*pid),
        _ => None,
    }
}

#[cfg(not(unix))]
fn single_listener_pid(_port: u16) -> Option<u32> {
    None
}

fn process_system() -> System {
    let mut system = System::new_with_specifics(
        RefreshKind::nothing().with_processes(ProcessRefreshKind::everything()),
    );
    system.refresh_processes(ProcessesToUpdate::All, true);
    system
}

fn fingerprint(system: &System, pid: u32) -> Option<ListenerProcess> {
    system
        .process(Pid::from_u32(pid))
        .map(|process| ListenerProcess {
            pid,
            start_time: process.start_time(),
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cleanup_requires_the_registered_pid_and_start_time() {
        let expected = ListenerProcess {
            pid: 123,
            start_time: 456,
        };

        assert!(!listener_matches(expected, None));
        assert!(!listener_matches(
            expected,
            Some(ListenerProcess {
                pid: 124,
                start_time: 456,
            })
        ));
        assert!(!listener_matches(
            expected,
            Some(ListenerProcess {
                pid: 123,
                start_time: 457,
            })
        ));
        assert!(listener_matches(expected, Some(expected)));
    }
}
