//! Local coding-agent session discovery for the desktop Launch surface.
//!
//! This is intentionally read-only. Each CLI owns its own session store; we
//! only surface enough metadata for users to choose what to resume.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::Path;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
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

static OBSERVED_STORE_LOCK: Mutex<()> = Mutex::new(());

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LaunchSession {
    pub agent_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub profile_id: Option<String>,
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
    let mut sessions = raw_sessions_for_agent_workspace(agent_id, workspace, include_archived);
    sessions.extend(observed_sessions_for_agent_workspace(agent_id, workspace));
    finalize_sessions(agent_id, sessions, limit, include_archived)
}

pub async fn list_for_agent_workspaces_with_archived_async(
    agent_id: &str,
    workspaces: &[std::path::PathBuf],
    limit: usize,
    include_archived: bool,
) -> Vec<LaunchSession> {
    let mut sessions =
        native_sessions_for_agent_workspaces_async(agent_id, workspaces, include_archived).await;
    sessions.extend(
        workspaces
            .iter()
            .flat_map(|workspace| observed_sessions_for_agent_workspace(agent_id, workspace)),
    );
    finalize_workspace_sessions(agent_id, sessions, limit, include_archived)
}

pub async fn list_native_for_agent_workspace_with_archived_async(
    agent_id: &str,
    workspace: &Path,
    limit: usize,
    include_archived: bool,
) -> Vec<LaunchSession> {
    let workspaces = [workspace.to_path_buf()];
    list_native_for_agent_workspaces_with_archived_async(
        agent_id,
        &workspaces,
        limit,
        include_archived,
    )
    .await
}

pub async fn list_native_for_agent_workspaces_with_archived_async(
    agent_id: &str,
    workspaces: &[std::path::PathBuf],
    limit: usize,
    include_archived: bool,
) -> Vec<LaunchSession> {
    let mut sessions =
        native_sessions_for_agent_workspaces_async(agent_id, workspaces, include_archived).await;
    apply_observed_profile_ids(agent_id, workspaces, &mut sessions);
    finalize_workspace_sessions(agent_id, sessions, limit, include_archived)
}

async fn native_sessions_for_agent_workspaces_async(
    agent_id: &str,
    workspaces: &[std::path::PathBuf],
    include_archived: bool,
) -> Vec<LaunchSession> {
    match agent_id {
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
    }
}

fn finalize_workspace_sessions(
    agent_id: &str,
    mut sessions: Vec<LaunchSession>,
    limit: usize,
    include_archived: bool,
) -> Vec<LaunchSession> {
    apply_archive_flags(agent_id, &mut sessions, include_archived);
    sessions.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
    dedupe_by_session_id(&mut sessions);

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
    dedupe_by_session_id(&mut sessions);
    sessions.truncate(limit);
    sessions
}

fn dedupe_by_session_id(sessions: &mut Vec<LaunchSession>) {
    let mut seen = HashSet::new();
    sessions.retain(|session| seen.insert(session.session_id.clone()));
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

#[derive(Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct ObservedLaunchSessionStore {
    #[serde(default)]
    sessions: Vec<ObservedLaunchSession>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct ObservedLaunchSession {
    launch_id: String,
    agent_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    profile_id: Option<String>,
    session_id: String,
    workspace: String,
    source: String,
    created_at: u64,
    updated_at: u64,
}

pub fn record_observed_launch_session(
    launch_id: Option<&str>,
    agent_id: &str,
    profile_id: Option<&str>,
    workspace: Option<&Path>,
    session_id: &str,
    source: &str,
) -> Result<(), String> {
    let Some(launch_id) = clean_non_empty(launch_id) else {
        return Ok(());
    };
    let Some(session_id) = clean_non_empty(Some(session_id)) else {
        return Ok(());
    };
    let agent_id =
        crate::resources::resolve_agent_id(agent_id).unwrap_or_else(|_| agent_id.to_string());
    let workspace = workspace
        .map(workspace_key)
        .filter(|value| !value.is_empty())
        .unwrap_or_default();
    let profile_id = profile_id
        .and_then(|value| clean_non_empty(Some(value)))
        .map(|value| crate::agent::launch::normalize_launch_profile_id(Some(&value)));
    let source = clean_non_empty(Some(source)).unwrap_or_else(|| "mcp".to_string());
    let now = now_secs();

    mutate_observed_store(|store| {
        if let Some(existing) = store
            .sessions
            .iter_mut()
            .find(|session| session.launch_id == launch_id)
        {
            existing.agent_id = agent_id;
            if profile_id.is_some() {
                existing.profile_id = profile_id;
            }
            existing.session_id = session_id;
            if !workspace.is_empty() {
                existing.workspace = workspace;
            }
            existing.source = source;
            existing.updated_at = now;
            return;
        }

        store.sessions.push(ObservedLaunchSession {
            launch_id,
            agent_id,
            profile_id,
            session_id,
            workspace,
            source,
            created_at: now,
            updated_at: now,
        });
    })
}

fn observed_sessions_for_agent_workspace(agent_id: &str, workspace: &Path) -> Vec<LaunchSession> {
    let agent_id =
        crate::resources::resolve_agent_id(agent_id).unwrap_or_else(|_| agent_id.to_string());
    let workspace = workspace_key(workspace);
    read_observed_store()
        .sessions
        .into_iter()
        .filter(|session| {
            session.agent_id == agent_id
                && session.workspace == workspace
                && !session.session_id.is_empty()
        })
        .map(|session| LaunchSession {
            agent_id: session.agent_id,
            profile_id: session.profile_id,
            title: fallback_title(Path::new(&session.workspace), &session.session_id),
            workspace: session.workspace,
            session_id: session.session_id,
            updated_at: session.updated_at,
            source: format!("vibearound-{}", session.source),
            archived: false,
        })
        .collect()
}

fn apply_observed_profile_ids(
    agent_id: &str,
    workspaces: &[std::path::PathBuf],
    sessions: &mut [LaunchSession],
) {
    let profiles = observed_profile_ids_for_agent_workspaces(agent_id, workspaces);
    if profiles.is_empty() {
        return;
    }
    for session in sessions {
        if session.profile_id.is_some() {
            continue;
        }
        let key = (session.workspace.clone(), session.session_id.clone());
        if let Some(profile_id) = profiles.get(&key) {
            session.profile_id = Some(profile_id.clone());
        }
    }
}

fn observed_profile_ids_for_agent_workspaces(
    agent_id: &str,
    workspaces: &[std::path::PathBuf],
) -> HashMap<(String, String), String> {
    let agent_id =
        crate::resources::resolve_agent_id(agent_id).unwrap_or_else(|_| agent_id.to_string());
    let workspace_keys = workspaces
        .iter()
        .map(|workspace| workspace_key(workspace))
        .collect::<HashSet<_>>();
    read_observed_store()
        .sessions
        .into_iter()
        .filter(|session| {
            session.agent_id == agent_id
                && workspace_keys.contains(&session.workspace)
                && !session.session_id.is_empty()
        })
        .filter_map(|session| {
            session
                .profile_id
                .map(|profile_id| ((session.workspace, session.session_id), profile_id))
        })
        .collect()
}

fn mutate_observed_store(
    mutator: impl FnOnce(&mut ObservedLaunchSessionStore),
) -> Result<(), String> {
    let _guard = OBSERVED_STORE_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let mut store = read_observed_store_unlocked();
    mutator(&mut store);
    write_observed_store_unlocked(&store)
}

fn read_observed_store() -> ObservedLaunchSessionStore {
    let _guard = OBSERVED_STORE_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    read_observed_store_unlocked()
}

fn read_observed_store_unlocked() -> ObservedLaunchSessionStore {
    let Ok(data) = fs::read_to_string(observed_store_path()) else {
        return ObservedLaunchSessionStore::default();
    };
    serde_json::from_str(&data).unwrap_or_default()
}

fn write_observed_store_unlocked(store: &ObservedLaunchSessionStore) -> Result<(), String> {
    let path = observed_store_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| format!("failed to create launch session store dir: {error}"))?;
    }
    let data = serde_json::to_string_pretty(store)
        .map_err(|error| format!("failed to serialize launch session store: {error}"))?;
    fs::write(&path, data)
        .map_err(|error| format!("failed to write launch session store: {error}"))?;
    if let Err(error) = crate::auth::set_owner_only(&path) {
        tracing::warn!(
            "[VibeAround] failed to restrict launch session store {:?}: {}",
            path,
            error
        );
    }
    Ok(())
}

fn observed_store_path() -> std::path::PathBuf {
    crate::config::data_dir().join("launch-sessions.json")
}

fn workspace_key(workspace: &Path) -> String {
    trim_trailing_separators(workspace.to_string_lossy().as_ref())
}

fn trim_trailing_separators(value: &str) -> String {
    let mut out = value.trim().to_string();
    while out.len() > 1 && (out.ends_with('/') || out.ends_with('\\')) {
        out.pop();
    }
    out
}

fn clean_non_empty(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
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
