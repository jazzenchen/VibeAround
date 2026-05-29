use std::collections::HashMap;
use std::fs;
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom};
use std::path::Path;

use serde_json::Value;

use crate::config;

#[cfg(test)]
use super::message_text;
use super::{
    clean_title, dash_encoded_cwd, fallback_title, modified_secs, timestamp_secs, LaunchSession,
};

const TITLE_SCAN_BYTES: u64 = 64 * 1024;

pub(super) fn sessions(workspace: &Path) -> Vec<LaunchSession> {
    let session_dir = config::home_dir()
        .join(".claude")
        .join("projects")
        .join(dash_encoded_cwd(workspace));
    let Ok(entries) = fs::read_dir(session_dir) else {
        return Vec::new();
    };
    let history = history_index_for_workspace(workspace);

    entries
        .flatten()
        .filter_map(|entry| {
            let path = entry.path();
            if path.extension().and_then(|ext| ext.to_str()) != Some("jsonl") {
                return None;
            }
            let session_id = path.file_stem()?.to_str()?.to_string();
            let indexed = history.get(&session_id);
            let updated_at = indexed
                .and_then(|entry| entry.updated_at)
                .unwrap_or_else(|| modified_secs(&path));
            let title = ai_title(&path, &session_id)
                .or_else(|| indexed.and_then(|entry| entry.title.clone()))
                .unwrap_or_else(|| fallback_title(workspace, &session_id));
            Some(LaunchSession {
                agent_id: "claude".to_string(),
                session_id,
                title,
                workspace: workspace.to_string_lossy().to_string(),
                updated_at,
                source: "claude".to_string(),
                archived: false,
            })
        })
        .collect()
}

#[derive(Debug, Default)]
struct ClaudeHistoryEntry {
    title: Option<String>,
    updated_at: Option<u64>,
}

fn history_index_for_workspace(workspace: &Path) -> HashMap<String, ClaudeHistoryEntry> {
    let path = config::home_dir().join(".claude").join("history.jsonl");
    let workspace = workspace.to_string_lossy();
    let Ok(file) = fs::File::open(path) else {
        return HashMap::new();
    };
    let mut index: HashMap<String, ClaudeHistoryEntry> = HashMap::new();

    for line in BufReader::new(file).lines().map_while(Result::ok) {
        let Ok(json) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        if json.get("project").and_then(Value::as_str) != Some(workspace.as_ref()) {
            continue;
        }
        let Some(session_id) = json.get("sessionId").and_then(Value::as_str) else {
            continue;
        };
        let entry = index.entry(session_id.to_string()).or_default();
        if entry.title.is_none() {
            entry.title = json
                .get("display")
                .and_then(Value::as_str)
                .and_then(clean_title);
        }
        if let Some(timestamp) = json.get("timestamp").and_then(Value::as_u64) {
            let timestamp = timestamp_secs(timestamp);
            entry.updated_at = Some(entry.updated_at.unwrap_or(0).max(timestamp));
        }
    }

    index
}

fn ai_title(path: &Path, session_id: &str) -> Option<String> {
    title_from_file_chunk(path, session_id, true)
        .or_else(|| title_from_file_chunk(path, session_id, false))
}

fn title_from_file_chunk(path: &Path, session_id: &str, tail: bool) -> Option<String> {
    let mut file = fs::File::open(path).ok()?;
    let len = file.metadata().ok()?.len();
    if tail {
        file.seek(SeekFrom::Start(len.saturating_sub(TITLE_SCAN_BYTES)))
            .ok()?;
    }

    let mut bytes = Vec::new();
    if tail {
        file.read_to_end(&mut bytes).ok()?;
    } else {
        file.take(TITLE_SCAN_BYTES).read_to_end(&mut bytes).ok()?;
    }
    parse_ai_title_chunk(&String::from_utf8_lossy(&bytes), session_id)
}

fn parse_ai_title_chunk(contents: &str, session_id: &str) -> Option<String> {
    let mut title = None;
    for line in contents.lines() {
        let Ok(json) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        if json.get("type").and_then(Value::as_str) != Some("ai-title") {
            continue;
        }
        if json
            .get("sessionId")
            .and_then(Value::as_str)
            .is_some_and(|value| value != session_id)
        {
            continue;
        }
        if let Some(ai_title) = json
            .get("aiTitle")
            .and_then(Value::as_str)
            .and_then(clean_title)
        {
            title = Some(ai_title);
        }
    }
    title
}

#[cfg(test)]
fn title(path: &Path) -> Option<String> {
    let file = fs::File::open(path).ok()?;
    for line in BufReader::new(file).lines().map_while(Result::ok).take(600) {
        let Ok(json) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        if json.get("type").and_then(Value::as_str) != Some("user")
            || json.get("isMeta").and_then(Value::as_bool).unwrap_or(false)
        {
            continue;
        }
        let Some(message) = json.get("message") else {
            continue;
        };
        if message.get("role").and_then(Value::as_str) != Some("user") {
            continue;
        }
        let Some(text) = message_text(message) else {
            continue;
        };
        if is_claude_context(text) {
            continue;
        }
        if let Some(title) = clean_title(text) {
            return Some(title);
        }
    }
    None
}

#[cfg(test)]
fn is_claude_context(value: &str) -> bool {
    let lower = value.trim_start().to_ascii_lowercase();
    if lower.starts_with("<system-reminder")
        || lower.contains("this file provides guidance to claude code")
    {
        return true;
    }
    let first_line = lower.lines().map(str::trim).find(|line| !line.is_empty());
    let Some(file) = first_line else {
        return false;
    };
    let file = file
        .trim_start_matches('#')
        .trim()
        .trim_matches('`')
        .trim_matches('"')
        .trim_matches('\'')
        .trim();

    matches!(
        file.rsplit(['/', '\\']).next().unwrap_or(file),
        "agents.md" | "claude.md" | "claude.local.md"
    )
}

#[cfg(test)]
mod tests {
    use std::fs;

    use serde_json::json;

    use super::{parse_ai_title_chunk, title};

    #[test]
    fn title_reads_past_claude_project_context() {
        let path = std::env::temp_dir().join(format!(
            "vibearound-claude-title-{}.jsonl",
            std::process::id()
        ));
        let context = json!({
            "type": "user",
            "message": { "role": "user", "content": "CLAUDE.md\nProject instructions" }
        });
        let prompt = json!({
            "type": "user",
            "message": {
                "role": "user",
                "content": [
                    { "type": "text", "text": "Investigate issue 68" }
                ]
            }
        });
        fs::write(&path, format!("{context}\n{prompt}\n")).expect("write test transcript");

        let result = title(&path);
        let _ = fs::remove_file(&path);

        assert_eq!(result, Some("Investigate issue 68".to_string()));
    }

    #[test]
    fn ai_title_reads_latest_matching_title() {
        let contents = r#"
{"type":"ai-title","aiTitle":"Wrong","sessionId":"other-session"}
{"type":"ai-title","aiTitle":"First title","sessionId":"session-1"}
{"type":"ai-title","aiTitle":"Latest title","sessionId":"session-1"}
"#;

        assert_eq!(
            parse_ai_title_chunk(contents, "session-1"),
            Some("Latest title".to_string())
        );
    }
}
