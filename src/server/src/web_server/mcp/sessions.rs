//! Session auto-discovery — find the most recent session ID a local agent
//! stored for a given workspace.
//!
//! Each agent persists session history in a different layout:
//!   - Claude:  `~/.claude/projects/<encoded-cwd>/<session>.jsonl`
//!   - Gemini:  `~/.gemini/tmp/<slug>/chats/session-*.json` (mapped via
//!              `~/.gemini/projects.json`)
//!   - Codex:   `~/.codex/sessions/YYYY/MM/DD/rollout-*.jsonl` (first line
//!              contains `payload.cwd` and `payload.id`)
//!   - Opencode: `~/.local/share/opencode/storage/session/*/ses_*.json`
//!
//! Used when the agent-side `prepare_handover` MCP tool is invoked without
//! an explicit `session_id`.

/// Find the most recent session ID for a given agent kind and workspace.
pub(super) fn find_latest_session(agent_kind: &str, cwd: &std::path::Path) -> Option<String> {
    match agent_kind {
        "claude" => find_latest_claude_session(cwd),
        "gemini" => find_latest_gemini_session(cwd),
        "codex" => find_latest_codex_session(cwd),
        "opencode" => find_latest_opencode_session(cwd),
        _ => None,
    }
}

/// Find the most recent Claude session file for a given cwd.
/// Claude encodes the cwd by replacing non-alphanumeric chars with `-`.
fn find_latest_claude_session(cwd: &std::path::Path) -> Option<String> {
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .ok()?;
    let projects_dir = std::path::PathBuf::from(home)
        .join(".claude")
        .join("projects");

    // Claude encodes cwd: /Users/foo/bar → -Users-foo-bar
    let encoded_cwd = cwd
        .to_string_lossy()
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '-' })
        .collect::<String>();

    let session_dir = projects_dir.join(&encoded_cwd);
    if !session_dir.is_dir() {
        return None;
    }

    // Find the .jsonl file with the most recent modification time
    let mut best: Option<(std::time::SystemTime, String)> = None;
    if let Ok(entries) = std::fs::read_dir(&session_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("jsonl") {
                continue;
            }
            let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
                continue;
            };
            let Ok(meta) = path.metadata() else { continue };
            let Ok(modified) = meta.modified() else {
                continue;
            };

            match &best {
                Some((best_time, _)) if modified <= *best_time => {}
                _ => {
                    best = Some((modified, stem.to_string()));
                }
            }
        }
    }

    best.map(|(_, session_id)| session_id)
}

/// Find the most recent Gemini session for a given cwd.
/// Gemini maps cwd → slug via `~/.gemini/projects.json`, then stores
/// session files at `~/.gemini/tmp/<slug>/chats/session-*.json`.
fn find_latest_gemini_session(cwd: &std::path::Path) -> Option<String> {
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .ok()?;
    let gemini_dir = std::path::PathBuf::from(&home).join(".gemini");

    // Read projects.json to map cwd → slug
    let projects_path = gemini_dir.join("projects.json");
    let projects_data = std::fs::read_to_string(&projects_path).ok()?;
    let projects_json: serde_json::Value = serde_json::from_str(&projects_data).ok()?;
    let projects_map = projects_json.get("projects")?.as_object()?;

    let cwd_str = cwd.to_string_lossy();
    let slug = projects_map.get(cwd_str.as_ref())?.as_str()?;

    let chats_dir = gemini_dir.join("tmp").join(slug).join("chats");
    if !chats_dir.is_dir() {
        return None;
    }

    // Find the most recent session-*.json file
    let mut best: Option<(std::time::SystemTime, String)> = None;
    if let Ok(entries) = std::fs::read_dir(&chats_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            let fname = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if !fname.starts_with("session-")
                || path.extension().and_then(|e| e.to_str()) != Some("json")
            {
                continue;
            }
            let Ok(meta) = path.metadata() else { continue };
            let Ok(modified) = meta.modified() else {
                continue;
            };

            match &best {
                Some((best_time, _)) if modified <= *best_time => {}
                _ => {
                    // Parse JSON to extract sessionId
                    if let Ok(data) = std::fs::read_to_string(&path) {
                        if let Ok(json) = serde_json::from_str::<serde_json::Value>(&data) {
                            if let Some(sid) = json.get("sessionId").and_then(|v| v.as_str()) {
                                best = Some((modified, sid.to_string()));
                            }
                        }
                    }
                }
            }
        }
    }

    best.map(|(_, session_id)| session_id)
}

/// Find the most recent Codex session for a given cwd.
/// Codex stores sessions at `~/.codex/sessions/YYYY/MM/DD/rollout-*.jsonl`.
/// The first line of each file contains `payload.cwd` and `payload.id`.
fn find_latest_codex_session(cwd: &std::path::Path) -> Option<String> {
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .ok()?;
    let sessions_dir = std::path::PathBuf::from(&home)
        .join(".codex")
        .join("sessions");
    if !sessions_dir.is_dir() {
        return None;
    }

    let cwd_str = cwd.to_string_lossy();
    let mut best: Option<(std::time::SystemTime, String)> = None;

    // Walk the sessions directory recursively
    fn walk_codex_sessions(
        dir: &std::path::Path,
        cwd_str: &str,
        best: &mut Option<(std::time::SystemTime, String)>,
    ) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                walk_codex_sessions(&path, cwd_str, best);
                continue;
            }
            if path.extension().and_then(|e| e.to_str()) != Some("jsonl") {
                continue;
            }
            let Ok(meta) = path.metadata() else { continue };
            let Ok(modified) = meta.modified() else {
                continue;
            };

            // Skip if older than current best
            if let Some((best_time, _)) = best {
                if modified <= *best_time {
                    continue;
                }
            }

            // Read first line and check cwd match
            let Ok(file) = std::fs::File::open(&path) else {
                continue;
            };
            let reader = std::io::BufRead::lines(std::io::BufReader::new(file));
            let Some(Ok(first_line)) = reader.into_iter().next() else {
                continue;
            };
            let Ok(json) = serde_json::from_str::<serde_json::Value>(&first_line) else {
                continue;
            };

            let payload = match json.get("payload") {
                Some(p) => p,
                None => continue,
            };
            let session_cwd = match payload.get("cwd").and_then(|v| v.as_str()) {
                Some(c) => c,
                None => continue,
            };
            if session_cwd != cwd_str {
                continue;
            }
            if let Some(sid) = payload.get("id").and_then(|v| v.as_str()) {
                *best = Some((modified, sid.to_string()));
            }
        }
    }

    walk_codex_sessions(&sessions_dir, &cwd_str, &mut best);
    best.map(|(_, session_id)| session_id)
}

/// Find the most recent Opencode session for a given cwd.
/// Opencode stores sessions at `~/.local/share/opencode/storage/session/*/ses_*.json`.
/// Each file contains an `directory` field matching the session's working directory.
fn find_latest_opencode_session(cwd: &std::path::Path) -> Option<String> {
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .ok()?;
    let sessions_dir = std::path::PathBuf::from(&home)
        .join(".local")
        .join("share")
        .join("opencode")
        .join("storage")
        .join("session");
    if !sessions_dir.is_dir() {
        return None;
    }

    let cwd_str = cwd.to_string_lossy();
    let mut best: Option<(std::time::SystemTime, String)> = None;

    // Walk session subdirectories looking for ses_*.json files
    if let Ok(projects) = std::fs::read_dir(&sessions_dir) {
        for project_entry in projects.flatten() {
            let project_dir = project_entry.path();
            if !project_dir.is_dir() {
                continue;
            }
            if let Ok(files) = std::fs::read_dir(&project_dir) {
                for file_entry in files.flatten() {
                    let path = file_entry.path();
                    let fname = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
                    if !fname.starts_with("ses_") || path.extension().and_then(|e| e.to_str()) != Some("json") {
                        continue;
                    }
                    let Ok(meta) = path.metadata() else { continue };
                    let Ok(modified) = meta.modified() else { continue };
                    let Ok(data) = std::fs::read_to_string(&path) else { continue };
                    let Ok(json) = serde_json::from_str::<serde_json::Value>(&data) else { continue };
                    let session_dir = json.get("directory")?.as_str()?;
                    if session_dir != cwd_str {
                        continue;
                    }
                    let Some(sid) = json.get("id").and_then(|v| v.as_str()) else { continue };
                    match &best {
                        Some((best_time, _)) if modified <= *best_time => {}
                        _ => {
                            best = Some((modified, sid.to_string()));
                        }
                    }
                }
            }
        }
    }

    best.map(|(_, session_id)| session_id)
}
