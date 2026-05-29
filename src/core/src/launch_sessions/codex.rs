use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use chrono::DateTime;
use rusqlite::{params, Connection, OpenFlags};
use serde_json::Value;
use tokio::fs as async_fs;
use tokio::io::{AsyncBufReadExt, BufReader as AsyncBufReader};

use crate::config;

use super::{clean_title, fallback_title, modified_secs, walk_files, LaunchSession};

pub(super) fn sessions(workspace: &Path, include_archived: bool) -> Vec<LaunchSession> {
    if let Some(sessions) = sessions_from_state_db(&[workspace.to_path_buf()], include_archived) {
        return sessions;
    }

    let root = config::home_dir().join(".codex").join("sessions");
    let index = session_index();
    let mut out = Vec::new();
    walk_files(&root, &mut |path| {
        if path.extension().and_then(|ext| ext.to_str()) != Some("jsonl") {
            return;
        }
        if let Some(session) = session_from_file(path, workspace, false, &index) {
            out.push(session);
        }
    });
    if include_archived {
        let archived_root = config::home_dir().join(".codex").join("archived_sessions");
        walk_files(&archived_root, &mut |path| {
            if path.extension().and_then(|ext| ext.to_str()) != Some("jsonl") {
                return;
            }
            if let Some(session) = session_from_file(path, workspace, true, &index) {
                out.push(session);
            }
        });
    }
    out
}

pub(super) async fn sessions_for_workspaces_async(
    workspaces: &[PathBuf],
    include_archived: bool,
) -> Vec<LaunchSession> {
    let db_workspaces = workspaces.to_vec();
    if let Some(sessions) = tokio::task::spawn_blocking(move || {
        sessions_from_state_db(&db_workspaces, include_archived)
    })
    .await
    .ok()
    .flatten()
    {
        return sessions;
    }

    let workspace_lookup = workspaces
        .iter()
        .map(|workspace| workspace.to_string_lossy().to_string())
        .collect::<HashSet<_>>();
    if workspace_lookup.is_empty() {
        return Vec::new();
    }

    let root = config::home_dir().join(".codex").join("sessions");
    let index = session_index_async().await;
    let mut out = Vec::new();
    scan_root_async(&root, &workspace_lookup, false, &index, &mut out).await;
    if include_archived {
        let archived_root = config::home_dir().join(".codex").join("archived_sessions");
        scan_root_async(&archived_root, &workspace_lookup, true, &index, &mut out).await;
    }
    out
}

fn sessions_from_state_db(
    workspaces: &[PathBuf],
    include_archived: bool,
) -> Option<Vec<LaunchSession>> {
    if workspaces.is_empty() {
        return Some(Vec::new());
    }

    let db_path = config::home_dir().join(".codex").join("state_5.sqlite");
    let conn = Connection::open_with_flags(
        db_path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .ok()?;

    let mut sessions = Vec::new();
    for workspace in workspaces {
        let workspace = workspace.to_string_lossy();
        let sql = if include_archived {
            "SELECT id, title, cwd, updated_at, archived \
             FROM threads \
             WHERE cwd = ?1 \
             ORDER BY updated_at_ms DESC, id DESC"
        } else {
            "SELECT id, title, cwd, updated_at, archived \
             FROM threads \
             WHERE archived = 0 AND cwd = ?1 \
             ORDER BY updated_at_ms DESC, id DESC"
        };
        let mut stmt = conn.prepare(sql).ok()?;
        let rows = stmt
            .query_map(params![workspace.as_ref()], |row| {
                let session_id: String = row.get(0)?;
                let title = row
                    .get::<_, Option<String>>(1)?
                    .and_then(|title| clean_title(&title))
                    .unwrap_or_else(|| fallback_title(Path::new(workspace.as_ref()), &session_id));
                let workspace: String = row.get(2)?;
                let updated_at: i64 = row.get(3)?;
                let archived: i64 = row.get(4)?;
                Ok(LaunchSession {
                    agent_id: "codex".to_string(),
                    session_id,
                    title,
                    workspace,
                    updated_at: u64::try_from(updated_at).unwrap_or(0),
                    source: "codex".to_string(),
                    archived: archived != 0,
                })
            })
            .ok()?;

        for session in rows.flatten() {
            sessions.push(session);
        }
    }

    Some(sessions)
}

async fn scan_root_async(
    root: &Path,
    workspace_lookup: &HashSet<String>,
    archived: bool,
    index: &HashMap<String, CodexSessionIndexEntry>,
    out: &mut Vec<LaunchSession>,
) {
    let mut dirs = vec![root.to_path_buf()];
    let mut scanned_files = 0usize;

    while let Some(dir) = dirs.pop() {
        let Ok(mut entries) = async_fs::read_dir(&dir).await else {
            continue;
        };
        while let Ok(Some(entry)) = entries.next_entry().await {
            let path = entry.path();
            let Ok(file_type) = entry.file_type().await else {
                continue;
            };
            if file_type.is_dir() {
                dirs.push(path);
                continue;
            }
            if path.extension().and_then(|ext| ext.to_str()) != Some("jsonl") {
                continue;
            }
            if let Some(session) =
                session_from_file_for_workspaces_async(&path, workspace_lookup, archived, index)
                    .await
            {
                out.push(session);
            }
            scanned_files += 1;
            if scanned_files % 8 == 0 {
                tokio::task::yield_now().await;
            }
        }
        tokio::task::yield_now().await;
    }
}

fn session_from_file(
    path: &Path,
    workspace: &Path,
    archived: bool,
    index: &HashMap<String, CodexSessionIndexEntry>,
) -> Option<LaunchSession> {
    let file = fs::File::open(path).ok()?;
    let mut line = String::new();
    BufReader::new(file).read_line(&mut line).ok()?;
    let session = session_from_metadata_line(&line, archived, modified_secs(path), index)?;
    let workspace_str = workspace.to_string_lossy();
    if session.workspace != workspace_str.as_ref() {
        return None;
    }
    Some(session)
}

async fn session_from_file_for_workspaces_async(
    path: &Path,
    workspace_lookup: &HashSet<String>,
    archived: bool,
    index: &HashMap<String, CodexSessionIndexEntry>,
) -> Option<LaunchSession> {
    let file = async_fs::File::open(path).await.ok()?;
    let mut lines = AsyncBufReader::new(file).lines();
    let line = lines.next_line().await.ok()??;
    let session =
        session_from_metadata_line(&line, archived, modified_secs_async(path).await, index)?;
    workspace_lookup
        .contains(&session.workspace)
        .then_some(session)
}

async fn modified_secs_async(path: &Path) -> u64 {
    async_fs::metadata(path)
        .await
        .and_then(|meta| meta.modified())
        .ok()
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

fn session_from_metadata_line(
    line: &str,
    archived: bool,
    updated_at: u64,
    index: &HashMap<String, CodexSessionIndexEntry>,
) -> Option<LaunchSession> {
    let json: Value = serde_json::from_str(line).ok()?;
    let payload = json.get("payload")?;
    let session_id = payload.get("id").and_then(Value::as_str)?.to_string();
    let workspace = payload.get("cwd").and_then(Value::as_str)?.to_string();
    let indexed = index.get(&session_id);
    let title = indexed
        .and_then(|entry| entry.thread_name.clone())
        .unwrap_or_else(|| fallback_title(Path::new(&workspace), &session_id));
    let updated_at = indexed
        .and_then(|entry| entry.updated_at)
        .unwrap_or(updated_at);
    Some(LaunchSession {
        agent_id: "codex".to_string(),
        session_id,
        title,
        workspace,
        updated_at,
        source: "codex".to_string(),
        archived,
    })
}

#[derive(Debug, Default)]
struct CodexSessionIndexEntry {
    thread_name: Option<String>,
    updated_at: Option<u64>,
}

fn session_index_path() -> std::path::PathBuf {
    config::home_dir()
        .join(".codex")
        .join("session_index.jsonl")
}

fn session_index() -> HashMap<String, CodexSessionIndexEntry> {
    fs::read_to_string(session_index_path())
        .ok()
        .map(|contents| parse_session_index(&contents))
        .unwrap_or_default()
}

async fn session_index_async() -> HashMap<String, CodexSessionIndexEntry> {
    async_fs::read_to_string(session_index_path())
        .await
        .ok()
        .map(|contents| parse_session_index(&contents))
        .unwrap_or_default()
}

fn parse_session_index(contents: &str) -> HashMap<String, CodexSessionIndexEntry> {
    contents
        .lines()
        .filter_map(|line| {
            let json: Value = serde_json::from_str(line).ok()?;
            let id = json.get("id").and_then(Value::as_str)?.to_string();
            let thread_name = json
                .get("thread_name")
                .and_then(Value::as_str)
                .and_then(clean_title);
            let updated_at = json
                .get("updated_at")
                .and_then(Value::as_str)
                .and_then(parse_rfc3339_secs);
            Some((
                id,
                CodexSessionIndexEntry {
                    thread_name,
                    updated_at,
                },
            ))
        })
        .collect()
}

fn parse_rfc3339_secs(value: &str) -> Option<u64> {
    let timestamp = DateTime::parse_from_rfc3339(value).ok()?.timestamp();
    u64::try_from(timestamp).ok()
}
