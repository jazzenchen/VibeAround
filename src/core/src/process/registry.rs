//! Cross-platform child process registry for plugin + agent subprocesses.
//!
//! Solves two related bugs:
//!
//! 1. **Orphaned plugin processes.** `tokio::process::Command::kill_on_drop(true)`
//!    only fires when the owning `Child` is actually Dropped. Our previous
//!    pattern moved `Child` into a `spawn_local` task that was aborted on
//!    shutdown — under abrupt runtime teardown, tokio does not necessarily
//!    poll that task to its destructor, so `kill_on_drop` never runs and the
//!    node plugin gets reparented to PID 1 (macOS/Linux) and keeps running.
//!
//! 2. **Orphaned ACP agent processes.** The agent factory moves `Child` into
//!    a `spawn_local` stdout-reader closure, dropping it only on stdout EOF.
//!    Same runtime-teardown hole as above.
//!
//! This module owns every spawned `Child` centrally behind a global singleton
//! so that:
//!
//! - `kill_all()` (called from `RunningDaemon::stop` and Tauri `RunEvent::Exit`)
//!   synchronously kills each registered child plus any helper descendants,
//!   then fires `start_kill()` on the registered `Child` before the tokio
//!   runtime shuts down, guaranteeing SIGKILL regardless of task-poll order.
//!
//! - `orphan_sweep()` runs at daemon startup and kills any leftover
//!   `node` processes whose command line references
//!   `/.vibearound/plugins/`, a discovered development channel-plugin
//!   directory, or an ACP agent package, whose parent is either init
//!   (PID 1 on Unix) or no longer alive. This self-heals from crashes,
//!   `kill -9`, and abrupt laptop sleeps.
//!
//! The registry does **not** replace per-process lifecycle management —
//! task authors still `remove()` cleanly on the normal shutdown path.
//! This is a safety net, not the primary drop path.

use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::OnceLock;

use parking_lot::Mutex;
use tokio::process::Child;

/// Classification used for orphan detection and structured logging.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProcessKind {
    /// Channel plugin process (node running from a discovered plugin directory).
    ChannelPlugin,
    /// ACP coding-agent child (node running the ACP bridge package).
    AcpAgent,
    /// Tunnel provider subprocess (cloudflared, lt, tailscale, …). Not ngrok (SDK).
    Tunnel,
    /// Host-side search provider subprocess (va-search-tool stdio).
    SearchProvider,
}

impl ProcessKind {
    /// Short lowercase tag used in structured logs (`kind=channel_plugin`).
    pub fn as_str(&self) -> &'static str {
        match self {
            ProcessKind::ChannelPlugin => "channel_plugin",
            ProcessKind::AcpAgent => "acp_agent",
            ProcessKind::Tunnel => "tunnel",
            ProcessKind::SearchProvider => "search_provider",
        }
    }
}

#[derive(Debug)]
struct Entry {
    kind: ProcessKind,
    label: String,
    child: Child,
}

pub struct ChildRegistry {
    state: Mutex<ChildRegistryState>,
}

struct ChildRegistryState {
    entries: HashMap<u64, Entry>,
    next_id: u64,
}

impl ChildRegistry {
    pub(crate) fn new() -> Self {
        Self {
            state: Mutex::new(ChildRegistryState {
                entries: HashMap::new(),
                next_id: 1,
            }),
        }
    }

    /// Global singleton. Stable across daemon restarts within the same
    /// process. Returned as `Arc<Self>` so the `Supervisor` can hold it
    /// through dependency injection while legacy callers still use
    /// `ChildRegistry::global().kill_all()` via `Arc` auto-deref.
    pub fn global() -> std::sync::Arc<ChildRegistry> {
        static INSTANCE: OnceLock<std::sync::Arc<ChildRegistry>> = OnceLock::new();
        std::sync::Arc::clone(INSTANCE.get_or_init(|| std::sync::Arc::new(ChildRegistry::new())))
    }

    /// Register a spawned child. Returns an opaque token that the caller
    /// must pass to `remove()` when the child exits cleanly.
    pub fn register(&self, kind: ProcessKind, label: impl Into<String>, child: Child) -> u64 {
        let label = label.into();
        let mut state = self.state.lock();
        let id = state.next_id;
        state.next_id = state.next_id.wrapping_add(1);
        let pid = child.id();
        tracing::info!(
            "[child-registry] register id={} kind={:?} label={} pid={:?}",
            id,
            kind,
            label,
            pid
        );
        state.entries.insert(id, Entry { kind, label, child });
        id
    }

    /// Remove a registered child and return its `Child` handle for graceful
    /// cleanup. Typically called after the caller has observed the child's
    /// stdout EOF, or during the happy-path shutdown.
    pub fn remove(&self, id: u64) -> Option<Child> {
        self.state.lock().entries.remove(&id).map(|entry| {
            tracing::info!(
                "[child-registry] remove id={} kind={:?} label={}",
                id,
                entry.kind,
                entry.label
            );
            entry.child
        })
    }

    /// Synchronously fire `start_kill()` on every registered child. This
    /// sends SIGKILL on Unix and TerminateProcess on Windows. Safe to call
    /// from inside a tokio runtime OR from a Tauri `RunEvent::Exit` handler
    /// where no runtime is available.
    ///
    /// Intentionally does NOT `.await` on `wait()` — Exit handlers must
    /// return promptly, and the OS will reap the killed children anyway.
    pub fn kill_all(&self) {
        let entries = std::mem::take(&mut self.state.lock().entries);
        tracing::info!("[child-registry] kill_all: {} child(ren)", entries.len());
        for (id, mut entry) in entries {
            let pid = entry.child.id();
            if let Some(pid) = pid {
                let descendant_count = kill_registered_descendants(pid, entry.kind, &entry.label);
                if descendant_count > 0 {
                    tracing::info!(
                        "[child-registry] killed {} descendant(s) for id={} kind={:?} label={} pid={}",
                        descendant_count,
                        id,
                        entry.kind,
                        entry.label,
                        pid
                    );
                }
            }
            match entry.child.start_kill() {
                Ok(()) => tracing::info!(
                    "[child-registry] killed id={} kind={:?} label={} pid={:?}",
                    id,
                    entry.kind,
                    entry.label,
                    pid
                ),
                Err(error) => tracing::info!(
                    "[child-registry] start_kill failed id={} label={}: {}",
                    id,
                    entry.label,
                    error
                ),
            }
        }
    }
}

fn kill_registered_descendants(root_pid: u32, kind: ProcessKind, label: &str) -> usize {
    use sysinfo::{Pid, ProcessRefreshKind, RefreshKind, System};

    let mut sys = System::new_with_specifics(
        RefreshKind::nothing().with_processes(ProcessRefreshKind::everything()),
    );
    sys.refresh_processes(sysinfo::ProcessesToUpdate::All, true);

    let descendants = process_descendants(root_pid, &sys);
    let mut killed = 0usize;
    for pid in descendants.into_iter().rev() {
        if pid == std::process::id() {
            continue;
        }

        let Some(proc_) = sys.process(Pid::from_u32(pid)) else {
            continue;
        };

        tracing::info!(
            "[child-registry] killing descendant root_pid={} kind={:?} label={} pid={} name={}",
            root_pid,
            kind,
            label,
            pid,
            proc_.name().to_string_lossy()
        );

        if proc_.kill() {
            killed += 1;
        } else {
            tracing::info!(
                "[child-registry] failed to kill descendant root_pid={} pid={}",
                root_pid,
                pid
            );
        }
    }

    killed
}

fn process_descendants(root_pid: u32, sys: &sysinfo::System) -> Vec<u32> {
    let mut by_parent: HashMap<u32, Vec<u32>> = HashMap::new();
    for (pid, proc_) in sys.processes() {
        if let Some(parent) = proc_.parent() {
            by_parent
                .entry(parent.as_u32())
                .or_default()
                .push(pid.as_u32());
        }
    }

    let mut descendants = Vec::new();
    let mut seen = HashSet::new();
    let mut stack = by_parent.remove(&root_pid).unwrap_or_default();
    while let Some(pid) = stack.pop() {
        if !seen.insert(pid) {
            continue;
        }

        descendants.push(pid);
        if let Some(children) = by_parent.remove(&pid) {
            stack.extend(children);
        }
    }

    descendants
}

/// Sweep stale child processes left over from a previous crash.
///
/// Matches VibeAround plugin and ACP agent subprocesses, including helper
/// descendants, when the owning daemon is gone. This is intentionally
/// broader than `node`: Windows can leave helper executables such as
/// `codex-acp.exe` alive after their parent `node.exe` process is orphaned,
/// and those descendants can continue holding inherited daemon handles.
///
/// Runs before daemon child processes are spawned. Windows orphan detection
/// checks parent-process liveness instead of a PPID invariant.
pub fn orphan_sweep() {
    use sysinfo::{Pid, ProcessRefreshKind, RefreshKind, System};

    let project_channel_plugin_dirs = crate::plugins::channel::discover()
        .into_values()
        .filter_map(|plugin| match plugin.source {
            crate::plugins::PluginSource::Project => Some(plugin.dir),
            crate::plugins::PluginSource::User => None,
        })
        .collect::<Vec<_>>();

    let mut sys = System::new_with_specifics(
        RefreshKind::nothing().with_processes(ProcessRefreshKind::everything()),
    );
    sys.refresh_processes(sysinfo::ProcessesToUpdate::All, true);

    let my_pid = std::process::id();
    let mut killed = 0usize;

    let candidates: Vec<(Pid, &'static str, String)> = sys
        .processes()
        .iter()
        .filter_map(|(pid, proc_)| {
            if pid.as_u32() == my_pid {
                return None;
            }

            let name = proc_.name().to_string_lossy();
            let cmdline = process_cmdline(proc_);
            let kind = vibearound_child_kind(&name, &cmdline, &project_channel_plugin_dirs)?;
            Some((*pid, kind, cmdline))
        })
        .collect();

    let candidate_pids: HashSet<u32> = candidates.iter().map(|(pid, _, _)| pid.as_u32()).collect();
    let mut orphan_memo = HashMap::new();

    for (pid, kind, cmdline) in candidates {
        if pid.as_u32() == my_pid {
            continue;
        }

        // Candidate selection already matched VibeAround-owned processes;
        // now ensure their daemon-owned process tree is actually orphaned.
        if !has_orphaned_candidate_ancestor(pid.as_u32(), &sys, &candidate_pids, &mut orphan_memo) {
            continue;
        }

        let parent = sys.process(pid).and_then(|proc_| proc_.parent());
        tracing::info!(
            "[child-registry] orphan_sweep: killing pid={} ppid={:?} kind={} cmd={}",
            pid.as_u32(),
            parent.map(|p| p.as_u32()),
            kind,
            cmdline
        );

        if sys.process(pid).is_some_and(|proc_| proc_.kill()) {
            killed += 1;
        } else {
            tracing::info!(
                "[child-registry] orphan_sweep: failed to kill pid={}",
                pid.as_u32()
            );
        }
    }

    if killed > 0 {
        tracing::info!("[child-registry] orphan_sweep: killed {} orphan(s)", killed);
    }
}

fn process_cmdline(proc_: &sysinfo::Process) -> String {
    proc_
        .cmd()
        .iter()
        .map(|s| s.to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join(" ")
}

fn vibearound_child_kind(
    name: &str,
    cmdline: &str,
    project_channel_plugin_dirs: &[std::path::PathBuf],
) -> Option<&'static str> {
    let name = name.to_lowercase();
    let cmdline = cmdline.to_lowercase();
    let normalized_cmdline = cmdline.replace('\\', "/");

    let in_plugins =
        command_references_plugin_dir(&normalized_cmdline, &crate::plugins::user_plugins_dir())
            || project_channel_plugin_dirs
                .iter()
                .any(|dir| command_references_plugin_dir(&normalized_cmdline, dir));
    let known_acp = cmdline.contains("@agentclientprotocol/")
        || cmdline.contains("@agentclientprotocol\\")
        || cmdline.contains("@zed-industries/claude-code-acp")
        || cmdline.contains("@zed-industries\\claude-code-acp")
        || cmdline.contains("@zed-industries/codex-acp")
        || cmdline.contains("@zed-industries\\codex-acp")
        || cmdline.contains("claude-agent-acp")
        || cmdline.contains("gemini-acp")
        || cmdline.contains("qwen-code-acp")
        || name.contains("codex-acp")
        || name.ends_with("-acp")
        || name.ends_with("-acp.exe");

    if known_acp {
        Some("agent-acp")
    } else if in_plugins {
        Some("plugin")
    } else {
        None
    }
}

fn command_references_plugin_dir(normalized_cmdline: &str, plugin_dir: &Path) -> bool {
    let normalized_dir = plugin_dir
        .to_string_lossy()
        .replace('\\', "/")
        .to_lowercase();
    let normalized_dir = normalized_dir.trim_end_matches('/');
    !normalized_dir.is_empty() && normalized_cmdline.contains(&format!("{normalized_dir}/"))
}

fn has_orphaned_candidate_ancestor(
    pid: u32,
    sys: &sysinfo::System,
    candidate_pids: &HashSet<u32>,
    memo: &mut HashMap<u32, bool>,
) -> bool {
    if let Some(result) = memo.get(&pid) {
        return *result;
    }

    let result = match sys.process(sysinfo::Pid::from_u32(pid)) {
        None => true,
        Some(proc_) => match proc_.parent() {
            None => true,
            Some(ppid) => {
                let ppid = ppid.as_u32();
                if (cfg!(unix) && ppid == 1) || sys.process(sysinfo::Pid::from_u32(ppid)).is_none()
                {
                    true
                } else if candidate_pids.contains(&ppid) {
                    has_orphaned_candidate_ancestor(ppid, sys, candidate_pids, memo)
                } else {
                    false
                }
            }
        },
    };

    memo.insert(pid, result);
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn installed_plugin_command_matches_user_plugin_root() {
        let entry = crate::plugins::user_plugins_dir()
            .join("va-plugin-channel-telegram")
            .join("dist/index.js");
        let cmdline = format!("node {}", entry.display());

        assert_eq!(vibearound_child_kind("node", &cmdline, &[]), Some("plugin"));
    }

    #[test]
    fn project_plugin_command_matches_discovered_directory() {
        let plugin_dir = std::path::PathBuf::from(
            "/workspace/VibeAround/src/plugins/va-plugin-channel-telegram",
        );
        let cmdline = format!("node {}/dist/index.js", plugin_dir.display());

        assert_eq!(
            vibearound_child_kind("node", &cmdline, &[plugin_dir]),
            Some("plugin")
        );
    }

    #[test]
    fn unrelated_src_plugins_command_does_not_match() {
        let project_plugin_dir = std::path::PathBuf::from(
            "/workspace/VibeAround/src/plugins/va-plugin-channel-telegram",
        );
        let cmdline = "node /workspace/other/src/plugins/va-plugin-channel-telegram/dist/index.js";

        assert_eq!(
            vibearound_child_kind("node", cmdline, &[project_plugin_dir]),
            None
        );
    }
}
