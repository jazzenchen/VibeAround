//! Manager Agent prompt system: multi-file personality and capability definition.
//!
//! Prompt files live in `~/.vibearound/manager/` and are loaded at startup.
//! The Manager Agent itself can modify these files (self-evolution).
//!
//! Files:
//! - identity.md    — personality, name, tone of voice
//! - capabilities.md — tools description and usage examples
//! - workflow.md    — decision logic: when to dispatch vs handle directly
//! - memory.md     — long-term memory (user preferences, learned experience)
//! - rules.md      — constraints, safety boundaries

use std::path::PathBuf;

use crate::config;

/// Directory containing Manager prompt files.
pub fn manager_prompt_dir() -> PathBuf {
    config::data_dir().join("manager")
}

/// Ordered list of prompt files to load and concatenate.
const PROMPT_FILES: &[&str] = &[
    "identity.md",
    "capabilities.md",
    "workflow.md",
    "memory.md",
    "rules.md",
];

/// Load the Manager's system prompt by concatenating all prompt files.
/// Falls back to the embedded default if no files exist.
pub fn load_manager_prompt() -> String {
    let dir = manager_prompt_dir();
    let mut parts = Vec::new();
    for file in PROMPT_FILES {
        let path = dir.join(file);
        if let Ok(content) = std::fs::read_to_string(&path) {
            let trimmed = content.trim();
            if !trimmed.is_empty() {
                parts.push(trimmed.to_string());
            }
        }
    }
    if parts.is_empty() {
        return default_manager_prompt();
    }
    parts.join("\n\n---\n\n")
}

/// Ensure the manager prompt directory exists with default files.
/// Called during init_data_dir(). Safe to call multiple times.
pub fn ensure_manager_prompt_dir() {
    let dir = manager_prompt_dir();
    if let Err(e) = std::fs::create_dir_all(&dir) {
        eprintln!("[VibeAround] Failed to create manager prompt dir: {}", e);
        return;
    }

    let defaults: &[(&str, &str)] = &[
        ("identity.md", DEFAULT_IDENTITY),
        ("capabilities.md", DEFAULT_CAPABILITIES),
        ("workflow.md", DEFAULT_WORKFLOW),
        ("memory.md", DEFAULT_MEMORY),
        ("rules.md", DEFAULT_RULES),
    ];

    for (filename, content) in defaults {
        let path = dir.join(filename);
        if !path.exists() {
            if let Err(e) = std::fs::write(&path, content) {
                eprintln!("[VibeAround] Failed to write {}: {}", filename, e);
            }
        }
    }
}

/// Embedded fallback prompt (used when no files exist at all).
fn default_manager_prompt() -> String {
    [
        DEFAULT_IDENTITY,
        DEFAULT_CAPABILITIES,
        DEFAULT_WORKFLOW,
        DEFAULT_MEMORY,
        DEFAULT_RULES,
    ]
    .join("\n\n---\n\n")
}

// ---------------------------------------------------------------------------
// Default prompt file contents
// ---------------------------------------------------------------------------

const DEFAULT_IDENTITY: &str = r#"# Identity

You are the VibeAround Manager Agent — a helpful, knowledgeable AI assistant that coordinates coding work across multiple projects and worker agents.

You have your own personality and can evolve over time by updating your prompt files in `~/.vibearound/manager/`.

You speak concisely and directly. You are friendly but focused on getting things done."#;

const DEFAULT_CAPABILITIES: &str = r#"# Capabilities

You have access to the `dispatch_task` MCP tool for delegating work to worker agents.
The tool schema is provided automatically via MCP — refer to it for parameter details.

Use `dispatch_task` whenever a task requires file-system access, code generation, or project-specific work."#;

const DEFAULT_WORKFLOW: &str = r#"# Workflow

## Principles
- You are a coordinator. You never write code or create files directly.
- All file-system work is dispatched to worker agents via `dispatch_task`.
- New projects go under `~/.vibearound/workspaces/<project-name>/`.

## Dispatch to worker when:
- The task involves creating, modifying, reading, or debugging code/files.

## Handle directly when:
- Planning, architecture, general Q&A, or summarizing work done by workers."#;

const DEFAULT_MEMORY: &str = r#"# Memory

(This file is for long-term memory. You can write notes here about user preferences, project context, and lessons learned. This file persists across sessions.)"#;

const DEFAULT_RULES: &str = r#"# Rules

- When dispatching to a worker, briefly tell the user which worker you're using.
- If a worker fails or crashes, report the error and offer to restart.
- Keep memory.md concise — summarize rather than log everything."#;

// ---------------------------------------------------------------------------
// MCP config file generation (per-kind, written to workspace before spawn)
// ---------------------------------------------------------------------------

use crate::agent::AgentKind;
use std::path::Path;

/// Ensure the MCP config file for the given agent kind exists in the workspace.
/// Called by `registry::spawn_agent()` before starting the backend.
/// Only writes if the file doesn't already exist.
pub fn ensure_mcp_config(kind: AgentKind, workspace: &Path, port: u16) {
    let url = format!("http://127.0.0.1:{}/mcp", port);

    let (rel_path, content) = match kind {
        AgentKind::Claude => (
            ".mcp.json",
            format!(
                r#"{{"mcpServers":{{"vibearound":{{"type":"http","url":"{}"}}}}}}"#,
                url
            ),
        ),
        AgentKind::Gemini => (
            ".gemini/settings.json",
            format!(
                r#"{{"mcpServers":{{"vibearound":{{"httpUrl":"{}"}}}}}}"#,
                url
            ),
        ),
        AgentKind::OpenCode => (
            "opencode.json",
            format!(
                r#"{{"mcp":{{"vibearound":{{"type":"remote","url":"{}","enabled":true}}}}}}"#,
                url
            ),
        ),
        AgentKind::Codex => (
            ".codex/config.toml",
            format!(
                "[mcp_servers.vibearound]\nurl = \"{}\"\n",
                url
            ),
        ),
    };

    let path = workspace.join(rel_path);
    if path.exists() {
        return;
    }
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Err(e) = std::fs::write(&path, &content) {
        eprintln!(
            "[VibeAround] Failed to write MCP config {:?}: {}",
            path, e
        );
    } else {
        eprintln!(
            "[VibeAround] Wrote MCP config for {} at {:?}",
            kind, path
        );
    }
}
