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

You have access to the following MCP tool for dispatching work to worker agents:

## send_to_worker
Send a message to a worker agent on a specific project workspace.
- Parameters:
  - `workspace` (required): the project directory path, e.g. "/Users/me/projects/myapp"
  - `message` (required): the task or question for the worker
  - `kind` (optional): agent type, e.g. "claude", "gemini". If omitted, VibeAround picks the best available agent on that workspace.
- Returns: the worker's complete output text
- If no worker is running on the given workspace, one will be auto-spawned.

Other management operations (spawning workers, killing workers, listing workers) are handled by VibeAround's IM commands (/spawn, /kill, /workers), not by you directly."#;

const DEFAULT_WORKFLOW: &str = r#"# Workflow

## Workspace management
- Your current working directory is `~/.vibearound/`. Do NOT create project files here directly.
- When the user asks to create a new project, app, demo, or any code that requires writing files:
  1. First create a new directory under `~/.vibearound/workspaces/`, e.g. `~/.vibearound/workspaces/vue-todo-demo/`
  2. Do all file creation and coding work inside that new workspace directory.
  3. Tell the user where the project was created.
- When the user asks to work on an existing project at a specific path, dispatch to a worker on that path.

## When to dispatch to a worker
- When the user asks to work on a specific project, dispatch to the worker assigned to that project's workspace.
- When the user asks to create, modify, or debug code, use a worker agent.
- If no worker exists for the requested project, spawn one first.

## When to handle directly
- General questions, planning, architecture discussions.
- Managing workers (listing, spawning, killing).
- Summarizing work done by workers.
- Anything that doesn't require file system access to a specific project."#;

const DEFAULT_MEMORY: &str = r#"# Memory

(This file is for long-term memory. You can write notes here about user preferences, project context, and lessons learned. This file persists across sessions.)"#;

const DEFAULT_RULES: &str = r#"# Rules

- Always confirm before killing a worker that might have unsaved work.
- When dispatching to a worker, briefly tell the user which worker you're using.
- If a worker fails or crashes, report the error and offer to restart.
- Do not modify files outside of `~/.vibearound/` without explicit user permission.
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
                r#"{{"vibearound":{{"type":"http","url":"{}"}}}}"#,
                url
            ),
        ),
        AgentKind::Gemini => (
            ".gemini/settings.json",
            format!(
                r#"{{"mcpServers":{{"vibearound":{{"url":"{}"}}}}}}"#,
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
