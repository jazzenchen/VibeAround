use std::fs;
use std::io::{BufRead, BufReader};
use std::path::Path;

use serde_json::Value;

use crate::config;

use super::{fallback_title, modified_secs, walk_files, LaunchSession};

pub(super) fn sessions(workspace: &Path) -> Vec<LaunchSession> {
    let root = config::home_dir()
        .join(".pi")
        .join("agent")
        .join("sessions");
    let mut out = Vec::new();
    walk_files(&root, &mut |path| {
        if path.extension().and_then(|ext| ext.to_str()) != Some("jsonl") {
            return;
        }
        if let Some(session) = session_from_file(path, workspace) {
            out.push(session);
        }
    });
    out
}

fn session_from_file(path: &Path, workspace: &Path) -> Option<LaunchSession> {
    let file = fs::File::open(path).ok()?;
    let workspace_str = workspace.to_string_lossy();
    let mut line = String::new();
    BufReader::new(file).read_line(&mut line).ok()?;
    let Ok(json) = serde_json::from_str::<Value>(&line) else {
        return None;
    };
    if json.get("type").and_then(Value::as_str) != Some("session") {
        return None;
    }

    let session_id = json
        .get("id")
        .and_then(Value::as_str)
        .map(ToOwned::to_owned)
        .or_else(|| path.file_stem()?.to_str().map(ToOwned::to_owned))?;
    let session_cwd = json.get("cwd").and_then(Value::as_str)?;
    if session_cwd != workspace_str.as_ref() {
        return None;
    }
    let title = fallback_title(workspace, &session_id);

    Some(LaunchSession {
        agent_id: "pi".to_string(),
        session_id,
        title,
        workspace: workspace.to_string_lossy().to_string(),
        updated_at: modified_secs(path),
        source: "pi".to_string(),
        archived: false,
    })
}
