//! Global config singleton. Load settings.json once; desktop and server both call
//! `ensure_loaded()` so the first caller does the work, later callers get the same instance.
//! All config (tunnel, ngrok, telegram, feishu) comes from settings.json.

use std::path::PathBuf;
use std::sync::Once;
use std::sync::OnceLock;

use crate::tunnels::TunnelProvider;

/// Root directory for config: settings.json lives here (workspace src/ when common is in src/core).
fn config_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..")
}

/// Install rustls default crypto provider once (required by rustls 0.22+ before any TLS use, e.g. ngrok SDK).
fn ensure_rustls_provider() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        rustls::crypto::aws_lc_rs::default_provider()
            .install_default()
            .expect("rustls default crypto provider");
    });
}

static CONFIG: OnceLock<Config> = OnceLock::new();

/// Cached config from settings.json.
pub struct Config {
    pub tunnel_provider: TunnelProvider,
    pub ngrok_auth_token: Option<String>,
    /// Reserved/static domain (e.g. myapp.ngrok.io). If set, tunnel uses this instead of a random URL.
    pub ngrok_domain: Option<String>,
    pub telegram_bot_token: Option<String>,
    pub feishu_app_id: Option<String>,
    pub feishu_app_secret: Option<String>,
    /// Root for job workspaces and projects.json. Default: ~/test.
    pub working_dir: PathBuf,
    /// Optional base URL for preview links (e.g. https://xxx.ngrok-free.app). Overrides ngrok_domain when set.
    pub preview_base_url: Option<String>,
    /// When attaching to a tmux session, detach other clients first (`tmux attach -d`). Default: true.
    pub tmux_detach_others: bool,
}

/// Ensure config is loaded (idempotent). Loads settings.json on first call; returns the same instance afterwards.
pub fn ensure_loaded() -> &'static Config {
    ensure_rustls_provider();
    CONFIG.get_or_init(|| {
        let path = config_root().join("settings.json");
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

    let channels = root.get("channels");
    let telegram_bot_token = channels
        .and_then(|c| c.get("telegram"))
        .and_then(|t| t.get("bot_token"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .filter(|s| !s.is_empty());

    let feishu_app_id = channels
        .and_then(|c| c.get("feishu"))
        .and_then(|f| f.get("app_id"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .filter(|s| !s.is_empty());

    let feishu_app_secret = channels
        .and_then(|c| c.get("feishu"))
        .and_then(|f| f.get("app_secret"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .filter(|s| !s.is_empty());

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

    Config {
        tunnel_provider,
        ngrok_auth_token,
        ngrok_domain,
        telegram_bot_token,
        feishu_app_id,
        feishu_app_secret,
        working_dir,
        preview_base_url,
        tmux_detach_others,
    }
}

/// Base URL for preview links (e.g. https://xxx.ngrok-free.app). Uses preview_base_url from settings if set, else ngrok_domain.
pub fn preview_base_url() -> Option<String> {
    let cfg = ensure_loaded();
    cfg.preview_base_url
        .as_ref()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .or_else(|| cfg.ngrok_domain.as_ref().map(|d| format!("https://{}", d.trim())))
}

/// Default working directory for job workspaces: ~/test.
fn default_working_dir() -> PathBuf {
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .unwrap_or_else(|_| "/tmp".into());
    PathBuf::from(home).join("test")
}

impl Default for Config {
    fn default() -> Self {
        Self {
            tunnel_provider: TunnelProvider::default(),
            ngrok_auth_token: None,
            ngrok_domain: None,
            telegram_bot_token: None,
            feishu_app_id: None,
            feishu_app_secret: None,
            working_dir: default_working_dir(),
            preview_base_url: None,
            tmux_detach_others: true,
        }
    }
}
