//! Local coding-agent session discovery for the desktop Launch surface.
//!
//! This is intentionally read-only. Each CLI owns its own session store; we
//! only surface enough metadata for users to choose what to resume.

use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Serialize;
use serde_json::Value;
use sha2::{Digest, Sha256};

mod archive;
mod claude;
mod codex;
mod cursor;
mod gemini;
mod opencode;
mod pi;
mod qwen;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LaunchSession {
    pub agent_id: String,
    pub session_id: String,
    pub title: String,
    pub workspace: String,
    pub updated_at: u64,
    pub source: String,
    pub archived: bool,
}

pub fn list_for_agent_workspace(
    agent_id: &str,
    workspace: &Path,
    limit: usize,
) -> Vec<LaunchSession> {
    list_for_agent_workspace_with_archived(agent_id, workspace, limit, false)
}

pub fn list_for_agent_workspace_with_archived(
    agent_id: &str,
    workspace: &Path,
    limit: usize,
    include_archived: bool,
) -> Vec<LaunchSession> {
    let sessions = raw_sessions_for_agent_workspace(agent_id, workspace, include_archived);
    finalize_sessions(agent_id, sessions, limit, include_archived)
}

pub async fn list_for_agent_workspace_with_archived_async(
    agent_id: &str,
    workspace: &Path,
    limit: usize,
    include_archived: bool,
) -> Vec<LaunchSession> {
    let workspaces = [workspace.to_path_buf()];
    list_for_agent_workspaces_with_archived_async(agent_id, &workspaces, limit, include_archived)
        .await
}

pub async fn list_for_agent_workspaces_with_archived_async(
    agent_id: &str,
    workspaces: &[std::path::PathBuf],
    limit: usize,
    include_archived: bool,
) -> Vec<LaunchSession> {
    let mut sessions = match agent_id {
        "codex" => codex::sessions_for_workspaces_async(workspaces, include_archived).await,
        "opencode" => {
            let workspaces = workspaces.to_vec();
            tokio::task::spawn_blocking(move || opencode::sessions_for_workspaces(&workspaces))
                .await
                .unwrap_or_default()
        }
        _ => {
            let agent_id = agent_id.to_string();
            let workspaces = workspaces.to_vec();
            tokio::task::spawn_blocking(move || {
                workspaces
                    .iter()
                    .flat_map(|workspace| {
                        raw_sessions_for_agent_workspace(&agent_id, workspace, include_archived)
                    })
                    .collect::<Vec<_>>()
            })
            .await
            .unwrap_or_default()
        }
    };
    apply_archive_flags(agent_id, &mut sessions, include_archived);
    sessions.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));

    let mut counts_by_workspace: HashMap<String, usize> = HashMap::new();
    sessions.retain(|session| {
        let count = counts_by_workspace
            .entry(session.workspace.clone())
            .or_insert(0);
        if *count >= limit {
            return false;
        }
        *count += 1;
        true
    });
    sessions
}

fn raw_sessions_for_agent_workspace(
    agent_id: &str,
    workspace: &Path,
    include_archived: bool,
) -> Vec<LaunchSession> {
    match agent_id {
        "claude" => claude::sessions(workspace),
        "codex" => codex::sessions(workspace, include_archived),
        "cursor" => cursor::sessions(workspace),
        "gemini" => gemini::sessions(workspace),
        "opencode" => opencode::sessions(workspace),
        "pi" => pi::sessions(workspace),
        "qwen-code" => qwen::sessions(workspace),
        _ => Vec::new(),
    }
}

fn finalize_sessions(
    agent_id: &str,
    mut sessions: Vec<LaunchSession>,
    limit: usize,
    include_archived: bool,
) -> Vec<LaunchSession> {
    apply_archive_flags(agent_id, &mut sessions, include_archived);
    sessions.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
    sessions.truncate(limit);
    sessions
}

fn apply_archive_flags(agent_id: &str, sessions: &mut Vec<LaunchSession>, include_archived: bool) {
    let archived_session_ids = archive::archived_session_ids(agent_id);
    if !archived_session_ids.is_empty() {
        for session in sessions.iter_mut() {
            if archived_session_ids.contains(&session.session_id) {
                session.archived = true;
            }
        }
        if !include_archived {
            sessions.retain(|session| !session.archived);
        }
    }
}

pub fn archive_session(agent_id: &str, workspace: &Path, session_id: &str) -> Result<(), String> {
    archive::archive_session(agent_id, workspace, session_id)
}

pub fn unarchive_session(agent_id: &str, workspace: &Path, session_id: &str) -> Result<(), String> {
    archive::unarchive_session(agent_id, workspace, session_id)
}

pub fn latest_for_agent_workspace(agent_id: &str, workspace: &Path) -> Option<LaunchSession> {
    list_for_agent_workspace(agent_id, workspace, 1)
        .into_iter()
        .next()
}

pub(super) fn dash_encoded_cwd(cwd: &Path) -> String {
    cwd.to_string_lossy()
        .chars()
        .map(|ch| if ch.is_alphanumeric() { ch } else { '-' })
        .collect()
}

pub(super) fn sha256_hex(value: &str) -> String {
    let digest = Sha256::digest(value.as_bytes());
    format!("{digest:x}")
}

pub(super) fn message_text(message: &Value) -> Option<&str> {
    if let Some(text) = message.get("content").and_then(Value::as_str) {
        return Some(text);
    }
    for key in ["content", "parts"] {
        if let Some(items) = message.get(key).and_then(Value::as_array) {
            for item in items {
                if let Some(text) = item.get("text").and_then(Value::as_str) {
                    return Some(text);
                }
                if let Some(text) = item.get("data").and_then(Value::as_str) {
                    return Some(text);
                }
            }
        }
    }
    None
}

pub(super) fn walk_files(dir: &Path, visit: &mut impl FnMut(&Path)) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            walk_files(&path, visit);
        } else {
            visit(&path);
        }
    }
}

pub(super) fn modified_secs(path: &Path) -> u64 {
    fs::metadata(path)
        .and_then(|meta| meta.modified())
        .ok()
        .and_then(system_time_secs)
        .unwrap_or(0)
}

fn system_time_secs(time: SystemTime) -> Option<u64> {
    time.duration_since(UNIX_EPOCH)
        .ok()
        .map(|duration| duration.as_secs())
}

pub(super) fn timestamp_secs(value: u64) -> u64 {
    if value > 1_000_000_000_000 {
        value / 1_000
    } else {
        value
    }
}

pub(super) fn clean_title(value: &str) -> Option<String> {
    let compact = value.split_whitespace().collect::<Vec<_>>().join(" ");
    let compact = compact.trim();
    if compact.is_empty() {
        return None;
    }
    let lower = compact.to_ascii_lowercase();
    if lower.starts_with("<command-name>")
        || lower.starts_with("<local-command")
        || lower.starts_with("<system-reminder")
    {
        return None;
    }
    let mut out = String::new();
    for ch in compact.chars().take(72) {
        out.push(ch);
    }
    Some(out)
}

pub(super) fn fallback_title(workspace: &Path, session_id: &str) -> String {
    let workspace = workspace
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .unwrap_or("Session");
    format!("{workspace} {}", short_id(session_id))
}

pub fn short_id(session_id: &str) -> String {
    session_id.chars().take(8).collect()
}

#[cfg(test)]
mod tests {
    use super::clean_title;

    #[test]
    fn clean_title_skips_control_messages() {
        assert_eq!(
            clean_title("<command-name>/model</command-name><command-message>x</command-message>"),
            None
        );
        assert_eq!(
            clean_title("<local-command-stdout>ok</local-command-stdout>"),
            None
        );
    }
}
