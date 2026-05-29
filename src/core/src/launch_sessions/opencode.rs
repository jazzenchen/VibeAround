use std::collections::HashSet;
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;

use rusqlite::{params, Connection, OpenFlags};
use serde_json::Value;

use crate::config;

use super::{clean_title, fallback_title, timestamp_secs, LaunchSession};

pub(super) fn sessions(workspace: &Path) -> Vec<LaunchSession> {
    sessions_for_workspaces(&[workspace.to_path_buf()])
}

pub(super) fn sessions_for_workspaces(workspaces: &[PathBuf]) -> Vec<LaunchSession> {
    if let Some(sessions) = sessions_from_db(workspaces) {
        return sessions;
    }
    sessions_from_cli(workspaces)
}

fn sessions_from_db(workspaces: &[PathBuf]) -> Option<Vec<LaunchSession>> {
    let db_path = config::home_dir()
        .join(".local")
        .join("share")
        .join("opencode")
        .join("opencode.db");
    let conn = Connection::open_with_flags(
        db_path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .ok()?;

    let mut sessions = Vec::new();
    for workspace in workspaces {
        let workspace = workspace.to_string_lossy();
        let mut stmt = conn
            .prepare(
                "SELECT id, directory, title, time_updated \
                 FROM session \
                 WHERE directory = ?1 AND time_archived IS NULL \
                 ORDER BY time_updated DESC",
            )
            .ok()?;
        let rows = stmt
            .query_map(params![workspace.as_ref()], |row| {
                let session_id: String = row.get(0)?;
                let workspace: String = row.get(1)?;
                let title = row
                    .get::<_, Option<String>>(2)?
                    .and_then(|title| clean_title(&title))
                    .unwrap_or_else(|| fallback_title(Path::new(&workspace), &session_id));
                let updated_at: i64 = row.get(3)?;
                Ok(LaunchSession {
                    agent_id: "opencode".to_string(),
                    session_id,
                    title,
                    workspace,
                    updated_at: u64::try_from(updated_at).map(timestamp_secs).unwrap_or(0),
                    source: "opencode".to_string(),
                    archived: false,
                })
            })
            .ok()?;

        for session in rows.flatten() {
            sessions.push(session);
        }
    }

    Some(sessions)
}

fn sessions_from_cli(workspaces: &[PathBuf]) -> Vec<LaunchSession> {
    let Ok(output) = Command::new("opencode")
        .args(["session", "list", "--format", "json"])
        .output()
    else {
        return Vec::new();
    };
    if !output.status.success() || output.stdout.is_empty() {
        return Vec::new();
    }
    let Ok(json) = serde_json::from_slice::<Value>(&output.stdout) else {
        return Vec::new();
    };
    let Some(items) = json.as_array() else {
        return Vec::new();
    };
    let workspace_lookup = workspaces
        .iter()
        .map(|workspace| workspace.to_string_lossy().to_string())
        .collect::<HashSet<_>>();

    items
        .iter()
        .filter_map(|item| {
            let workspace = item.get("directory").and_then(Value::as_str)?;
            if !workspace_lookup.contains(workspace) {
                return None;
            }
            let session_id = item.get("id").and_then(Value::as_str)?.to_string();
            let title = item
                .get("title")
                .and_then(Value::as_str)
                .and_then(clean_title)
                .unwrap_or_else(|| fallback_title(Path::new(workspace), &session_id));
            Some(LaunchSession {
                agent_id: "opencode".to_string(),
                session_id,
                title,
                workspace: workspace.to_string(),
                updated_at: item
                    .get("updated")
                    .and_then(Value::as_u64)
                    .map(timestamp_secs)
                    .unwrap_or(0),
                source: "opencode".to_string(),
                archived: false,
            })
        })
        .collect()
}
