//! Startup sweep for child processes left over from a previous crash.
//!
//! Matches VibeAround plugin and ACP agent subprocesses, including helper
//! descendants, when the owning daemon is gone. Runs before daemon child
//! processes are spawned; a safety net, not a lifecycle mechanism.

use std::collections::{HashMap, HashSet};
use std::path::Path;

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
            "[orphan-sweep]: killing pid={} ppid={:?} kind={} cmd={}",
            pid.as_u32(),
            parent.map(|p| p.as_u32()),
            kind,
            cmdline
        );

        if sys.process(pid).is_some_and(|proc_| proc_.kill()) {
            killed += 1;
        } else {
            tracing::info!("[orphan-sweep]: failed to kill pid={}", pid.as_u32());
        }
    }

    if killed > 0 {
        tracing::info!("[orphan-sweep]: killed {} orphan(s)", killed);
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
