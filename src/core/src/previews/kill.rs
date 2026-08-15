//! Best-effort cleanup for processes currently listening on Preview ports.

use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, RefreshKind, System};

pub(super) fn kill_port(port: u16) {
    let pids = listener_pids(port);
    if pids.is_empty() {
        tracing::info!("[preview] kill: port {} has no listener", port);
        return;
    }

    let system = process_system();
    for pid in pids {
        let killed = system
            .process(Pid::from_u32(pid))
            .is_some_and(|process| process.kill());
        tracing::info!(
            "[preview] kill: listener pid={} port={} killed={}",
            pid,
            port,
            killed
        );
    }
}

#[cfg(unix)]
fn listener_pids(port: u16) -> Vec<u32> {
    use std::process::Command;

    let Ok(output) = Command::new("lsof")
        .args(["-nP", "-ti", &format!("tcp:{port}"), "-sTCP:LISTEN"])
        .output()
    else {
        return Vec::new();
    };
    unique_pids(&output.stdout)
}

#[cfg(windows)]
fn listener_pids(port: u16) -> Vec<u32> {
    let Ok(output) = crate::process::env::std_command("netstat")
        .args(["-ano", "-p", "TCP"])
        .output()
    else {
        return Vec::new();
    };
    let mut pids = String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| {
            let fields = line.split_whitespace().collect::<Vec<_>>();
            if fields.len() < 5 || !fields[3].eq_ignore_ascii_case("LISTENING") {
                return None;
            }
            let local_port = fields[1].rsplit_once(':')?.1.parse::<u16>().ok()?;
            if local_port != port {
                return None;
            }
            fields[4].parse::<u32>().ok()
        })
        .collect::<Vec<_>>();
    pids.sort_unstable();
    pids.dedup();
    pids
}

#[cfg(not(any(unix, windows)))]
fn listener_pids(_port: u16) -> Vec<u32> {
    Vec::new()
}

#[cfg(unix)]
fn unique_pids(bytes: &[u8]) -> Vec<u32> {
    let mut pids = String::from_utf8_lossy(bytes)
        .lines()
        .filter_map(|line| line.trim().parse::<u32>().ok())
        .collect::<Vec<_>>();
    pids.sort_unstable();
    pids.dedup();
    pids
}

fn process_system() -> System {
    let mut system = System::new_with_specifics(
        RefreshKind::nothing().with_processes(ProcessRefreshKind::everything()),
    );
    system.refresh_processes(ProcessesToUpdate::All, true);
    system
}
