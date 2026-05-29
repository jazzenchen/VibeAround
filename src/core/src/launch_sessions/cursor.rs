use std::fs;
use std::io::{BufRead, BufReader};
use std::path::Path;

use serde_json::Value;

use crate::config;

use super::{clean_title, fallback_title, message_text, modified_secs, walk_files, LaunchSession};

pub(super) fn sessions(workspace: &Path) -> Vec<LaunchSession> {
    let transcripts_dir = config::home_dir()
        .join(".cursor")
        .join("projects")
        .join(encoded_cwd(workspace))
        .join("agent-transcripts");
    let mut out = Vec::new();
    walk_files(&transcripts_dir, &mut |path| {
        if path.extension().and_then(|ext| ext.to_str()) != Some("jsonl") {
            return;
        }
        let Some(session_id) = path.file_stem().and_then(|stem| stem.to_str()) else {
            return;
        };
        out.push(LaunchSession {
            agent_id: "cursor".to_string(),
            session_id: session_id.to_string(),
            title: title(path).unwrap_or_else(|| fallback_title(workspace, session_id)),
            workspace: workspace.to_string_lossy().to_string(),
            updated_at: modified_secs(path),
            source: "cursor".to_string(),
            archived: false,
        });
    });
    out
}

fn encoded_cwd(cwd: &Path) -> String {
    cwd.to_string_lossy()
        .trim_start_matches('/')
        .chars()
        .map(|ch| if ch.is_alphanumeric() { ch } else { '-' })
        .collect()
}

fn title(path: &Path) -> Option<String> {
    let file = fs::File::open(path).ok()?;
    let mut line = String::new();
    BufReader::new(file).read_line(&mut line).ok()?;
    let json: Value = serde_json::from_str(&line).ok()?;
    if json.get("role").and_then(Value::as_str) != Some("user") {
        return None;
    }
    json.get("message")
        .and_then(message_text)
        .and_then(clean_user_title)
}

fn clean_user_title(value: &str) -> Option<String> {
    let trimmed = value.trim();
    let inner = trimmed
        .strip_prefix("<user_query>")
        .and_then(|rest| rest.strip_suffix("</user_query>"))
        .map(str::trim)
        .unwrap_or(trimmed);
    clean_title(inner)
}

#[cfg(test)]
mod tests {
    use super::clean_user_title;

    #[test]
    fn title_strips_user_query_wrapper() {
        assert_eq!(
            clean_user_title("<user_query>\nhello cursor\n</user_query>"),
            Some("hello cursor".to_string())
        );
    }
}
