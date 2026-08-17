//! Resource loader — single source of truth for agent, tunnel, plugin,
//! MCP tool, command, and PTY environment definitions.
//!
//! All data is embedded at compile time via `include_str!` and parsed
//! once on first access via `LazyLock`.

use std::collections::{BTreeMap, HashMap};
use std::sync::LazyLock;

use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Embedded JSON sources
// ---------------------------------------------------------------------------

static AGENTS_JSON: &str = include_str!("../../resources/agents.json");
static AGENT_LAUNCH_JSON: &str = include_str!("../../resources/agent-launch.json");
static TUNNELS_JSON: &str = include_str!("../../resources/tunnels.json");
static PLUGINS_JSON: &str = include_str!("../../resources/plugins.json");
static MCP_TOOLS_JSON: &str = include_str!("../../resources/mcp-tools.json");
static COMMANDS_JSON: &str = include_str!("../../resources/commands.json");
static PTY_ENV_JSON: &str = include_str!("../../resources/pty-env.json");

pub const CHATGPT_DESKTOP_MACOS_APP_NAME: &str = "ChatGPT";
pub const CHATGPT_DESKTOP_MACOS_BUNDLE_ID: &str = "com.openai.codex";
pub const CHATGPT_DESKTOP_WINDOWS_PACKAGE_FAMILY: &str = "OpenAI.Codex_2p2nqsd0c76g0";

pub fn chatgpt_desktop_windows_start_app_query() -> String {
    format!(
        "$app = Get-StartApps | Where-Object {{ $_.AppID -like '{}!*' }} | Select-Object -First 1; if (-not $app) {{ $app = Get-StartApps -Name 'Codex' | Select-Object -First 1 }}; if ($app) {{ $app.AppID }}",
        CHATGPT_DESKTOP_WINDOWS_PACKAGE_FAMILY
    )
}

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
    pub platforms: Vec<String>,
    #[serde(default)]
    pub direct_only: bool,
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
    #[serde(default)]
    pub platform_commands: HashMap<String, String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct AgentLaunchConfig {
    #[serde(default)]
    pub command: Option<String>,
    #[serde(default)]
    pub platform_commands: HashMap<String, String>,
    #[serde(default)]
    pub terminal_commands: HashMap<String, String>,
    #[serde(default)]
    pub platform_terminal_commands: HashMap<String, HashMap<String, String>>,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub platform_args: HashMap<String, Vec<String>>,
    #[serde(default)]
    pub terminal_args: HashMap<String, Vec<String>>,
    #[serde(default)]
    pub platform_terminal_args: HashMap<String, HashMap<String, Vec<String>>>,
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    #[serde(default)]
    pub platform_env: HashMap<String, BTreeMap<String, String>>,
    #[serde(default)]
    pub terminal_env: HashMap<String, BTreeMap<String, String>>,
    #[serde(default)]
    pub platform_terminal_env: HashMap<String, HashMap<String, BTreeMap<String, String>>>,
    #[serde(default)]
    pub resume: Option<AgentLaunchResumeConfig>,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct AgentLaunchResumeConfig {
    #[serde(default)]
    pub command: Option<String>,
    #[serde(default)]
    pub platform_commands: HashMap<String, String>,
    #[serde(default)]
    pub terminal_commands: HashMap<String, String>,
    #[serde(default)]
    pub platform_terminal_commands: HashMap<String, HashMap<String, String>>,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub platform_args: HashMap<String, Vec<String>>,
    #[serde(default)]
    pub terminal_args: HashMap<String, Vec<String>>,
    #[serde(default)]
    pub platform_terminal_args: HashMap<String, HashMap<String, Vec<String>>>,
}

impl AgentDef {
    pub fn supports_current_platform(&self) -> bool {
        self.platforms.is_empty()
            || self
                .platforms
                .iter()
                .any(|platform| platform == current_platform())
    }

    pub fn supports_acp_runtime(&self) -> bool {
        !self.direct_only && !self.acp.program.trim().is_empty()
    }

    pub fn pty_command_for_current_platform(&self) -> &str {
        self.pty
            .platform_commands
            .get(current_platform())
            .map(String::as_str)
            .unwrap_or(self.pty.command.as_str())
    }

    pub fn launch_command_for_current_platform(&self) -> &str {
        self.launch_command_for_terminal(None)
    }

    pub fn launch_command_for_terminal(&self, terminal_id: Option<&str>) -> &str {
        self.launch_config()
            .and_then(|launch| launch.command_for_terminal(terminal_id))
            .unwrap_or_else(|| self.pty_command_for_current_platform())
    }

    pub fn launch_args_for_current_platform(&self) -> Vec<String> {
        self.launch_args_for_terminal(None)
    }

    pub fn launch_args_for_terminal(&self, terminal_id: Option<&str>) -> Vec<String> {
        self.launch_config()
            .map(|launch| launch.args_for_terminal(terminal_id))
            .unwrap_or_default()
    }

    pub fn launch_env_for_current_platform(&self) -> Vec<(String, String)> {
        self.launch_env_for_terminal(None)
    }

    pub fn launch_env_for_terminal(&self, terminal_id: Option<&str>) -> Vec<(String, String)> {
        let Some(launch) = self.launch_config() else {
            return Vec::new();
        };
        launch.env_for_terminal(terminal_id)
    }

    pub fn launch_resume_for_current_platform(
        &self,
        session_id: &str,
    ) -> Option<(String, Vec<String>)> {
        self.launch_resume_for_terminal(session_id, None)
    }

    pub fn launch_resume_for_terminal(
        &self,
        session_id: &str,
        terminal_id: Option<&str>,
    ) -> Option<(String, Vec<String>)> {
        let launch = self.launch_config()?;
        let resume = launch.resume.as_ref()?;
        let command = resume
            .command_for_terminal(terminal_id)
            .or_else(|| launch.command_for_terminal(terminal_id))
            .unwrap_or_else(|| self.pty_command_for_current_platform())
            .to_string();
        let args = resume
            .args_for_terminal(terminal_id)
            .iter()
            .map(|arg| render_launch_template_arg(arg, session_id))
            .collect();
        Some((command, args))
    }

    fn launch_config(&self) -> Option<&'static AgentLaunchConfig> {
        agent_launch_by_id(&self.id)
    }
}

impl AgentLaunchConfig {
    fn command_for_terminal(&self, terminal_id: Option<&str>) -> Option<&str> {
        if let Some(terminal_id) = terminal_id {
            if let Some(command) = self
                .platform_terminal_commands
                .get(current_platform())
                .and_then(|platform| platform.get(terminal_id))
            {
                return Some(command.as_str());
            }
            if let Some(command) = self.terminal_commands.get(terminal_id) {
                return Some(command.as_str());
            }
        }
        self.platform_commands
            .get(current_platform())
            .or(self.command.as_ref())
            .map(String::as_str)
    }

    fn args_for_terminal(&self, terminal_id: Option<&str>) -> Vec<String> {
        let mut args = self.args.clone();
        if let Some(platform_args) = self.platform_args.get(current_platform()) {
            args.extend(platform_args.clone());
        }
        if let Some(terminal_id) = terminal_id {
            if let Some(terminal_args) = self.terminal_args.get(terminal_id) {
                args.extend(terminal_args.clone());
            }
            if let Some(platform_terminal_args) = self
                .platform_terminal_args
                .get(current_platform())
                .and_then(|platform| platform.get(terminal_id))
            {
                args.extend(platform_terminal_args.clone());
            }
        }
        args
    }

    fn env_for_terminal(&self, terminal_id: Option<&str>) -> Vec<(String, String)> {
        let mut env = self.env.clone();
        if let Some(platform_env) = self.platform_env.get(current_platform()) {
            env.extend(platform_env.clone());
        }
        if let Some(terminal_id) = terminal_id {
            if let Some(terminal_env) = self.terminal_env.get(terminal_id) {
                env.extend(terminal_env.clone());
            }
            if let Some(platform_terminal_env) = self
                .platform_terminal_env
                .get(current_platform())
                .and_then(|platform| platform.get(terminal_id))
            {
                env.extend(platform_terminal_env.clone());
            }
        }
        env.into_iter().collect()
    }
}

impl AgentLaunchResumeConfig {
    fn command_for_terminal(&self, terminal_id: Option<&str>) -> Option<&str> {
        if let Some(terminal_id) = terminal_id {
            if let Some(command) = self
                .platform_terminal_commands
                .get(current_platform())
                .and_then(|platform| platform.get(terminal_id))
            {
                return Some(command.as_str());
            }
            if let Some(command) = self.terminal_commands.get(terminal_id) {
                return Some(command.as_str());
            }
        }
        self.platform_commands
            .get(current_platform())
            .or(self.command.as_ref())
            .map(String::as_str)
    }

    fn args_for_terminal(&self, terminal_id: Option<&str>) -> Vec<String> {
        let mut args = self
            .platform_args
            .get(current_platform())
            .cloned()
            .unwrap_or_else(|| self.args.clone());
        if let Some(terminal_id) = terminal_id {
            if let Some(terminal_args) = self.terminal_args.get(terminal_id) {
                args.extend(terminal_args.clone());
            }
            if let Some(platform_terminal_args) = self
                .platform_terminal_args
                .get(current_platform())
                .and_then(|platform| platform.get(terminal_id))
            {
                args.extend(platform_terminal_args.clone());
            }
        }
        args
    }
}

fn render_launch_template_arg(arg: &str, session_id: &str) -> String {
    arg.replace("{session_id}", session_id)
}

fn current_platform() -> &'static str {
    if cfg!(target_os = "windows") {
        "windows"
    } else if cfg!(target_os = "macos") {
        "macos"
    } else {
        "linux"
    }
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
    /// Optional project-scoped skill directory. Some agents keep global or
    /// legacy skills in a different location than repo-shared skills.
    #[serde(default)]
    pub project_skill_dir: Option<String>,
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
    pub dependency_id: Option<String>,
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
    /// Immutable source commit used by the managed plugin installer.
    pub revision: String,
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

pub static AGENT_LAUNCHES: LazyLock<HashMap<String, AgentLaunchConfig>> = LazyLock::new(|| {
    serde_json::from_str(AGENT_LAUNCH_JSON).expect("Failed to parse agent-launch.json")
});

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

/// Find the external va-launch template for an agent by ID or alias.
pub fn agent_launch_by_id(id: &str) -> Option<&'static AgentLaunchConfig> {
    let canonical = resolve_agent_id(id).unwrap_or_else(|_| id.trim().to_lowercase());
    AGENT_LAUNCHES.get(canonical.as_str())
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

pub fn validate_acp_runtime_agent(agent_id: &str) -> Result<&'static AgentDef, String> {
    let agent =
        agent_by_id(agent_id).ok_or_else(|| format!("Unknown agent '{}'", agent_id.trim()))?;
    if agent.supports_acp_runtime() {
        Ok(agent)
    } else {
        Err(acp_runtime_agent_error(agent))
    }
}

pub fn acp_runtime_agent_error(agent: &AgentDef) -> String {
    format!(
        "{} can only be opened directly from the local desktop app. It cannot run as an IM/channel agent because it does not expose an ACP runtime. Please choose an ACP-compatible agent such as Codex CLI.",
        agent.display_name
    )
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
        assert!(
            !AGENT_LAUNCHES.is_empty(),
            "agent-launch.json should not be empty"
        );
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
    fn launch_templates_extend_without_rewriting_pty_commands() {
        let codex = agent_by_id("codex").expect("codex agent");
        assert!(agent_launch_by_id("openai-codex").is_some());
        assert_eq!(codex.pty_command_for_current_platform(), "codex");
        assert_eq!(codex.launch_command_for_current_platform(), "codex");
        assert_eq!(
            codex.launch_args_for_current_platform(),
            vec!["-c", "check_for_update_on_startup=false"]
        );

        let claude = agent_by_id("claude").expect("claude agent");
        assert_eq!(
            claude.pty_command_for_current_platform(),
            "claude code --permission-mode acceptEdits"
        );
        assert_eq!(
            claude.launch_env_for_current_platform(),
            vec![
                ("DISABLE_AUTOUPDATER".to_string(), "1".to_string()),
                ("DISABLE_UPDATES".to_string(), "1".to_string())
            ]
        );
    }

    #[test]
    fn launch_templates_can_override_commands_by_terminal() {
        let platform = current_platform().to_string();
        let mut platform_terminal_commands = HashMap::new();
        platform_terminal_commands.insert(
            platform.clone(),
            HashMap::from([("web-pty".to_string(), "platform-web".to_string())]),
        );
        let launch = AgentLaunchConfig {
            command: Some("base".to_string()),
            platform_commands: HashMap::from([(platform, "platform".to_string())]),
            terminal_commands: HashMap::from([
                ("web-pty".to_string(), "terminal-web".to_string()),
                ("native".to_string(), "terminal-native".to_string()),
            ]),
            platform_terminal_commands,
            ..Default::default()
        };

        assert_eq!(launch.command_for_terminal(None), Some("platform"));
        assert_eq!(
            launch.command_for_terminal(Some("native")),
            Some("terminal-native")
        );
        assert_eq!(
            launch.command_for_terminal(Some("web-pty")),
            Some("platform-web")
        );
    }

    #[test]
    fn launch_templates_reference_registered_agents() {
        for agent_id in AGENT_LAUNCHES.keys() {
            assert!(
                agent_by_id(agent_id).is_some(),
                "launch template references unknown agent '{}'",
                agent_id
            );
        }
    }

    #[test]
    fn direct_only_agents_are_not_acp_runtime_agents() {
        assert!(agent_by_id("codex").unwrap().supports_acp_runtime());
        assert!(!agent_by_id("codex-desktop").unwrap().supports_acp_runtime());

        let error = validate_acp_runtime_agent("codex-desktop").unwrap_err();
        assert!(error.contains("ChatGPT Desktop (Codex) can only be opened directly"));
        assert!(error.contains("IM/channel agent"));
    }

    #[test]
    fn chatgpt_windows_lookup_uses_store_identity_with_legacy_fallback() {
        let query = chatgpt_desktop_windows_start_app_query();
        assert!(query.contains("OpenAI.Codex_2p2nqsd0c76g0!*"));
        assert!(query.contains("Get-StartApps -Name 'Codex'"));
    }

    #[test]
    fn plugin_registry_uses_kind_and_slug() {
        let telegram = plugin_by_id("telegram").expect("telegram plugin must exist");
        assert!(telegram.is_kind("channel"));
        assert_eq!(telegram.install_dir_name(), "va-plugin-channel-telegram");

        assert!(PLUGINS
            .iter()
            .all(|plugin| plugin.is_kind("channel") || plugin.is_kind("search")));
        assert!(plugin_by_id("va-search-tool").is_some_and(|plugin| plugin.is_kind("search")));
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
