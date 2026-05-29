use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::{BufRead, BufReader};
use std::path::Path;

use chrono::DateTime;
use serde_json::Value;

use crate::config;

use super::{clean_title, fallback_title, modified_secs, sha256_hex, LaunchSession};

pub(super) fn sessions(workspace: &Path) -> Vec<LaunchSession> {
    let keys = project_keys(workspace);
    if keys.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    for key in keys {
        out.extend(sessions_for_project_key(workspace, &key, &mut seen));
    }
    out
}

fn sessions_for_project_key(
    workspace: &Path,
    project_key: &str,
    seen: &mut HashSet<String>,
) -> Vec<LaunchSession> {
    let chats_dir = config::home_dir()
        .join(".gemini")
        .join("tmp")
        .join(project_key)
        .join("chats");
    let Ok(entries) = fs::read_dir(chats_dir) else {
        return Vec::new();
    };
    let logs = logs_index(project_key);

    entries
        .flatten()
        .filter_map(|entry| {
            let path = entry.path();
            let name = path.file_name()?.to_str()?;
            let extension = path.extension().and_then(|ext| ext.to_str())?;
            if !name.starts_with("session-") || !matches!(extension, "json" | "jsonl") {
                return None;
            }

            let indexed = session_key(&path).and_then(|key| logs.get(key));
            let metadata = if extension == "json" {
                metadata_from_json(&path).or_else(|| indexed.map(metadata_from_log_entry))?
            } else if let Some(indexed) = indexed {
                GeminiSessionMetadata {
                    session_id: indexed.session_id.clone(),
                    title: indexed.title.clone(),
                    updated_at: indexed.updated_at,
                }
            } else if extension == "jsonl" {
                metadata_from_jsonl(&path)?
            } else {
                metadata_from_json(&path)?
            };

            let session_id = metadata.session_id;
            if !seen.insert(session_id.clone()) {
                return None;
            }
            let title = metadata
                .title
                .unwrap_or_else(|| fallback_title(workspace, &session_id));
            Some(LaunchSession {
                agent_id: "gemini".to_string(),
                session_id,
                title,
                workspace: workspace.to_string_lossy().to_string(),
                updated_at: metadata.updated_at.unwrap_or_else(|| modified_secs(&path)),
                source: "gemini".to_string(),
                archived: false,
            })
        })
        .collect()
}

fn project_keys(workspace: &Path) -> Vec<String> {
    let mut keys = Vec::new();
    let projects_path = config::home_dir().join(".gemini").join("projects.json");
    let workspace = workspace.to_string_lossy();
    if let Some(key) = fs::read_to_string(projects_path)
        .ok()
        .and_then(|data| serde_json::from_str::<Value>(&data).ok())
        .and_then(|json| {
            json.get("projects")?
                .get(workspace.as_ref())?
                .as_str()
                .map(ToOwned::to_owned)
        })
    {
        keys.push(key);
    }

    keys.push(sha256_hex(workspace.as_ref()));
    keys.sort();
    keys.dedup();
    keys
}

fn metadata_from_log_entry(entry: &GeminiLogEntry) -> GeminiSessionMetadata {
    GeminiSessionMetadata {
        session_id: entry.session_id.clone(),
        title: entry.title.clone(),
        updated_at: entry.updated_at,
    }
}

#[derive(Clone, Debug, Default)]
struct GeminiLogEntry {
    session_id: String,
    title: Option<String>,
    updated_at: Option<u64>,
}

#[derive(Debug)]
struct GeminiSessionMetadata {
    session_id: String,
    title: Option<String>,
    updated_at: Option<u64>,
}

fn logs_index(slug: &str) -> HashMap<String, GeminiLogEntry> {
    let path = config::home_dir()
        .join(".gemini")
        .join("tmp")
        .join(slug)
        .join("logs.json");
    let Ok(data) = fs::read_to_string(path) else {
        return HashMap::new();
    };
    let Ok(json) = serde_json::from_str::<Value>(&data) else {
        return HashMap::new();
    };
    let Some(items) = json.as_array() else {
        return HashMap::new();
    };

    let mut index = HashMap::new();
    for item in items {
        if item.get("type").and_then(Value::as_str) != Some("user") {
            continue;
        }
        let Some(session_id) = item.get("sessionId").and_then(Value::as_str) else {
            continue;
        };
        let title = item
            .get("message")
            .and_then(Value::as_str)
            .and_then(clean_title);
        let updated_at = item
            .get("timestamp")
            .and_then(Value::as_str)
            .and_then(parse_rfc3339_secs);

        upsert_log_entry(
            &mut index,
            session_id,
            session_id,
            title.clone(),
            updated_at,
        );
        let short_id = session_id.chars().take(8).collect::<String>();
        upsert_log_entry(&mut index, &short_id, session_id, title, updated_at);
    }

    index
}

fn upsert_log_entry(
    index: &mut HashMap<String, GeminiLogEntry>,
    key: &str,
    session_id: &str,
    title: Option<String>,
    updated_at: Option<u64>,
) {
    let entry = index.entry(key.to_string()).or_default();
    if entry.session_id.is_empty() {
        entry.session_id = session_id.to_string();
    }
    if entry.title.is_none() {
        entry.title = title;
    }
    if let Some(updated_at) = updated_at {
        entry.updated_at = Some(entry.updated_at.unwrap_or(0).max(updated_at));
    }
}

fn session_key(path: &Path) -> Option<&str> {
    path.file_stem()?.to_str()?.rsplit('-').next()
}

fn metadata_from_jsonl(path: &Path) -> Option<GeminiSessionMetadata> {
    let file = fs::File::open(path).ok()?;
    let mut line = String::new();
    BufReader::new(file).read_line(&mut line).ok()?;
    let json: Value = serde_json::from_str(&line).ok()?;
    let session_id = json.get("sessionId").and_then(Value::as_str)?.to_string();
    let updated_at = json
        .get("lastUpdated")
        .and_then(Value::as_str)
        .and_then(parse_rfc3339_secs);
    Some(GeminiSessionMetadata {
        session_id,
        title: None,
        updated_at,
    })
}

fn metadata_from_json(path: &Path) -> Option<GeminiSessionMetadata> {
    let data = fs::read_to_string(path).ok()?;
    let json: Value = serde_json::from_str(&data).ok()?;
    let session_id = json.get("sessionId").and_then(Value::as_str)?.to_string();
    let title = json
        .get("summary")
        .and_then(Value::as_str)
        .and_then(clean_title)
        .or_else(|| title(&json));
    let updated_at = json
        .get("lastUpdated")
        .and_then(Value::as_str)
        .and_then(parse_rfc3339_secs);
    Some(GeminiSessionMetadata {
        session_id,
        title,
        updated_at,
    })
}

fn title(json: &Value) -> Option<String> {
    let messages = json.get("messages")?.as_array()?;
    for message in messages {
        if message.get("type").and_then(Value::as_str) != Some("user") {
            continue;
        }
        let content = message.get("content")?.as_array()?;
        for item in content {
            if let Some(title) = item
                .get("text")
                .and_then(Value::as_str)
                .and_then(clean_title)
            {
                return Some(title);
            }
        }
    }
    None
}

fn parse_rfc3339_secs(value: &str) -> Option<u64> {
    let timestamp = DateTime::parse_from_rfc3339(value).ok()?.timestamp();
    u64::try_from(timestamp).ok()
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::{metadata_from_json, metadata_from_jsonl, session_key};

    #[test]
    fn session_key_reads_short_id_from_filename() {
        let path = std::path::Path::new("/tmp/session-2026-05-13T10-54-a907e6be.jsonl");

        assert_eq!(session_key(path), Some("a907e6be"));
    }

    #[test]
    fn metadata_from_jsonl_reads_first_line_only() {
        let path = std::env::temp_dir().join(format!(
            "vibearound-gemini-session-{}.jsonl",
            std::process::id()
        ));
        fs::write(
            &path,
            concat!(
                "{\"sessionId\":\"9e6686fd-4f47-4e2f-96c4-70760405c0fb\",",
                "\"lastUpdated\":\"2026-05-13T10:01:09.941Z\"}\n",
                "{\"type\":\"user\",\"content\":[{\"text\":\"hello\"}]}\n"
            ),
        )
        .expect("write gemini metadata");

        let metadata = metadata_from_jsonl(&path).expect("metadata");
        let _ = fs::remove_file(&path);

        assert_eq!(metadata.session_id, "9e6686fd-4f47-4e2f-96c4-70760405c0fb");
        assert_eq!(metadata.updated_at, Some(1_778_666_469));
    }

    #[test]
    fn metadata_from_json_prefers_summary_title() {
        let path = std::env::temp_dir().join(format!(
            "vibearound-gemini-session-{}.json",
            std::process::id()
        ));
        fs::write(
            &path,
            concat!(
                "{",
                "\"sessionId\":\"4fb8e8af-5622-4710-aba0-997916910797\",",
                "\"summary\":\"Fix null avg price in SQL query.\",",
                "\"messages\":[{\"type\":\"user\",\"content\":[{\"text\":\"raw prompt\"}]}]",
                "}"
            ),
        )
        .expect("write gemini json");

        let metadata = metadata_from_json(&path).expect("metadata");
        let _ = fs::remove_file(&path);

        assert_eq!(metadata.title, Some("Fix null avg price in SQL query.".to_string()));
    }
}
