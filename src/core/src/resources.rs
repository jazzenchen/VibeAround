//! Resource loader — single source of truth for agent, tunnel, plugin,
//! MCP tool, command, and PTY environment definitions.
//!
//! All data is embedded at compile time via `include_str!` and parsed
//! once on first access via `LazyLock`.

use std::collections::HashMap;
use std::sync::LazyLock;

use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Embedded JSON sources
// ---------------------------------------------------------------------------

static AGENTS_JSON: &str = include_str!("../../resources/agents.json");
static TUNNELS_JSON: &str = include_str!("../../resources/tunnels.json");
static PLUGINS_JSON: &str = include_str!("../../resources/plugins.json");
static MCP_TOOLS_JSON: &str = include_str!("../../resources/mcp-tools.json");
static COMMANDS_JSON: &str = include_str!("../../resources/commands.json");
static PTY_ENV_JSON: &str = include_str!("../../resources/pty-env.json");

// ---------------------------------------------------------------------------
// Data types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AgentDef {
    pub id: String,
    pub display_name: String,
    pub description: String,
    #[serde(default)]
    pub aliases: Vec<String>,
    #[serde(default)]
    pub install: Option<AgentInstallInfo>,
    pub acp: AgentAcpConfig,
    pub pty: AgentPtyConfig,
    #[serde(default)]
    pub resume_template: Option<String>,
    #[serde(default)]
    pub global_config: Option<AgentGlobalConfig>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AgentInstallInfo {
    /// Install type: "npm" | "script" | "path"
    #[serde(rename = "type")]
    pub install_type: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AgentAcpConfig {
    pub program: String,
    #[serde(default)]
    pub args: Vec<String>,
    /// If set, the agent is an npm package that should be pre-installed
    /// into `~/.vibearound/plugins/` during onboarding.
    pub npm_package: Option<String>,
    /// Binary name inside `node_modules/.bin/` (defaults to last segment of npm_package).
    pub bin_name: Option<String>,
    /// Shell command to install the agent binary (e.g. "curl ... | bash").
    /// Run during onboarding when the user enables this agent.
    #[serde(default)]
    pub install_cmd: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AgentPtyConfig {
    pub command: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AgentGlobalConfig {
    pub settings_path: String,
    /// Legacy config path — also written to for backward compat (e.g. older Claude Code).
    #[serde(default)]
    pub settings_path_legacy: Option<String>,
    /// Config file format: "json" (default) or "toml".
    #[serde(default)]
    pub settings_format: Option<String>,
    pub mcp_key: String,
    pub mcp_entry: serde_json::Value,
    #[serde(default)]
    pub skill_dir: Option<String>,
    /// Skill filename (default: "SKILL.md"). Override for agents using different
    /// rule formats (e.g. "vibearound.mdc" for Cursor, "vibearound.md" for Kiro).
    #[serde(default)]
    pub skill_filename: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct TunnelDef {
    pub id: String,
    pub display_name: String,
    #[serde(default)]
    pub program: Option<String>,
    #[serde(default)]
    pub args: Option<Vec<String>>,
    #[serde(default)]
    pub spawn_error_hint: Option<String>,
    #[serde(default)]
    pub platform_overrides: Option<HashMap<String, TunnelOverride>>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct TunnelOverride {
    #[serde(default)]
    pub spawn_error_hint: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct PluginDef {
    pub id: String,
    #[serde(default = "default_plugin_kind")]
    pub kind: String,
    #[serde(default)]
    pub slug: Option<String>,
    pub name: String,
    pub description: String,
    pub github: String,
    #[serde(default)]
    pub install_steps: Vec<String>,
}

impl PluginDef {
    pub fn is_kind(&self, kind: &str) -> bool {
        self.kind == kind
    }

    pub fn install_dir_name(&self) -> &str {
        self.slug.as_deref().unwrap_or(&self.id)
    }
}

fn default_plugin_kind() -> String {
    "channel".to_string()
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct McpToolDef {
    pub name: String,
    pub description: String,
    #[serde(rename = "inputSchema")]
    pub input_schema: serde_json::Value,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct CommandsDef {
    pub system_commands: Vec<CommandEntry>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct CommandEntry {
    pub name: String,
    pub description: String,
    #[serde(default)]
    pub args: Option<String>,
    #[serde(default)]
    pub aliases: Option<Vec<String>>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct PtyEnvDef {
    pub env: HashMap<String, String>,
    pub themes: HashMap<String, PtyTheme>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct PtyTheme {
    pub fg: String,
    pub bg: String,
    #[serde(rename = "COLORFGBG")]
    pub colorfgbg: String,
}

// ---------------------------------------------------------------------------
// Parsed statics — parsed once on first access
// ---------------------------------------------------------------------------

pub static AGENTS: LazyLock<Vec<AgentDef>> =
    LazyLock::new(|| serde_json::from_str(AGENTS_JSON).expect("Failed to parse agents.json"));

pub static TUNNELS: LazyLock<Vec<TunnelDef>> =
    LazyLock::new(|| serde_json::from_str(TUNNELS_JSON).expect("Failed to parse tunnels.json"));

pub static PLUGINS: LazyLock<Vec<PluginDef>> =
    LazyLock::new(|| serde_json::from_str(PLUGINS_JSON).expect("Failed to parse plugins.json"));

pub static MCP_TOOLS: LazyLock<Vec<McpToolDef>> =
    LazyLock::new(|| serde_json::from_str(MCP_TOOLS_JSON).expect("Failed to parse mcp-tools.json"));

pub static COMMANDS: LazyLock<CommandsDef> =
    LazyLock::new(|| serde_json::from_str(COMMANDS_JSON).expect("Failed to parse commands.json"));

pub static PTY_ENV: LazyLock<PtyEnvDef> =
    LazyLock::new(|| serde_json::from_str(PTY_ENV_JSON).expect("Failed to parse pty-env.json"));

// ---------------------------------------------------------------------------
// Lookup helpers
// ---------------------------------------------------------------------------

/// Find an agent definition by ID.
pub fn agent_by_id(id: &str) -> Option<&'static AgentDef> {
    AGENTS.iter().find(|a| a.id == id)
}

/// Find an agent definition by any alias (including the primary ID).
pub fn agent_by_alias(alias: &str) -> Option<&'static AgentDef> {
    let lower = alias.trim().to_lowercase();
    AGENTS
        .iter()
        .find(|a| a.id == lower || a.aliases.iter().any(|al| al == &lower))
}

/// Resolve an agent alias to the canonical agent ID.
pub fn resolve_agent_id(alias: &str) -> Result<String, String> {
    let trimmed = alias.trim();
    agent_by_alias(trimmed)
        .map(|def| def.id.clone())
        .ok_or_else(|| format!("Unknown agent '{}'", trimmed))
}

/// Get all agent IDs.
pub fn agent_ids() -> Vec<&'static str> {
    AGENTS.iter().map(|a| a.id.as_str()).collect()
}

/// Find a tunnel definition by ID.
pub fn tunnel_by_id(id: &str) -> Option<&'static TunnelDef> {
    TUNNELS.iter().find(|t| t.id == id)
}

/// Find a plugin definition by ID.
pub fn plugin_by_id(id: &str) -> Option<&'static PluginDef> {
    PLUGINS.iter().find(|p| p.id == id)
}

/// Resolve a tunnel's spawn error hint for the current platform.
pub fn tunnel_spawn_error_hint(tunnel: &TunnelDef) -> Option<&str> {
    // Check platform-specific override first
    if let Some(overrides) = &tunnel.platform_overrides {
        let platform = if cfg!(target_os = "windows") {
            "windows"
        } else if cfg!(target_os = "macos") {
            "macos"
        } else {
            "linux"
        };
        if let Some(ov) = overrides.get(platform) {
            if let Some(hint) = &ov.spawn_error_hint {
                return Some(hint.as_str());
            }
        }
    }
    tunnel.spawn_error_hint.as_deref()
}

/// Build the MCP tools list JSON value, injecting agent IDs into enum fields.
pub fn mcp_tools_list_json() -> serde_json::Value {
    let agent_ids: Vec<serde_json::Value> = agent_ids()
        .iter()
        .map(|id| serde_json::Value::String(id.to_string()))
        .collect();

    let mut tools: Vec<serde_json::Value> = MCP_TOOLS
        .iter()
        .map(|t| serde_json::to_value(t).unwrap())
        .collect();

    // Inject agent_kind enum values wherever tool schemas expose an agent selector.
    for tool in &mut tools {
        if let Some(schema) = tool.get_mut("inputSchema") {
            inject_agent_schema_enums(schema, &agent_ids);
        }
    }

    serde_json::json!({ "tools": tools })
}

fn inject_agent_schema_enums(schema: &mut serde_json::Value, agent_ids: &[serde_json::Value]) {
    match schema {
        serde_json::Value::Object(obj) => {
            if let Some(props) = obj.get_mut("properties").and_then(|v| v.as_object_mut()) {
                for key in ["agent_kind", "kind"] {
                    if let Some(prop) = props.get_mut(key).and_then(|v| v.as_object_mut()) {
                        prop.insert(
                            "enum".to_string(),
                            serde_json::Value::Array(agent_ids.to_vec()),
                        );
                    }
                }
            }
            for value in obj.values_mut() {
                inject_agent_schema_enums(value, agent_ids);
            }
        }
        serde_json::Value::Array(items) => {
            for item in items {
                inject_agent_schema_enums(item, agent_ids);
            }
        }
        _ => {}
    }
}

/// Format system commands into help text.
pub fn format_system_commands_help() -> String {
    let mut lines = Vec::new();
    for cmd in &COMMANDS.system_commands {
        let usage = match &cmd.args {
            Some(args) => format!("  /{} {} — {}", cmd.name, args, cmd.description),
            None => format!("  /{} — {}", cmd.name, cmd.description),
        };
        lines.push(usage);
    }
    lines.join("\n")
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_json_files_parse() {
        // Accessing the statics triggers parsing; .expect() will panic on failure
        assert!(!AGENTS.is_empty(), "agents.json should not be empty");
        assert!(!TUNNELS.is_empty(), "tunnels.json should not be empty");
        assert!(!PLUGINS.is_empty(), "plugins.json should not be empty");
        assert!(!MCP_TOOLS.is_empty(), "mcp-tools.json should not be empty");
        assert!(
            !COMMANDS.system_commands.is_empty(),
            "commands.json should not be empty"
        );
        assert!(
            !PTY_ENV.env.is_empty(),
            "pty-env.json env should not be empty"
        );
        assert!(
            !PTY_ENV.themes.is_empty(),
            "pty-env.json themes should not be empty"
        );
    }

    #[test]
    fn agent_lookup_works() {
        assert!(agent_by_id("claude").is_some());
        assert!(agent_by_id("pi").is_some());
        assert!(agent_by_id("gemini").is_some());
        assert!(agent_by_alias("claude-code").is_some());
        assert!(agent_by_alias("pi-coding-agent").is_some());
        assert!(agent_by_alias("nonexistent").is_none());
    }

    #[test]
    fn plugin_registry_uses_kind_and_slug() {
        let telegram = plugin_by_id("telegram").expect("telegram plugin must exist");
        assert!(telegram.is_kind("channel"));
        assert_eq!(telegram.install_dir_name(), "va-plugin-channel-telegram");

        assert!(PLUGINS.iter().all(|plugin| plugin.is_kind("channel")));
        assert!(
            plugin_by_id("deepseek").is_none(),
            "DeepSeek is a built-in profile catalog entry, not an installable plugin"
        );
    }

    #[test]
    fn mcp_tools_list_injects_agent_enums() {
        let tools = mcp_tools_list_json();
        let tools_arr = tools["tools"].as_array().unwrap();
        // Find a tool with agent_kind property
        let handover = tools_arr
            .iter()
            .find(|t| t["name"] == "prepare_handover")
            .unwrap();
        let agent_kind_enum = &handover["inputSchema"]["properties"]["agent_kind"]["enum"];
        assert!(agent_kind_enum.is_array());
        let ids: Vec<&str> = agent_kind_enum
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|v| v.as_str())
            .collect();
        assert!(ids.contains(&"claude"));
        assert!(ids.contains(&"gemini"));
    }
}
