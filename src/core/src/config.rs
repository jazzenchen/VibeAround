//! Config loading helpers.
//! All config comes from ~/.vibearound/settings.json.
//! Callers load a fresh Config when they need one.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Once};
use std::time::{SystemTime, UNIX_EPOCH};

use parking_lot::RwLock;

use crate::tunnels::TunnelProvider;

/// Global config cache. Populated on first `ensure_loaded()` call, reloaded
/// by `reload()` or automatically after `update_settings_json()`.
static CONFIG_CACHE: RwLock<Option<Arc<Config>>> = RwLock::new(None);

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
    home_dir().join(".vibearound")
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
    let settings_path = dir.join("settings.json");
    if !settings_path.exists() {
        tracing::info!(
            "[VibeAround] Creating default settings.json at {:?}",
            settings_path
        );
        if let Err(e) = std::fs::write(&settings_path, DEFAULT_SETTINGS_JSON) {
            tracing::info!("[VibeAround] Failed to write settings.json: {}", e);
        } else if let Err(e) = crate::auth::set_owner_only(&settings_path) {
            tracing::info!("[VibeAround] Failed to chmod settings.json: {}", e);
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
    // --- Workspaces ---
    /// User-added project folders (not including the built-in ~/.vibearound/workspaces/).
    pub workspaces: Vec<PathBuf>,
    pub preview_base_url: Option<String>,
    pub tmux_detach_others: bool,
    // --- Agents ---
    pub default_agent: String,
    /// Per-agent default profile id used when a route has not chosen a
    /// profile explicitly. Keys are canonical agent ids.
    pub default_profiles: BTreeMap<String, String>,
    /// Subset of agent IDs from `resources/agents.json` the user has enabled.
    /// Validated at load time — entries that don't resolve via
    /// `resources::agent_by_alias` are dropped.
    pub enabled_agents: Vec<String>,
    // --- Optional outbound HTTP proxy ---
    pub proxy: HttpProxyConfig,
    // --- Raw channels JSON (for dynamic plugin config) ---
    raw_channels: serde_json::Value,
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
    /// The default workspace is fixed to ~/.vibearound/workspaces.
    pub fn resolve_workspace(&self, _agent_kind: &str) -> PathBuf {
        builtin_workspaces_dir()
    }

    /// Resolve the default profile id for an agent alias/id.
    pub fn default_profile_for(&self, agent_kind: &str) -> Option<String> {
        let agent_id = crate::resources::agent_by_alias(agent_kind)
            .map(|def| def.id.as_str())
            .unwrap_or(agent_kind);
        self.default_profiles
            .get(agent_id)
            .cloned()
            .filter(|s| !s.trim().is_empty())
    }

    /// All available workspaces: the built-in root + user-added paths.
    pub fn all_workspaces(&self) -> Vec<PathBuf> {
        let mut all = vec![builtin_workspaces_dir()];
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
    let path = data_dir().join("settings.json");
    let cfg = Arc::new(load_settings_from(&path));
    *CONFIG_CACHE.write() = Some(Arc::clone(&cfg));
    cfg
}

/// Force re-read config from disk and update the cache.
/// Called after `update_settings_json()` and on daemon restart.
pub fn reload() -> Arc<Config> {
    ensure_rustls_provider();
    init_data_dir();
    let path = data_dir().join("settings.json");
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

    let raw_channels = root
        .get("channels")
        .cloned()
        .unwrap_or(serde_json::Value::Object(serde_json::Map::new()));

    // --- Workspaces (new format) with backward compat for old working_dir ---
    let mut workspaces: Vec<PathBuf> = root
        .get("workspaces")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str())
                .map(|s| expand_home(s.trim()))
                .filter(|p| !p.as_os_str().is_empty())
                .collect()
        })
        .unwrap_or_default();

    // Backward compat: keep old workspace-like fields discoverable as regular
    // workspaces, but the default workspace itself is fixed to the built-in root.
    let mut add_workspace = |candidate: PathBuf| {
        if !workspaces.contains(&candidate) {
            workspaces.push(candidate);
        }
    };

    if let Some(legacy_default) = root
        .get("default_workspace")
        .and_then(|v| v.as_str())
        .map(|s| expand_home(s.trim()))
        .filter(|p| !p.as_os_str().is_empty())
    {
        add_workspace(legacy_default);
    }

    if root.get("workspaces").is_none() {
        if let Some(legacy) = root
            .get("working_dir")
            .and_then(|v| v.as_str())
            .map(|s| expand_home(s.trim()))
            .filter(|p| !p.as_os_str().is_empty())
        {
            add_workspace(legacy);
        }
    }

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

    let default_profiles = root
        .get("default_profiles")
        .and_then(|v| v.as_object())
        .map(|obj| {
            obj.iter()
                .filter_map(|(agent, profile)| {
                    let profile = profile.as_str()?.trim();
                    if profile.is_empty() {
                        return None;
                    }
                    let agent_id = crate::resources::agent_by_alias(agent)
                        .map(|def| def.id.clone())
                        .unwrap_or_else(|| agent.to_string());
                    Some((agent_id, profile.to_string()))
                })
                .collect::<BTreeMap<_, _>>()
        })
        .unwrap_or_default();

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
        workspaces,
        preview_base_url,
        tmux_detach_others,
        default_agent,
        default_profiles,
        enabled_agents,
        proxy,
        raw_channels,
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

/// Read + write settings.json (for API-driven updates).
/// Automatically reloads the in-memory config cache after writing.
pub fn update_settings_json(mutator: impl FnOnce(&mut serde_json::Value)) -> Result<(), String> {
    let path = data_dir().join("settings.json");
    let data = std::fs::read_to_string(&path).unwrap_or_else(|_| "{}".to_string());
    let mut root: serde_json::Value = serde_json::from_str(&data).unwrap_or(serde_json::json!({}));
    mutator(&mut root);
    write_settings_json_locked(&root)?;
    // Invalidate cache so next ensure_loaded() picks up the change.
    *CONFIG_CACHE.write() = None;
    Ok(())
}

/// Replace settings.json with an already-mutated JSON value. Use this for
/// whole-file settings flows such as onboarding. Incremental updates should
/// prefer [`update_settings_json`] so they merge against the latest on-disk
/// content.
pub fn write_settings_json(root: &serde_json::Value) -> Result<(), String> {
    write_settings_json_locked(root)?;
    *CONFIG_CACHE.write() = None;
    Ok(())
}

fn write_settings_json_locked(root: &serde_json::Value) -> Result<(), String> {
    let path = data_dir().join("settings.json");
    write_settings_json_to_path(&path, root)
}

fn write_settings_json_to_path(path: &Path, root: &serde_json::Value) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let pretty = serde_json::to_string_pretty(root).map_err(|e| e.to_string())?;
    fs::write(path, pretty).map_err(|e| e.to_string())?;
    crate::auth::set_owner_only(path).map_err(|e| e.to_string())
}

impl Default for Config {
    fn default() -> Self {
        Self {
            tunnel_provider: TunnelProvider::default(),
            ngrok_auth_token: None,
            ngrok_domain: None,
            cloudflare_tunnel_token: None,
            cloudflare_hostname: None,
            workspaces: vec![],
            preview_base_url: None,
            tmux_detach_others: true,
            default_agent: "claude".to_string(),
            default_profiles: BTreeMap::new(),
            enabled_agents: crate::resources::AGENTS
                .iter()
                .map(|a| a.id.clone())
                .collect(),
            proxy: HttpProxyConfig::default(),
            raw_channels: serde_json::Value::Object(serde_json::Map::new()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn default_workspace_setting_is_not_used_as_default() {
        let dir = unique_test_dir("fixed-workspace");
        fs::create_dir_all(&dir).unwrap();
        let legacy_workspace = dir.join("legacy-default");
        let path = dir.join("settings.json");
        fs::write(
            &path,
            serde_json::json!({
                "default_workspace": legacy_workspace.to_string_lossy().to_string()
            })
            .to_string(),
        )
        .unwrap();

        let config = load_settings_from(&path);

        assert_eq!(config.resolve_workspace("codex"), builtin_workspaces_dir());
        assert!(config.workspaces.contains(&legacy_workspace));
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn expand_home_handles_bare_home() {
        assert_eq!(expand_home("~"), home_dir());
        assert_eq!(expand_home("~/project"), home_dir().join("project"));
    }
}
