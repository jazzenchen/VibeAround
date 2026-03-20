//! Global config singleton. Load settings.json once; desktop and server both call
//! `ensure_loaded()` so the first caller does the work, later callers get the same instance.
//! All config comes from ~/.vibearound/settings.json.

use std::path::PathBuf;
use std::sync::Once;
use std::sync::OnceLock;

use crate::tunnels::TunnelProvider;

/// Default server port for both standalone server and desktop-spawned server.
pub const DEFAULT_PORT: u16 = 12358;

/// Minimal default settings.json content, embedded at compile time.
const DEFAULT_SETTINGS_JSON: &str = r#"{
  "working_dir": ""
}"#;

/// Data directory: ~/.vibearound
pub fn data_dir() -> PathBuf {
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .unwrap_or_else(|_| "/tmp".into());
    PathBuf::from(home).join(".vibearound")
}

/// Ensure ~/.vibearound/ exists with settings.json and workspaces/.
fn init_data_dir() {
    let dir = data_dir();
    if let Err(e) = std::fs::create_dir_all(&dir) {
        eprintln!("[VibeAround] Failed to create data dir {:?}: {}", dir, e);
        return;
    }
    let settings_path = dir.join("settings.json");
    if !settings_path.exists() {
        eprintln!("[VibeAround] Creating default settings.json at {:?}", settings_path);
        if let Err(e) = std::fs::write(&settings_path, DEFAULT_SETTINGS_JSON) {
            eprintln!("[VibeAround] Failed to write settings.json: {}", e);
        }
    }
    let ws_dir = dir.join("workspaces");
    if let Err(e) = std::fs::create_dir_all(&ws_dir) {
        eprintln!("[VibeAround] Failed to create workspaces dir: {}", e);
    }
    crate::agent::manager_prompt::ensure_manager_prompt_dir();
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

static CONFIG: OnceLock<Config> = OnceLock::new();

/// Per-channel verbose/output settings for IM.
#[derive(Debug, Clone)]
pub struct ImVerboseConfig {
    pub show_thinking: bool,
    pub show_tool_use: bool,
}

impl Default for ImVerboseConfig {
    fn default() -> Self {
        Self { show_thinking: false, show_tool_use: false }
    }
}

/// Cached config from settings.json.
pub struct Config {
    // --- Tunnel ---
    pub tunnel_provider: TunnelProvider,
    pub ngrok_auth_token: Option<String>,
    pub ngrok_domain: Option<String>,
    pub cloudflare_tunnel_token: Option<String>,
    pub cloudflare_hostname: Option<String>,
    // --- Working dir ---
    pub working_dir: PathBuf,
    pub preview_base_url: Option<String>,
    pub tmux_detach_others: bool,
    // --- Agents ---
    pub default_agent: String,
    pub enabled_agents: Vec<crate::agent::AgentKind>,
    // --- Raw channels JSON (for dynamic plugin config) ---
    raw_channels: serde_json::Value,
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

    /// Get verbose config for a specific channel.
    pub fn channel_verbose(&self, name: &str) -> ImVerboseConfig {
        parse_verbose_config(self.raw_channels.get(name))
    }
}

/// Ensure config is loaded (idempotent).
pub fn ensure_loaded() -> &'static Config {
    ensure_rustls_provider();
    CONFIG.get_or_init(|| {
        init_data_dir();
        let path = data_dir().join("settings.json");
        load_settings_from(&path)
    })
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

    let working_dir = root
        .get("working_dir")
        .and_then(|v| v.as_str())
        .map(|s| PathBuf::from(s.trim()))
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(default_working_dir);

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
                .filter_map(crate::agent::AgentKind::from_str_loose)
                .collect::<Vec<_>>()
        })
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| crate::agent::AgentKind::all().to_vec());

    Config {
        tunnel_provider,
        ngrok_auth_token,
        ngrok_domain,
        cloudflare_tunnel_token,
        cloudflare_hostname,
        working_dir,
        preview_base_url,
        tmux_detach_others,
        default_agent,
        enabled_agents,
        raw_channels,
    }
}

/// Base URL for preview links.
pub fn preview_base_url() -> Option<String> {
    let cfg = ensure_loaded();
    cfg.preview_base_url
        .as_ref()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .or_else(|| cfg.cloudflare_hostname.as_ref().map(|h| format!("https://{}", h.trim())))
        .or_else(|| cfg.ngrok_domain.as_ref().map(|d| format!("https://{}", d.trim())))
}

/// Parse verbose config from a channel JSON object.
fn parse_verbose_config(channel_obj: Option<&serde_json::Value>) -> ImVerboseConfig {
    let verbose = channel_obj.and_then(|c| c.get("verbose"));
    ImVerboseConfig {
        show_thinking: verbose
            .and_then(|v| v.get("show_thinking"))
            .and_then(|v| v.as_bool())
            .unwrap_or(false),
        show_tool_use: verbose
            .and_then(|v| v.get("show_tool_use"))
            .and_then(|v| v.as_bool())
            .unwrap_or(false),
    }
}

fn default_working_dir() -> PathBuf {
    data_dir()
}

impl Default for Config {
    fn default() -> Self {
        Self {
            tunnel_provider: TunnelProvider::default(),
            ngrok_auth_token: None,
            ngrok_domain: None,
            cloudflare_tunnel_token: None,
            cloudflare_hostname: None,
            working_dir: default_working_dir(),
            preview_base_url: None,
            tmux_detach_others: true,
            default_agent: "claude".to_string(),
            enabled_agents: crate::agent::AgentKind::all().to_vec(),
            raw_channels: serde_json::Value::Object(serde_json::Map::new()),
        }
    }
}
