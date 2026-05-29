use std::collections::HashSet;
use std::fs;
use std::io::{BufRead, BufReader};
use std::path::Path;

use chrono::DateTime;
use serde_json::Value;

use crate::config;

use super::{
    clean_title, dash_encoded_cwd, fallback_title, message_text, modified_secs, sha256_hex,
    LaunchSession,
};

pub(super) fn sessions(workspace: &Path) -> Vec<LaunchSession> {
    let mut out = sessions_from_project_dir(workspace);
    out.extend(sessions_from_tmp_dir(workspace));
    let mut seen = HashSet::new();
    out.retain(|session| seen.insert(session.session_id.clone()));
    out
}

fn sessions_from_project_dir(workspace: &Path) -> Vec<LaunchSession> {
    let chats_dir = config::home_dir()
        .join(".qwen")
        .join("projects")
        .join(dash_encoded_cwd(workspace))
        .join("chats");
    let Ok(entries) = fs::read_dir(chats_dir) else {
        return Vec::new();
    };

    entries
        .flatten()
        .filter_map(|entry| {
            let path = entry.path();
            if path.extension().and_then(|ext| ext.to_str()) != Some("jsonl") {
                return None;
            }
            let fallback_id = path.file_stem()?.to_str()?.to_string();
            let metadata = session_metadata(&path);
            let session_id = metadata
                .as_ref()
                .and_then(|metadata| metadata.session_id.clone())
                .unwrap_or(fallback_id);
            let title = metadata
                .as_ref()
                .and_then(|metadata| metadata.title.clone())
                .unwrap_or_else(|| fallback_title(workspace, &session_id));
            let updated_at = metadata
                .as_ref()
                .and_then(|metadata| metadata.updated_at)
                .unwrap_or_else(|| modified_secs(&path));
            Some(LaunchSession {
                agent_id: "qwen-code".to_string(),
                session_id,
                title,
                workspace: workspace.to_string_lossy().to_string(),
                updated_at,
                source: "qwen-code".to_string(),
                archived: false,
            })
        })
        .collect()
}

fn sessions_from_tmp_dir(workspace: &Path) -> Vec<LaunchSession> {
    let workspace_str = workspace.to_string_lossy();
    let chats_dir = config::home_dir()
        .join(".qwen")
        .join("tmp")
        .join(sha256_hex(workspace_str.as_ref()))
        .join("chats");
    let Ok(entries) = fs::read_dir(chats_dir) else {
        return Vec::new();
    };

    entries
        .flatten()
        .filter_map(|entry| {
            let path = entry.path();
            let name = path.file_name()?.to_str()?;
            let extension = path.extension().and_then(|ext| ext.to_str())?;
            if !name.starts_with("session-") || !matches!(extension, "json" | "jsonl") {
                return None;
            }
            let metadata = if extension == "json" {
                old_json_metadata(&path)?
            } else {
                old_jsonl_metadata(&path)?
            };
            let session_id = metadata.session_id?;
            let title = metadata
                .title
                .unwrap_or_else(|| fallback_title(workspace, &session_id));
            Some(LaunchSession {
                agent_id: "qwen-code".to_string(),
                session_id,
                title,
                workspace: workspace_str.to_string(),
                updated_at: metadata.updated_at.unwrap_or_else(|| modified_secs(&path)),
                source: "qwen-code".to_string(),
                archived: false,
            })
        })
        .collect()
}

#[derive(Debug, Default)]
struct QwenSessionMetadata {
    session_id: Option<String>,
    title: Option<String>,
    updated_at: Option<u64>,
}

fn session_metadata(path: &Path) -> Option<QwenSessionMetadata> {
    let file = fs::File::open(path).ok()?;
    let mut line = String::new();
    BufReader::new(file).read_line(&mut line).ok()?;
    let json: Value = serde_json::from_str(&line).ok()?;
    let session_id = json
        .get("sessionId")
        .and_then(Value::as_str)
        .map(ToOwned::to_owned);
    let updated_at = json
        .get("timestamp")
        .and_then(Value::as_str)
        .and_then(parse_rfc3339_secs);
    let title = if json.get("type").and_then(Value::as_str) == Some("user") {
        json.get("message")
            .and_then(message_text)
            .and_then(clean_title)
    } else {
        None
    };
    Some(QwenSessionMetadata {
        session_id,
        title,
        updated_at,
    })
}

fn old_json_metadata(path: &Path) -> Option<QwenSessionMetadata> {
    let data = fs::read_to_string(path).ok()?;
    let json: Value = serde_json::from_str(&data).ok()?;
    let session_id = json
        .get("sessionId")
        .and_then(Value::as_str)
        .map(ToOwned::to_owned);
    let title = json
        .get("summary")
        .and_then(Value::as_str)
        .and_then(clean_title);
    let updated_at = json
        .get("lastUpdated")
        .and_then(Value::as_str)
        .and_then(parse_rfc3339_secs);
    Some(QwenSessionMetadata {
        session_id,
        title,
        updated_at,
    })
}

fn old_jsonl_metadata(path: &Path) -> Option<QwenSessionMetadata> {
    let file = fs::File::open(path).ok()?;
    let mut line = String::new();
    BufReader::new(file).read_line(&mut line).ok()?;
    let json: Value = serde_json::from_str(&line).ok()?;
    let session_id = json
        .get("sessionId")
        .and_then(Value::as_str)
        .map(ToOwned::to_owned);
    let updated_at = json
        .get("lastUpdated")
        .or_else(|| json.get("timestamp"))
        .and_then(Value::as_str)
        .and_then(parse_rfc3339_secs);
    Some(QwenSessionMetadata {
        session_id,
        title: None,
        updated_at,
    })
}

fn parse_rfc3339_secs(value: &str) -> Option<u64> {
    let timestamp = DateTime::parse_from_rfc3339(value).ok()?.timestamp();
    u64::try_from(timestamp).ok()
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::super::message_text;

    #[test]
    fn message_text_reads_parts() {
        let message = json!({
            "role": "user",
            "parts": [{ "text": "hello qwen" }]
        });

        assert_eq!(message_text(&message), Some("hello qwen"));
    }
}
