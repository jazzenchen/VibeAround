//! Plugin discovery and manifest schema.
//!
//! Plugins are disk-resident directories under either
//! - `~/.vibearound/plugins/<plugin-slug>/` (user-installed), or
//! - `<repo>/plugins/<plugin-slug>/` (project, dev-only),
//!
//! each containing a `plugin.json` manifest describing the plugin.
//! Channel plugins (`kind == "channel"`) cover IM integrations like Telegram /
//! Feishu / etc. Future plugin kinds can add another `plugins/<kind>.rs`
//! sibling without changing the discovery infrastructure here.

pub mod channel;

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::config;

pub(crate) const PLUGIN_MANIFEST_NAME: &str = "plugin.json";
pub(crate) const PROJECT_PLUGINS_DIR: &str = "plugins";

// ---------------------------------------------------------------------------
// Manifest schema
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct PluginAuthCapabilities {
    #[serde(default)]
    pub methods: Vec<String>,
}

impl PluginAuthCapabilities {
    pub fn supports_qrcode_login(&self) -> bool {
        self.methods.iter().any(|method| method == "qrcode_login")
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct PluginCapabilities {
    #[serde(default, rename = "interactiveCards")]
    pub interactive_cards: bool,
    #[serde(default)]
    pub streaming: bool,
    #[serde(default)]
    pub reactions: bool,
    #[serde(default, rename = "editMessage")]
    pub edit_message: bool,
    #[serde(default)]
    pub media: bool,
    pub auth: Option<PluginAuthCapabilities>,
}

impl PluginCapabilities {
    pub fn supports_qrcode_login(&self) -> bool {
        self.auth
            .as_ref()
            .map(PluginAuthCapabilities::supports_qrcode_login)
            .unwrap_or(false)
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct PluginManifest {
    pub id: String,
    pub name: String,
    pub version: String,
    #[serde(default, alias = "type")]
    pub kind: String,
    #[serde(default)]
    pub runtime: String,
    #[serde(default)]
    pub entry: String,
    pub build: Option<String>,
    #[serde(rename = "minHostVersion")]
    pub min_host_version: Option<String>,
    #[serde(rename = "configSchema")]
    pub config_schema: Option<serde_json::Value>,
    #[serde(default)]
    pub capabilities: PluginCapabilities,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum PluginSource {
    User,
    Project,
}

#[derive(Debug, Clone)]
pub struct DiscoveredPlugin {
    pub manifest: PluginManifest,
    pub dir: PathBuf,
    pub source: PluginSource,
}

impl DiscoveredPlugin {
    pub fn entry_path(&self) -> PathBuf {
        self.dir.join(&self.manifest.entry)
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiscoveredPluginSummary {
    pub id: String,
    pub name: String,
    pub version: String,
    pub kind: String,
    pub runtime: String,
    pub entry: String,
    pub source: PluginSource,
    /// Directory name on disk (may differ from `id` in plugin.json).
    pub dir_name: String,
    pub supports_qrcode_login: bool,
    pub config_schema: Option<serde_json::Value>,
    pub capabilities: PluginCapabilities,
}

impl From<&DiscoveredPlugin> for DiscoveredPluginSummary {
    fn from(plugin: &DiscoveredPlugin) -> Self {
        let dir_name = plugin
            .dir
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("")
            .to_string();
        Self {
            id: plugin.manifest.id.clone(),
            name: plugin.manifest.name.clone(),
            version: plugin.manifest.version.clone(),
            kind: plugin.manifest.kind.clone(),
            runtime: plugin.manifest.runtime.clone(),
            entry: plugin.manifest.entry.clone(),
            source: plugin.source.clone(),
            dir_name,
            supports_qrcode_login: plugin.manifest.capabilities.supports_qrcode_login(),
            config_schema: plugin.manifest.config_schema.clone(),
            capabilities: plugin.manifest.capabilities.clone(),
        }
    }
}

// ---------------------------------------------------------------------------
// Disk discovery (all kinds)
// ---------------------------------------------------------------------------

/// Discover every plugin manifest in the user and project plugin
/// directories, regardless of `kind`. Kind-specific callers
/// (e.g. [`channel::discover`]) filter this map to the plugins they care
/// about.
pub fn discover_plugins() -> HashMap<String, DiscoveredPlugin> {
    let mut discovered = HashMap::new();

    if let Some(project_dir) = project_plugins_dir() {
        load_plugins_from_dir(&project_dir, PluginSource::Project, &mut discovered);
    }
    load_plugins_from_dir(&user_plugins_dir(), PluginSource::User, &mut discovered);

    discovered
}

/// Discover plugins installed in the per-user VibeAround data directory only.
///
/// Onboarding install checks use this path so development/project plugins do
/// not make a fresh user install look complete before `~/.vibearound/plugins`
/// has the requested plugin.
pub fn discover_user_plugins() -> HashMap<String, DiscoveredPlugin> {
    let mut discovered = HashMap::new();
    load_plugins_from_dir(&user_plugins_dir(), PluginSource::User, &mut discovered);
    discovered
}

/// Look up any plugin kind by manifest id.
pub fn find(plugin_id: &str) -> Option<DiscoveredPlugin> {
    discover_plugins().remove(plugin_id)
}

/// Look up any plugin kind by manifest id in the per-user plugin directory.
pub fn find_user(plugin_id: &str) -> Option<DiscoveredPlugin> {
    discover_user_plugins().remove(plugin_id)
}

pub fn user_plugins_dir() -> PathBuf {
    config::data_dir().join(PROJECT_PLUGINS_DIR)
}

/// Return the in-tree plugins directory used during development.
///
/// Only meaningful in debug builds: the path is derived from
/// `CARGO_MANIFEST_DIR`, which is the *build machine's* absolute source
/// path. Baking that into a release binary would both leak local paths
/// into the shipped artifact and point at a directory that doesn't
/// exist on end-user machines. Release builds return `None` and rely
/// exclusively on `user_plugins_dir()`.
pub fn project_plugins_dir() -> Option<PathBuf> {
    #[cfg(debug_assertions)]
    {
        Some(
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .parent()
                .unwrap_or(Path::new("."))
                .join(PROJECT_PLUGINS_DIR),
        )
    }
    #[cfg(not(debug_assertions))]
    {
        None
    }
}

fn load_plugins_from_dir(
    base_dir: &Path,
    source: PluginSource,
    discovered: &mut HashMap<String, DiscoveredPlugin>,
) {
    let Ok(entries) = std::fs::read_dir(base_dir) else {
        return;
    };

    for entry in entries.flatten() {
        let plugin_dir = entry.path();
        if !plugin_dir.is_dir() {
            continue;
        }

        let manifest_path = plugin_dir.join(PLUGIN_MANIFEST_NAME);
        let manifest = match read_plugin_manifest(&manifest_path) {
            Some(manifest) => manifest,
            None => continue,
        };

        let plugin_id = manifest.id.trim().to_string();
        if plugin_id.is_empty() {
            tracing::info!(
                "[plugins] skipping plugin with empty id: {}",
                manifest_path.display()
            );
            continue;
        }

        if manifest.kind.trim().is_empty() {
            tracing::info!(
                "[plugins] skipping plugin '{}' with empty kind: {}",
                plugin_id,
                manifest_path.display()
            );
            continue;
        }

        let discovered_plugin = DiscoveredPlugin {
            manifest,
            dir: plugin_dir.clone(),
            source: source.clone(),
        };

        if let Some(previous) = discovered.insert(plugin_id.clone(), discovered_plugin) {
            tracing::info!(
                "[plugins] plugin '{}' from {} overrides {}",
                plugin_id,
                plugin_dir.display(),
                previous.dir.display()
            );
        }
    }
}

fn read_plugin_manifest(path: &Path) -> Option<PluginManifest> {
    let raw = std::fs::read_to_string(path).ok()?;
    match serde_json::from_str::<PluginManifest>(&raw) {
        Ok(manifest) => Some(manifest),
        Err(error) => {
            tracing::info!(
                "[plugins] failed to parse manifest {}: {}",
                path.display(),
                error
            );
            None
        }
    }
}
