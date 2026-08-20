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
    sessions_from_root(&root, workspace, "pi", "pi")
}

pub(super) fn sessions_from_root(
    root: &Path,
    workspace: &Path,
    agent_id: &str,
    source: &str,
) -> Vec<LaunchSession> {
    let mut out = Vec::new();
    walk_files(root, &mut |path| {
        if path.extension().and_then(|ext| ext.to_str()) != Some("jsonl") {
            return;
        }
        if let Some(session) = session_from_file(path, workspace, agent_id, source) {
            out.push(session);
        }
    });
    out
}

fn session_from_file(
    path: &Path,
    workspace: &Path,
    agent_id: &str,
    source: &str,
) -> Option<LaunchSession> {
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
        agent_id: agent_id.to_string(),
        profile_id: None,
        session_id,
        title,
        workspace: workspace.to_string_lossy().to_string(),
        updated_at: modified_secs(path),
        source: source.to_string(),
        archived: false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shared_pi_reader_preserves_first_party_agent_identity() {
        let root = std::env::temp_dir().join(format!(
            "vibearound-va-agent-session-test-{}",
            uuid::Uuid::new_v4()
        ));
        let workspace = root.join("workspace");
        let sessions_dir = root.join("sessions");
        fs::create_dir_all(&sessions_dir).expect("create session test directory");
        let session_path = sessions_dir.join("session.jsonl");
        fs::write(
            &session_path,
            format!(
                "{{\"type\":\"session\",\"id\":\"va-session\",\"cwd\":{}}}\n",
                serde_json::to_string(&workspace.to_string_lossy()).expect("workspace json")
            ),
        )
        .expect("write session header");

        let sessions = sessions_from_root(&sessions_dir, &workspace, "va-agent", "va-agent");
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].agent_id, "va-agent");
        assert_eq!(sessions[0].session_id, "va-session");
        assert_eq!(sessions[0].source, "va-agent");

        fs::remove_dir_all(root).expect("remove session test directory");
    }
}
