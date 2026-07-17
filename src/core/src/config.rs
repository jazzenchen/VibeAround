//! Config loading helpers.
//! All config comes from ~/.vibearound/settings.json.
//! Callers load a fresh Config when they need one.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Once};
use std::time::{SystemTime, UNIX_EPOCH};

use parking_lot::RwLock;
use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::tunnels::TunnelProvider;

/// Global config cache. Populated on first `ensure_loaded()` call, reloaded
/// by `reload()` or automatically after `update_settings_json()`.
static CONFIG_CACHE: RwLock<Option<Arc<Config>>> = RwLock::new(None);

#[derive(Debug, Clone, Serialize)]
pub struct SettingsSnapshot {
    pub settings: serde_json::Value,
    pub revision: String,
}

#[derive(Debug, Clone)]
pub enum SettingsReplaceResult {
    Replaced(SettingsSnapshot),
    Conflict(SettingsSnapshot),
}

/// Default server port for both standalone server and desktop-spawned server.
pub const DEFAULT_PORT: u16 = 12358;

/// Minimal default settings.json content, embedded at compile time.
const DEFAULT_SETTINGS_JSON: &str = r#"{
  "workspaces": []
}"#;

/// User home directory (HOME on Unix, USERPROFILE on Windows).
pub fn home_dir() -> PathBuf {
    PathBuf::from(
        std::env::var("HOME")
            .or_else(|_| std::env::var("USERPROFILE"))
            .unwrap_or_else(|_| "/tmp".into()),
    )
}

/// Data directory: ~/.vibearound
pub fn data_dir() -> PathBuf {
    if let Ok(path) = std::env::var("VIBEAROUND_DATA_DIR") {
        let path = path.trim();
        if !path.is_empty() {
            return expand_home(path);
        }
    }
    home_dir().join(".vibearound")
}

/// Path to the primary settings file.
pub fn settings_path() -> PathBuf {
    data_dir().join("settings.json")
}

/// Runtime state directory for append-only stores and other non-config data.
pub fn state_dir() -> PathBuf {
    data_dir().join("state")
}

pub fn state_file(name: &str) -> PathBuf {
    state_dir().join(name)
}

pub fn legacy_state_file(name: &str) -> PathBuf {
    data_dir().join(name)
}

pub fn migrate_legacy_state_file(name: &str) -> PathBuf {
    let target = state_file(name);
    let legacy = legacy_state_file(name);
    if legacy.exists() {
        if let Some(parent) = target.parent() {
            if let Err(error) = std::fs::create_dir_all(parent) {
                tracing::warn!(path = ?parent, error = %error, "failed to create state dir");
                return target;
            }
        }
        if target.exists() {
            match archive_state_file(&legacy, "legacy-root") {
                Ok(archive) => {
                    tracing::info!(from = ?legacy, to = ?archive, "archived legacy state file")
                }
                Err(error) => {
                    tracing::warn!(from = ?legacy, error = %error, "failed to archive legacy state file")
                }
            }
        } else if let Err(error) = std::fs::rename(&legacy, &target) {
            tracing::warn!(from = ?legacy, to = ?target, error = %error, "failed to migrate legacy state file");
        } else {
            tracing::info!(from = ?legacy, to = ?target, "migrated legacy state file")
        }
    }
    target
}

pub fn archive_state_file(path: &Path, reason: &str) -> std::io::Result<PathBuf> {
    let archive_dir = state_dir().join("archive");
    std::fs::create_dir_all(&archive_dir)?;
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("state-file");
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    let archive = archive_dir.join(format!("{file_name}.{timestamp}.{reason}"));
    std::fs::rename(path, &archive)?;
    Ok(archive)
}

/// Ensure ~/.vibearound/ exists with settings.json and workspaces/.
fn init_data_dir() {
    let dir = data_dir();
    if let Err(e) = std::fs::create_dir_all(&dir) {
        tracing::info!("[VibeAround] Failed to create data dir {:?}: {}", dir, e);
        return;
    }
    let settings_path = settings_path();
    if !settings_path.exists() {
        tracing::info!(
            "[VibeAround] Creating default settings.json at {:?}",
            settings_path
        );
        if let Err(e) = mutate_settings_json_at(&settings_path, |_| Ok(())) {
            tracing::info!("[VibeAround] Failed to initialize settings.json: {}", e);
        }
    }
    let ws_dir = dir.join("workspaces");
    if let Err(e) = std::fs::create_dir_all(&ws_dir) {
        tracing::info!("[VibeAround] Failed to create workspaces dir: {}", e);
    }
    let state_dir = state_dir();
    if let Err(e) = std::fs::create_dir_all(&state_dir) {
        tracing::info!("[VibeAround] Failed to create state dir: {}", e);
    }
}

/// Install rustls default crypto provider once.
fn ensure_rustls_provider() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        rustls::crypto::aws_lc_rs::default_provider()
            .install_default()
            .expect("rustls default crypto provider");
    });
}

/// Cached config from settings.json.
#[derive(Clone)]
pub struct Config {
    // --- Tunnel ---
    pub tunnel_provider: TunnelProvider,
    pub ngrok_auth_token: Option<String>,
    pub ngrok_domain: Option<String>,
    pub cloudflare_tunnel_token: Option<String>,
    pub cloudflare_hostname: Option<String>,
    pub toolchain_mode: ToolchainMode,
    pub portable_toolchain: bool,
    // --- Workspaces ---
    /// Default workspace root for new agent sessions.
    pub default_workspace: PathBuf,
    /// User-added project folders.
    pub workspaces: Vec<PathBuf>,
    pub preview_base_url: Option<String>,
    pub tmux_detach_others: bool,
    // --- Agents ---
    pub default_agent: String,
    /// Subset of agent IDs from `resources/agents.json` the user has enabled.
    /// Validated at load time — entries that don't resolve via
    /// `resources::agent_by_alias` are dropped.
    pub enabled_agents: Vec<String>,
    // --- Agent integrations ---
    pub integrations: AgentIntegrationsConfig,
    // --- Optional outbound HTTP proxy ---
    pub proxy: HttpProxyConfig,
    // --- API bridge behavior ---
    pub api_bridge: ApiBridgeConfig,
    // --- Local ACP-to-API service ---
    pub local_agent_api: LocalAgentApiConfig,
    // --- Host-side web search fallback ---
    pub search_tool: SearchToolConfig,
    // --- Remote/IM channel defaults ---
    pub remote: RemoteConfig,
    // --- Raw channels JSON (for dynamic plugin config) ---
    raw_channels: serde_json::Value,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WorkspaceSettings {
    pub default_workspace: PathBuf,
    pub workspaces: Vec<PathBuf>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ToolchainMode {
    #[default]
    System,
    Managed,
}

impl ToolchainMode {
    pub fn from_config(value: &str) -> Self {
        match value.trim().to_ascii_lowercase().as_str() {
            "managed" | "vibearound" | "vibearound_managed" => Self::Managed,
            _ => Self::System,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::System => "system",
            Self::Managed => "managed",
        }
    }

    pub fn is_managed(self) -> bool {
        matches!(self, Self::Managed)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AgentIntegrationsConfig {
    pub mcp_auto_install: bool,
    pub skill_auto_install: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ApiBridgeConfig {
    pub retry_429: Retry429Config,
    pub replace_provider_web_search: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LocalAgentApiConfig {
    pub enabled: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SearchToolConfig {
    pub stdio_path: Option<PathBuf>,
    pub max_results: Option<usize>,
    pub search_context_size: Option<String>,
    pub sources: BTreeMap<String, SearchSourceConfig>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RemoteConfig {
    pub channels: BTreeMap<String, RemoteChannelDefaults>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RemoteChannelDefaults {
    pub agent_id: Option<String>,
    pub profile_id: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SearchSourceConfig {
    pub enabled: bool,
    pub api_key: Option<String>,
    pub api_key_env: Option<String>,
    pub base_url: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Retry429Config {
    pub enabled: bool,
    pub max_retries: Option<usize>,
    pub delay_seconds: u64,
}

impl Default for AgentIntegrationsConfig {
    fn default() -> Self {
        Self {
            mcp_auto_install: true,
            skill_auto_install: true,
        }
    }
}

impl SearchToolConfig {
    pub fn has_enabled_sources(&self) -> bool {
        self.sources.values().any(|source| source.enabled)
    }

    pub fn enabled_source_names(&self) -> Vec<String> {
        let mut names = ["exa", "tavily", "grok", "brave"]
            .into_iter()
            .filter(|name| self.sources.get(*name).is_some_and(|source| source.enabled))
            .map(ToString::to_string)
            .collect::<Vec<_>>();
        names.extend(
            self.sources
                .iter()
                .filter(|(name, source)| {
                    source.enabled && !matches!(name.as_str(), "exa" | "tavily" | "grok" | "brave")
                })
                .map(|(name, _)| name.clone()),
        );
        names
    }
}

impl Default for Retry429Config {
    fn default() -> Self {
        Self {
            enabled: true,
            max_retries: Some(10),
            delay_seconds: 10,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct HttpProxyConfig {
    pub enabled: bool,
    pub http_proxy: Option<String>,
    pub no_proxy: Option<String>,
}

impl HttpProxyConfig {
    pub fn is_configured(&self) -> bool {
        self.enabled && self.http_proxy.is_some()
    }
}

impl Config {
    /// List all channel names configured in settings.json (e.g. ["feishu", "telegram"]).
    pub fn channel_names(&self) -> Vec<String> {
        self.raw_channels
            .as_object()
            .map(|obj| obj.keys().cloned().collect())
            .unwrap_or_default()
    }

    /// Get raw JSON config for a specific channel (e.g. channels.feishu → { app_id, app_secret, ... }).
    /// Passed directly to the plugin process via initialize.
    pub fn channel_raw_config(&self, name: &str) -> Option<serde_json::Value> {
        self.raw_channels.get(name).cloned()
    }

    /// Resolve the workspace directory for an agent session.
    pub fn resolve_workspace(&self, _agent_kind: &str) -> PathBuf {
        self.default_workspace.clone()
    }

    pub fn remote_channel_defaults(&self, channel_kind: &str) -> RemoteChannelDefaults {
        self.remote
            .channels
            .get(channel_kind)
            .cloned()
            .unwrap_or_default()
    }

    /// All available workspaces: the default root, built-in root, and user-added paths.
    pub fn all_workspaces(&self) -> Vec<PathBuf> {
        let builtin = builtin_workspaces_dir();
        let mut all = vec![self.default_workspace.clone()];
        if !all.contains(&builtin) {
            all.push(builtin);
        }
        for ws in &self.workspaces {
            if !all.contains(ws) {
                all.push(ws.clone());
            }
        }
        all
    }
}

/// Load config — returns cached version if available, otherwise reads from disk.
/// Call `reload()` to force a fresh read (e.g. after settings change).
pub fn ensure_loaded() -> Arc<Config> {
    // Fast path: return cached config.
    if let Some(cfg) = CONFIG_CACHE.read().as_ref() {
        return Arc::clone(cfg);
    }
    // Slow path: first call — initialize data dir, read from disk, cache.
    ensure_rustls_provider();
    init_data_dir();
    let path = settings_path();
    let cfg = Arc::new(load_settings_from(&path));
    *CONFIG_CACHE.write() = Some(Arc::clone(&cfg));
    cfg
}

/// Force re-read config from disk and update the cache.
/// Called after `update_settings_json()` and on daemon restart.
pub fn reload() -> Arc<Config> {
    ensure_rustls_provider();
    init_data_dir();
    let path = settings_path();
    let cfg = Arc::new(load_settings_from(&path));
    *CONFIG_CACHE.write() = Some(Arc::clone(&cfg));
    cfg
}

fn load_settings_from(path: &std::path::Path) -> Config {
    let Ok(data) = std::fs::read_to_string(path) else {
        return Config::default();
    };
    let Ok(root) = serde_json::from_str::<serde_json::Value>(&data) else {
        return Config::default();
    };

    let tunnel_provider = root
        .get("tunnel")
        .and_then(|t| t.get("provider"))
        .and_then(|p| p.as_str())
        .map(TunnelProvider::from_config)
        .unwrap_or_default();

    let tunnel_ngrok = root.get("tunnel").and_then(|t| t.get("ngrok"));
    let ngrok_auth_token = tunnel_ngrok
        .and_then(|n| n.get("auth_token"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .filter(|s| !s.is_empty());
    let ngrok_domain = tunnel_ngrok
        .and_then(|n| n.get("domain"))
        .and_then(|v| v.as_str())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());

    let tunnel_cloudflare = root.get("tunnel").and_then(|t| t.get("cloudflare"));
    let cloudflare_tunnel_token = tunnel_cloudflare
        .and_then(|c| c.get("tunnel_token"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .filter(|s| !s.is_empty());
    let cloudflare_hostname = tunnel_cloudflare
        .and_then(|c| c.get("hostname"))
        .and_then(|v| v.as_str())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    let startkit_settings = root.get("startkit");
    let toolchain_mode = startkit_settings
        .and_then(|value| value.get("toolchain_mode"))
        .and_then(|value| value.as_str())
        .map(ToolchainMode::from_config)
        .unwrap_or_default();
    let portable_toolchain = startkit_settings
        .and_then(|value| {
            value
                .get("portable_toolchain")
                .or_else(|| value.get("portableToolchain"))
        })
        .and_then(|value| value.as_bool())
        .unwrap_or_else(|| toolchain_mode.is_managed());

    let raw_channels = root
        .get("channels")
        .cloned()
        .unwrap_or(serde_json::Value::Object(serde_json::Map::new()));

    let workspace_settings = workspace_settings_from_json(&root);
    let default_workspace = workspace_settings.default_workspace;
    let workspaces = workspace_settings.workspaces;

    let preview_base_url = root
        .get("preview_base_url")
        .or_else(|| root.get("tunnel").and_then(|t| t.get("preview_base_url")))
        .and_then(|v| v.as_str())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());

    let tmux_detach_others = root
        .get("tmux")
        .and_then(|t| t.get("detach_others"))
        .and_then(|v| v.as_bool())
        .unwrap_or(true);

    let default_agent = root
        .get("default_agent")
        .and_then(|v| v.as_str())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "claude".to_string());

    let enabled_agents = root
        .get("enabled_agents")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str())
                .filter_map(|s| crate::resources::agent_by_alias(s).map(|def| def.id.clone()))
                .collect::<Vec<_>>()
        })
        .unwrap_or_else(|| {
            crate::resources::AGENTS
                .iter()
                .map(|a| a.id.clone())
                .collect()
        });

    let integrations = root
        .get("integrations")
        .and_then(|value| value.as_object())
        .map(|integrations| AgentIntegrationsConfig {
            mcp_auto_install: integrations
                .get("mcp_auto_install")
                .or_else(|| integrations.get("auto_install_mcp"))
                .and_then(|value| value.as_bool())
                .unwrap_or(true),
            skill_auto_install: integrations
                .get("skill_auto_install")
                .or_else(|| integrations.get("auto_install_skills"))
                .and_then(|value| value.as_bool())
                .unwrap_or(true),
        })
        .unwrap_or_default();

    let api_bridge = load_api_bridge_config(&root);
    let local_agent_api = load_local_agent_api_config(&root);
    let search_tool = load_search_tool_config(&root);
    let remote = load_remote_config(&root);

    let proxy = root
        .get("proxy")
        .and_then(|value| value.as_object())
        .map(|proxy| {
            let http_proxy = proxy
                .get("http_proxy")
                .or_else(|| proxy.get("url"))
                .and_then(|value| value.as_str())
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty());
            let no_proxy = proxy
                .get("no_proxy")
                .and_then(|value| value.as_str())
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty());
            let enabled = proxy
                .get("enabled")
                .and_then(|value| value.as_bool())
                .unwrap_or_else(|| http_proxy.is_some());
            HttpProxyConfig {
                enabled,
                http_proxy,
                no_proxy,
            }
        })
        .unwrap_or_default();

    Config {
        tunnel_provider,
        ngrok_auth_token,
        ngrok_domain,
        cloudflare_tunnel_token,
        cloudflare_hostname,
        toolchain_mode,
        portable_toolchain,
        default_workspace,
        workspaces,
        preview_base_url,
        tmux_detach_others,
        default_agent,
        enabled_agents,
        integrations,
        proxy,
        api_bridge,
        local_agent_api,
        search_tool,
        remote,
        raw_channels,
    }
}

pub fn workspace_settings_from_json(root: &serde_json::Value) -> WorkspaceSettings {
    let default_workspace = root
        .get("default_workspace")
        .and_then(|value| value.as_str())
        .map(|value| expand_home(value.trim()))
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or_else(builtin_workspaces_dir);
    let mut workspaces = root
        .get("workspaces")
        .and_then(|value| value.as_array())
        .map(|items| {
            items
                .iter()
                .filter_map(|value| value.as_str())
                .map(|value| expand_home(value.trim()))
                .filter(|path| !path.as_os_str().is_empty())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    workspaces.retain(|workspace| workspace != &default_workspace);

    WorkspaceSettings {
        default_workspace,
        workspaces,
    }
}

fn load_api_bridge_config(root: &serde_json::Value) -> ApiBridgeConfig {
    root.get("api_bridge")
        .or_else(|| root.get("bridge"))
        .and_then(|value| value.as_object())
        .map(|settings| ApiBridgeConfig {
            retry_429: settings
                .get("retry_429")
                .or_else(|| settings.get("rate_limit_retry"))
                .and_then(|value| value.as_object())
                .map(load_retry_429_config)
                .unwrap_or_default(),
            replace_provider_web_search: bool_setting(
                settings,
                &["replace_provider_web_search", "replaceProviderWebSearch"],
            )
            .unwrap_or(false),
        })
        .unwrap_or_default()
}

fn load_local_agent_api_config(root: &serde_json::Value) -> LocalAgentApiConfig {
    root.get("local_agent_api")
        .or_else(|| root.get("localAgentApi"))
        .or_else(|| root.get("local_api"))
        .and_then(|value| value.as_object())
        .map(|settings| LocalAgentApiConfig {
            enabled: settings
                .get("enabled")
                .and_then(|value| value.as_bool())
                .unwrap_or(false),
        })
        .unwrap_or_default()
}

fn load_search_tool_config(root: &serde_json::Value) -> SearchToolConfig {
    let Some(settings) = root
        .get("search_tool")
        .or_else(|| root.get("searchTool"))
        .and_then(|value| value.as_object())
    else {
        return SearchToolConfig::default();
    };

    let stdio_path = string_setting(settings, &["stdio_path", "stdioPath", "command"])
        .map(|value| expand_home(&value));
    let max_results = usize_setting(settings, &["max_results", "maxResults", "num_results"]);
    let search_context_size =
        search_context_size_setting(settings, &["search_context_size", "searchContextSize"]);
    let sources = settings
        .get("sources")
        .and_then(|value| value.as_object())
        .map(|sources| {
            sources
                .iter()
                .filter_map(|(name, value)| {
                    let name = normalize_search_source_name(name)?;
                    let settings = value.as_object()?;
                    Some((name, load_search_source_config(settings)))
                })
                .collect::<BTreeMap<_, _>>()
        })
        .unwrap_or_default();

    SearchToolConfig {
        stdio_path,
        max_results,
        search_context_size,
        sources,
    }
}

fn load_remote_config(root: &serde_json::Value) -> RemoteConfig {
    let channels = root
        .get("remote")
        .or_else(|| root.get("im_remote"))
        .and_then(|value| value.get("channels"))
        .and_then(|value| value.as_object())
        .map(|channels| {
            channels
                .iter()
                .filter_map(|(channel_kind, value)| {
                    let channel_kind = channel_kind.trim();
                    if channel_kind.is_empty() {
                        return None;
                    }
                    let settings = value.as_object()?;
                    Some((
                        channel_kind.to_string(),
                        load_remote_channel_defaults(settings),
                    ))
                })
                .collect::<BTreeMap<_, _>>()
        })
        .unwrap_or_default();

    RemoteConfig { channels }
}

fn load_remote_channel_defaults(
    settings: &serde_json::Map<String, serde_json::Value>,
) -> RemoteChannelDefaults {
    let agent_id = string_setting(settings, &["agent_id", "agentId", "agent"])
        .and_then(|agent| crate::resources::agent_by_alias(&agent).map(|def| def.id.clone()));
    let profile_id = string_setting(settings, &["profile_id", "profileId", "profile"]);

    RemoteChannelDefaults {
        agent_id,
        profile_id,
    }
}

fn load_search_source_config(
    settings: &serde_json::Map<String, serde_json::Value>,
) -> SearchSourceConfig {
    let api_key = string_setting(settings, &["api_key", "apiKey", "key"]);
    let api_key_env = string_setting(settings, &["api_key_env", "apiKeyEnv", "keyEnv"]);
    let base_url = string_setting(settings, &["base_url", "baseUrl", "url"]);
    SearchSourceConfig {
        enabled: settings
            .get("enabled")
            .and_then(|value| value.as_bool())
            .unwrap_or_else(|| api_key.is_some() || api_key_env.is_some()),
        api_key,
        api_key_env,
        base_url,
    }
}

fn string_setting(
    settings: &serde_json::Map<String, serde_json::Value>,
    keys: &[&str],
) -> Option<String> {
    keys.iter()
        .find_map(|key| settings.get(*key))
        .and_then(|value| value.as_str())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
}

fn bool_setting(
    settings: &serde_json::Map<String, serde_json::Value>,
    keys: &[&str],
) -> Option<bool> {
    keys.iter()
        .find_map(|key| settings.get(*key))
        .and_then(|value| value.as_bool())
}

fn usize_setting(
    settings: &serde_json::Map<String, serde_json::Value>,
    keys: &[&str],
) -> Option<usize> {
    keys.iter()
        .find_map(|key| settings.get(*key))
        .and_then(|value| value.as_u64())
        .map(|value| value as usize)
}

fn search_context_size_setting(
    settings: &serde_json::Map<String, serde_json::Value>,
    keys: &[&str],
) -> Option<String> {
    string_setting(settings, keys)
        .map(|value| value.to_ascii_lowercase())
        .filter(|value| matches!(value.as_str(), "low" | "medium" | "high"))
}

fn normalize_search_source_name(name: &str) -> Option<String> {
    let normalized = name.trim().to_ascii_lowercase();
    if normalized.is_empty() {
        None
    } else {
        Some(normalized)
    }
}

fn load_retry_429_config(settings: &serde_json::Map<String, serde_json::Value>) -> Retry429Config {
    let defaults = Retry429Config::default();
    Retry429Config {
        enabled: settings
            .get("enabled")
            .and_then(|value| value.as_bool())
            .unwrap_or(defaults.enabled),
        max_retries: retry_limit_setting(settings, defaults.max_retries),
        delay_seconds: settings
            .get("delay_seconds")
            .or_else(|| settings.get("delay"))
            .and_then(|value| value.as_u64())
            .unwrap_or(defaults.delay_seconds)
            .max(1),
    }
}

fn retry_limit_setting(
    settings: &serde_json::Map<String, serde_json::Value>,
    default: Option<usize>,
) -> Option<usize> {
    let Some(value) = settings
        .get("max_retries")
        .or_else(|| settings.get("retries"))
    else {
        return default;
    };
    if value.is_null() {
        None
    } else {
        value.as_u64().map(|value| value as usize).or(default)
    }
}

/// Base URL for preview links. Reads from the config cache.
pub fn preview_base_url() -> Option<String> {
    let cfg = ensure_loaded();
    cfg.preview_base_url
        .clone()
        .filter(|s| !s.is_empty())
        .or_else(|| {
            cfg.cloudflare_hostname
                .as_ref()
                .map(|h| format!("https://{}", h.trim()))
        })
        .or_else(|| {
            cfg.ngrok_domain
                .as_ref()
                .map(|d| format!("https://{}", d.trim()))
        })
}

/// Expand ~ to home directory in a path string.
fn expand_home(s: &str) -> PathBuf {
    if s == "~" {
        home_dir()
    } else if let Some(rest) = s.strip_prefix("~/") {
        home_dir().join(rest)
    } else {
        PathBuf::from(s)
    }
}

/// The built-in workspaces root: ~/.vibearound/workspaces/
pub fn builtin_workspaces_dir() -> PathBuf {
    data_dir().join("workspaces")
}

/// Mutate settings.json while holding its cross-process transaction lock.
///
/// The lock covers reading, strict parsing, mutation, and atomic replacement.
/// A missing file starts from the embedded default. Malformed JSON is returned
/// as an error and is never replaced. The mutator must not call another
/// settings writer while the lock is held.
pub fn mutate_settings_json<T>(
    mutator: impl FnOnce(&mut serde_json::Value) -> Result<T, String>,
) -> Result<T, String> {
    let result = mutate_settings_json_at(&settings_path(), mutator)?;
    *CONFIG_CACHE.write() = None;
    Ok(result)
}

/// Read and incrementally update the latest settings document.
pub fn update_settings_json(mutator: impl FnOnce(&mut serde_json::Value)) -> Result<(), String> {
    mutate_settings_json(|root| {
        mutator(root);
        Ok(())
    })
}

pub async fn update_settings_json_async(
    mutator: impl FnOnce(&mut serde_json::Value) + Send + 'static,
) -> Result<(), String> {
    tokio::task::spawn_blocking(move || update_settings_json(mutator))
        .await
        .map_err(|error| error.to_string())?
}

/// Remove a user workspace registration from settings.json.
///
/// This does not delete the directory on disk. Legacy workspace fields are
/// removed too because they are still read as regular workspace entries.
pub fn remove_workspace_path(path: &Path) -> Result<bool, String> {
    let mut removed = false;
    update_settings_json(|root| {
        removed = remove_workspace_from_settings(root, path);
    })?;
    Ok(removed)
}

/// Read the raw settings JSON file after ensuring the data directory exists.
pub fn read_settings_json() -> Result<serde_json::Value, String> {
    ensure_rustls_provider();
    init_data_dir();
    let path = settings_path();
    let data = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
    serde_json::from_str(&data).map_err(|e| e.to_string())
}

pub fn read_settings_snapshot() -> Result<SettingsSnapshot, String> {
    settings_snapshot(read_settings_json()?)
}

/// Apply an RFC 6902 JSON Patch to the latest settings document.
pub fn patch_settings_json(patch: &serde_json::Value) -> Result<SettingsSnapshot, String> {
    let snapshot = patch_settings_json_at(&settings_path(), patch)?;
    *CONFIG_CACHE.write() = None;
    Ok(snapshot)
}

/// Replace the complete settings document only when `expected_revision` still
/// matches the latest value.
pub fn replace_settings_json_if_revision(
    expected_revision: &str,
    replacement: &serde_json::Value,
) -> Result<SettingsReplaceResult, String> {
    let result =
        replace_settings_json_if_revision_at(&settings_path(), expected_revision, replacement)?;
    if matches!(result, SettingsReplaceResult::Replaced(_)) {
        *CONFIG_CACHE.write() = None;
    }
    Ok(result)
}

enum SettingsTransaction<T> {
    Read(T),
    Write(T),
}

fn mutate_settings_json_at<T>(
    path: &Path,
    mutator: impl FnOnce(&mut serde_json::Value) -> Result<T, String>,
) -> Result<T, String> {
    transact_settings_json_at(path, |root| mutator(root).map(SettingsTransaction::Write))
}

fn transact_settings_json_at<T>(
    path: &Path,
    operation: impl FnOnce(&mut serde_json::Value) -> Result<SettingsTransaction<T>, String>,
) -> Result<T, String> {
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    std::fs::create_dir_all(parent)
        .map_err(|error| format!("create {}: {error}", parent.display()))?;

    let lock_path = settings_lock_path(path);
    let _lock = crate::file_lock::ExclusiveFileLock::acquire(&lock_path)
        .map_err(|error| format!("lock {}: {error}", lock_path.display()))?;
    let data = match std::fs::read_to_string(path) {
        Ok(data) => data,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            DEFAULT_SETTINGS_JSON.to_string()
        }
        Err(error) => return Err(format!("read {}: {error}", path.display())),
    };
    let mut root: serde_json::Value = serde_json::from_str(&data)
        .map_err(|error| format!("parse {}: {error}", path.display()))?;
    if !root.is_object() {
        return Err(format!("{} must contain a JSON object", path.display()));
    }

    match operation(&mut root)? {
        SettingsTransaction::Read(result) => Ok(result),
        SettingsTransaction::Write(result) => {
            if !root.is_object() {
                return Err("settings.json root must remain a JSON object".to_string());
            }
            write_settings_json_to_path(path, &root)?;
            Ok(result)
        }
    }
}

fn patch_settings_json_at(
    path: &Path,
    patch: &serde_json::Value,
) -> Result<SettingsSnapshot, String> {
    let patch = serde_json::from_value::<json_patch::Patch>(patch.clone())
        .map_err(|error| format!("invalid settings patch: {error}"))?;
    mutate_settings_json_at(path, move |settings| {
        json_patch::patch(settings, &patch)
            .map_err(|error| format!("settings patch conflict: {error}"))?;
        settings_snapshot(settings.clone())
    })
}

fn replace_settings_json_if_revision_at(
    path: &Path,
    expected_revision: &str,
    replacement: &serde_json::Value,
) -> Result<SettingsReplaceResult, String> {
    if !replacement.is_object() {
        return Err("settings.json root must be a JSON object".to_string());
    }
    let replacement = replacement.clone();
    transact_settings_json_at(path, |current| {
        let current_snapshot = settings_snapshot(current.clone())?;
        if current_snapshot.revision != expected_revision {
            return Ok(SettingsTransaction::Read(SettingsReplaceResult::Conflict(
                current_snapshot,
            )));
        }

        *current = replacement;
        let snapshot = settings_snapshot(current.clone())?;
        Ok(SettingsTransaction::Write(SettingsReplaceResult::Replaced(
            snapshot,
        )))
    })
}

fn settings_snapshot(settings: serde_json::Value) -> Result<SettingsSnapshot, String> {
    let encoded = serde_json::to_vec(&settings).map_err(|error| error.to_string())?;
    let digest = Sha256::digest(encoded);
    let mut revision = String::with_capacity(digest.len() * 2);
    const HEX: &[u8; 16] = b"0123456789abcdef";
    for byte in digest {
        revision.push(HEX[(byte >> 4) as usize] as char);
        revision.push(HEX[(byte & 0x0f) as usize] as char);
    }
    Ok(SettingsSnapshot { settings, revision })
}

fn settings_lock_path(path: &Path) -> PathBuf {
    let mut lock_path = path.as_os_str().to_os_string();
    lock_path.push(".lock");
    PathBuf::from(lock_path)
}

fn write_settings_json_to_path(path: &Path, root: &serde_json::Value) -> Result<(), String> {
    let pretty = serde_json::to_string_pretty(root).map_err(|e| e.to_string())?;
    crate::file_replace::write_private(path, pretty).map_err(|e| e.to_string())
}

pub fn remove_workspace_from_settings(root: &mut serde_json::Value, path: &Path) -> bool {
    let Some(obj) = root.as_object_mut() else {
        return false;
    };

    let mut removed = false;
    if let Some(arr) = obj
        .get_mut("workspaces")
        .and_then(|value| value.as_array_mut())
    {
        let before_len = arr.len();
        arr.retain(|value| {
            value
                .as_str()
                .map(|candidate| !settings_path_matches(candidate, path))
                .unwrap_or(true)
        });
        removed |= arr.len() != before_len;
    }

    {
        let key = "working_dir";
        let should_remove = obj
            .get(key)
            .and_then(|value| value.as_str())
            .map(|candidate| settings_path_matches(candidate, path))
            .unwrap_or(false);
        if should_remove {
            obj.remove(key);
            removed = true;
        }
    }

    removed
}

fn settings_path_matches(candidate: &str, target: &Path) -> bool {
    paths_equal(&expand_home(candidate.trim()), target)
}

fn paths_equal(left: &Path, right: &Path) -> bool {
    left == right
        || std::fs::canonicalize(left)
            .ok()
            .zip(std::fs::canonicalize(right).ok())
            .map(|(left, right)| left == right)
            .unwrap_or(false)
}

impl Default for Config {
    fn default() -> Self {
        Self {
            tunnel_provider: TunnelProvider::default(),
            ngrok_auth_token: None,
            ngrok_domain: None,
            cloudflare_tunnel_token: None,
            cloudflare_hostname: None,
            toolchain_mode: ToolchainMode::System,
            portable_toolchain: false,
            default_workspace: builtin_workspaces_dir(),
            workspaces: vec![],
            preview_base_url: None,
            tmux_detach_others: true,
            default_agent: "claude".to_string(),
            enabled_agents: crate::resources::AGENTS
                .iter()
                .map(|a| a.id.clone())
                .collect(),
            integrations: AgentIntegrationsConfig::default(),
            proxy: HttpProxyConfig::default(),
            api_bridge: ApiBridgeConfig::default(),
            local_agent_api: LocalAgentApiConfig::default(),
            search_tool: SearchToolConfig::default(),
            remote: RemoteConfig::default(),
            raw_channels: serde_json::Value::Object(serde_json::Map::new()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn unique_test_dir(name: &str) -> PathBuf {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        std::env::temp_dir().join(format!(
            "vibearound-config-{name}-{}-{nonce}",
            std::process::id()
        ))
    }

    #[test]
    fn settings_write_replaces_file() {
        let dir = unique_test_dir("write");
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("settings.json");
        fs::write(&path, "{}").unwrap();

        write_settings_json_to_path(&path, &serde_json::json!({ "workspaces": [] })).unwrap();

        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&fs::read_to_string(&path).unwrap()).unwrap(),
            serde_json::json!({ "workspaces": [] })
        );

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(&path).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }

        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn settings_write_creates_parent_dir() {
        let dir = unique_test_dir("parent");
        let path = dir.join("nested").join("settings.json");

        write_settings_json_to_path(&path, &serde_json::json!({ "onboarded": true })).unwrap();

        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&fs::read_to_string(&path).unwrap()).unwrap(),
            serde_json::json!({ "onboarded": true })
        );
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn concurrent_settings_mutations_preserve_disjoint_fields() {
        use std::sync::{Arc, Barrier};

        let dir = unique_test_dir("concurrent-fields");
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("settings.json");
        fs::write(&path, "{}").unwrap();
        let start = Arc::new(Barrier::new(3));
        let handles = ["alpha", "beta"].map(|field| {
            let path = path.clone();
            let start = Arc::clone(&start);
            std::thread::spawn(move || {
                start.wait();
                mutate_settings_json_at(&path, |root| {
                    std::thread::sleep(std::time::Duration::from_millis(20));
                    root.as_object_mut()
                        .expect("validated settings object")
                        .insert(field.to_string(), serde_json::json!(true));
                    Ok(())
                })
            })
        });

        start.wait();
        for handle in handles {
            handle.join().unwrap().unwrap();
        }

        let settings: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(settings["alpha"], true);
        assert_eq!(settings["beta"], true);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn concurrent_settings_mutations_serialize_same_field() {
        use std::sync::{Arc, Barrier};

        let dir = unique_test_dir("concurrent-counter");
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("settings.json");
        fs::write(&path, r#"{ "count": 0 }"#).unwrap();
        let start = Arc::new(Barrier::new(5));
        let handles = (0..4)
            .map(|_| {
                let path = path.clone();
                let start = Arc::clone(&start);
                std::thread::spawn(move || {
                    start.wait();
                    mutate_settings_json_at(&path, |root| {
                        let count = root["count"].as_u64().unwrap();
                        std::thread::sleep(std::time::Duration::from_millis(10));
                        root["count"] = serde_json::json!(count + 1);
                        Ok(())
                    })
                })
            })
            .collect::<Vec<_>>();

        start.wait();
        for handle in handles {
            handle.join().unwrap().unwrap();
        }

        let settings: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(settings["count"], 4);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn settings_mutation_preserves_malformed_file() {
        let dir = unique_test_dir("malformed");
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("settings.json");
        let malformed = r#"{ "workspaces": ["#;
        fs::write(&path, malformed).unwrap();

        let error = mutate_settings_json_at(&path, |_| Ok(())).unwrap_err();

        assert!(error.contains("parse"));
        assert_eq!(fs::read_to_string(&path).unwrap(), malformed);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn settings_patch_changes_only_named_fields() {
        let dir = unique_test_dir("json-patch");
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("settings.json");
        fs::write(
            &path,
            r#"{ "api_bridge": { "enabled": true, "port": 9000, "retry_429": { "max_retries": 10 } }, "onboarded": true }"#,
        )
        .unwrap();

        let snapshot = patch_settings_json_at(
            &path,
            &serde_json::json!([
                { "op": "replace", "path": "/api_bridge/port", "value": 9001 },
                { "op": "replace", "path": "/api_bridge/retry_429/max_retries", "value": null },
                { "op": "remove", "path": "/onboarded" }
            ]),
        )
        .unwrap();

        assert_eq!(snapshot.settings["api_bridge"]["enabled"], true);
        assert_eq!(snapshot.settings["api_bridge"]["port"], 9001);
        assert!(snapshot.settings["api_bridge"]["retry_429"]["max_retries"].is_null());
        assert!(snapshot.settings.get("onboarded").is_none());
        assert_eq!(snapshot.revision.len(), 64);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn stale_settings_revision_cannot_replace_newer_document() {
        let dir = unique_test_dir("revision");
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("settings.json");
        fs::write(&path, r#"{ "value": 1 }"#).unwrap();
        let original = settings_snapshot(serde_json::json!({ "value": 1 })).unwrap();

        let first = replace_settings_json_if_revision_at(
            &path,
            &original.revision,
            &serde_json::json!({ "value": 2 }),
        )
        .unwrap();
        assert!(matches!(first, SettingsReplaceResult::Replaced(_)));

        let second = replace_settings_json_if_revision_at(
            &path,
            &original.revision,
            &serde_json::json!({ "value": 3 }),
        )
        .unwrap();
        let SettingsReplaceResult::Conflict(current) = second else {
            panic!("stale revision unexpectedly replaced settings");
        };

        assert_eq!(current.settings["value"], 2);
        let persisted: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(persisted["value"], 2);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn empty_enabled_agents_stays_empty() {
        let dir = unique_test_dir("enabled-agents");
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("settings.json");
        fs::write(&path, r#"{ "enabled_agents": [] }"#).unwrap();

        let config = load_settings_from(&path);

        assert!(config.enabled_agents.is_empty());
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn proxy_settings_are_trimmed() {
        let dir = unique_test_dir("proxy");
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("settings.json");
        fs::write(
            &path,
            r#"{ "proxy": { "http_proxy": " http://127.0.0.1:7890 ", "no_proxy": " localhost,127.0.0.1 " } }"#,
        )
        .unwrap();

        let config = load_settings_from(&path);

        assert_eq!(
            config.proxy.http_proxy.as_deref(),
            Some("http://127.0.0.1:7890")
        );
        assert!(config.proxy.enabled);
        assert!(config.proxy.is_configured());
        assert_eq!(
            config.proxy.no_proxy.as_deref(),
            Some("localhost,127.0.0.1")
        );
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn disabled_proxy_keeps_values_but_is_not_configured() {
        let dir = unique_test_dir("proxy-disabled");
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("settings.json");
        fs::write(
            &path,
            r#"{ "proxy": { "enabled": false, "http_proxy": "http://127.0.0.1:7890" } }"#,
        )
        .unwrap();

        let config = load_settings_from(&path);

        assert!(!config.proxy.enabled);
        assert_eq!(
            config.proxy.http_proxy.as_deref(),
            Some("http://127.0.0.1:7890")
        );
        assert!(!config.proxy.is_configured());
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn portable_toolchain_can_be_configured_independently() {
        let dir = unique_test_dir("portable-toolchain");
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("settings.json");
        fs::write(
            &path,
            r#"{ "startkit": { "toolchain_mode": "managed", "portable_toolchain": false } }"#,
        )
        .unwrap();

        let config = load_settings_from(&path);

        assert_eq!(config.toolchain_mode, ToolchainMode::Managed);
        assert!(!config.portable_toolchain);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn legacy_managed_toolchain_enables_portable_toolchain() {
        let dir = unique_test_dir("legacy-portable-toolchain");
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("settings.json");
        fs::write(&path, r#"{ "startkit": { "toolchain_mode": "managed" } }"#).unwrap();

        let config = load_settings_from(&path);

        assert_eq!(config.toolchain_mode, ToolchainMode::Managed);
        assert!(config.portable_toolchain);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn remote_channel_defaults_are_loaded() {
        let dir = unique_test_dir("remote-defaults");
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("settings.json");
        fs::write(
            &path,
            serde_json::json!({
                "remote": {
                    "channels": {
                        "feishu": {
                            "agentId": "codex",
                            "profileId": "direct",
                            "workspace": "/ignored"
                        },
                        "slack": {
                            "agent": "does-not-exist"
                        }
                    }
                }
            })
            .to_string(),
        )
        .unwrap();

        let config = load_settings_from(&path);

        let feishu = config.remote_channel_defaults("feishu");
        assert_eq!(feishu.agent_id.as_deref(), Some("codex"));
        assert_eq!(feishu.profile_id.as_deref(), Some("direct"));
        let slack = config.remote_channel_defaults("slack");
        assert_eq!(slack.agent_id, None);
        assert_eq!(
            config.remote_channel_defaults("unknown"),
            RemoteChannelDefaults::default()
        );
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn integration_auto_install_defaults_to_enabled() {
        let dir = unique_test_dir("integrations-default");
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("settings.json");
        fs::write(&path, "{}").unwrap();

        let config = load_settings_from(&path);

        assert!(config.integrations.mcp_auto_install);
        assert!(config.integrations.skill_auto_install);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn integration_auto_install_can_be_disabled() {
        let dir = unique_test_dir("integrations-disabled");
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("settings.json");
        fs::write(
            &path,
            r#"{ "integrations": { "mcp_auto_install": false, "skill_auto_install": false } }"#,
        )
        .unwrap();

        let config = load_settings_from(&path);

        assert!(!config.integrations.mcp_auto_install);
        assert!(!config.integrations.skill_auto_install);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn api_bridge_retry_429_defaults_to_enabled() {
        let dir = unique_test_dir("api-bridge-retry-default");
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("settings.json");
        fs::write(&path, "{}").unwrap();

        let config = load_settings_from(&path);

        assert!(config.api_bridge.retry_429.enabled);
        assert_eq!(config.api_bridge.retry_429.max_retries, Some(10));
        assert_eq!(config.api_bridge.retry_429.delay_seconds, 10);
        assert!(!config.api_bridge.replace_provider_web_search);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn local_agent_api_defaults_to_disabled() {
        let dir = unique_test_dir("local-agent-api-default");
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("settings.json");
        fs::write(&path, "{}").unwrap();

        let config = load_settings_from(&path);

        assert!(!config.local_agent_api.enabled);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn local_agent_api_can_be_enabled() {
        let dir = unique_test_dir("local-agent-api-enabled");
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("settings.json");
        fs::write(&path, r#"{ "local_agent_api": { "enabled": true } }"#).unwrap();

        let config = load_settings_from(&path);

        assert!(config.local_agent_api.enabled);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn api_bridge_retry_429_can_be_configured() {
        let dir = unique_test_dir("api-bridge-retry-configured");
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("settings.json");
        fs::write(
            &path,
            r#"{ "api_bridge": { "replaceProviderWebSearch": true, "retry_429": { "enabled": false, "max_retries": 4, "delay_seconds": 12 } } }"#,
        )
        .unwrap();

        let config = load_settings_from(&path);

        assert!(config.api_bridge.replace_provider_web_search);
        assert!(!config.api_bridge.retry_429.enabled);
        assert_eq!(config.api_bridge.retry_429.max_retries, Some(4));
        assert_eq!(config.api_bridge.retry_429.delay_seconds, 12);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn api_bridge_retry_429_null_retries_means_unlimited() {
        let dir = unique_test_dir("api-bridge-retry-unlimited");
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("settings.json");
        fs::write(
            &path,
            r#"{ "api_bridge": { "retry_429": { "max_retries": null, "delay_seconds": 3 } } }"#,
        )
        .unwrap();

        let config = load_settings_from(&path);

        assert_eq!(config.api_bridge.retry_429.max_retries, None);
        assert_eq!(config.api_bridge.retry_429.delay_seconds, 3);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn search_tool_defaults_to_disabled() {
        let dir = unique_test_dir("search-tool-default");
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("settings.json");
        fs::write(&path, "{}").unwrap();

        let config = load_settings_from(&path);

        assert!(config.search_tool.stdio_path.is_none());
        assert_eq!(config.search_tool.max_results, None);
        assert_eq!(config.search_tool.search_context_size, None);
        assert!(config.search_tool.sources.is_empty());
        assert!(!config.search_tool.has_enabled_sources());
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn search_tool_enabled_source_names_use_preferred_order() {
        let config = SearchToolConfig {
            stdio_path: None,
            max_results: None,
            search_context_size: None,
            sources: BTreeMap::from([
                (
                    "grok".to_string(),
                    SearchSourceConfig {
                        enabled: true,
                        ..SearchSourceConfig::default()
                    },
                ),
                (
                    "brave".to_string(),
                    SearchSourceConfig {
                        enabled: true,
                        ..SearchSourceConfig::default()
                    },
                ),
                (
                    "exa".to_string(),
                    SearchSourceConfig {
                        enabled: true,
                        ..SearchSourceConfig::default()
                    },
                ),
                (
                    "tavily".to_string(),
                    SearchSourceConfig {
                        enabled: true,
                        ..SearchSourceConfig::default()
                    },
                ),
            ]),
        };

        assert_eq!(
            config.enabled_source_names(),
            vec![
                "exa".to_string(),
                "tavily".to_string(),
                "grok".to_string(),
                "brave".to_string(),
            ]
        );
    }

    #[test]
    fn search_tool_settings_can_be_configured() {
        let dir = unique_test_dir("search-tool-configured");
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("settings.json");
        fs::write(
            &path,
            r#"
            {
              "searchTool": {
                "stdioPath": "~/bin/va-search-tool",
                "maxResults": 8,
                "searchContextSize": " high ",
                "sources": {
                  " Exa ": {
                    "apiKey": " exa-key ",
                    "baseUrl": " https://api.exa.ai "
                  },
                  "tavily": {
                    "enabled": false,
                    "api_key_env": " TAVILY_API_KEY "
                  },
                  "grok": {
                    "key": "xai-key"
                  }
                }
              }
            }
            "#,
        )
        .unwrap();

        let config = load_settings_from(&path);

        assert_eq!(
            config.search_tool.stdio_path.as_deref(),
            Some(home_dir().join("bin/va-search-tool").as_path())
        );
        assert!(config.search_tool.has_enabled_sources());
        assert_eq!(config.search_tool.max_results, Some(8));
        assert_eq!(
            config.search_tool.search_context_size.as_deref(),
            Some("high")
        );
        let exa = config.search_tool.sources.get("exa").expect("exa source");
        assert!(exa.enabled);
        assert_eq!(exa.api_key.as_deref(), Some("exa-key"));
        assert_eq!(exa.base_url.as_deref(), Some("https://api.exa.ai"));
        let tavily = config
            .search_tool
            .sources
            .get("tavily")
            .expect("tavily source");
        assert!(!tavily.enabled);
        assert_eq!(tavily.api_key_env.as_deref(), Some("TAVILY_API_KEY"));
        let grok = config.search_tool.sources.get("grok").expect("grok source");
        assert!(grok.enabled);
        assert_eq!(grok.api_key.as_deref(), Some("xai-key"));
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn default_workspace_setting_is_used_as_default() {
        let dir = unique_test_dir("default-workspace");
        fs::create_dir_all(&dir).unwrap();
        let default_workspace = dir.join("custom-default");
        let path = dir.join("settings.json");
        fs::write(
            &path,
            serde_json::json!({
                "default_workspace": default_workspace.to_string_lossy().to_string()
            })
            .to_string(),
        )
        .unwrap();

        let config = load_settings_from(&path);

        assert_eq!(config.resolve_workspace("codex"), default_workspace);
        assert_eq!(config.default_workspace, default_workspace);
        assert!(config.all_workspaces().contains(&default_workspace));
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn remove_workspace_cleans_workspaces_and_legacy_working_dir() {
        let dir = unique_test_dir("remove-workspace");
        fs::create_dir_all(&dir).unwrap();
        let workspace = dir.join("project-a");
        let other = dir.join("project-b");
        let mut root = serde_json::json!({
            "workspaces": [
                workspace.to_string_lossy().to_string(),
                other.to_string_lossy().to_string()
            ],
            "default_workspace": workspace.to_string_lossy().to_string(),
            "working_dir": workspace.to_string_lossy().to_string()
        });

        assert!(remove_workspace_from_settings(&mut root, &workspace));

        let workspaces = root
            .get("workspaces")
            .and_then(|value| value.as_array())
            .unwrap();
        assert_eq!(workspaces.len(), 1);
        assert_eq!(
            workspaces[0].as_str(),
            Some(other.to_string_lossy().as_ref())
        );
        assert_eq!(
            root.get("default_workspace")
                .and_then(|value| value.as_str()),
            Some(workspace.to_string_lossy().as_ref())
        );
        assert!(root.get("working_dir").is_none());
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn expand_home_handles_bare_home() {
        assert_eq!(expand_home("~"), home_dir());
        assert_eq!(expand_home("~/project"), home_dir().join("project"));
    }
}
