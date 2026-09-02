//! Plugin discovery and manifest schema.
//!
//! Plugins are disk-resident directories under either
//! - `~/.vibearound/plugins/<plugin-slug>/` (user-installed), or
//! - `<repo>/plugins/<plugin-slug>/` (project, dev-only),
//! - or, in debug builds, sibling development checkouts next to this repo,
//!
//! each containing a `plugin.json` manifest describing the plugin.
//! Channel plugins (`kind == "channel"`) cover IM integrations like Telegram /
//! Feishu / etc. Future plugin kinds can add another `plugins/<kind>.rs`
//! sibling without changing the discovery infrastructure here.

pub mod channel;

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

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
    #[serde(default, rename = "topicScope")]
    pub topic_scope: TopicConversationScope,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum TopicConversationScope {
    Chat,
    #[default]
    Topic,
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

    pub fn installed_version(&self) -> String {
        read_package_version(&self.dir.join("package.json"))
            .unwrap_or_else(|| self.manifest.version.clone())
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
            version: plugin.installed_version(),
            kind: plugin.manifest.kind.clone(),
            runtime: plugin.manifest.runtime.clone(),
            entry: plugin.manifest.entry.clone(),
            source: plugin.source.clone(),
            dir_name,
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
    load_dev_checkout_plugins(&mut discovered);
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

/// Directory for app-private dependencies owned by a plugin-like feature.
///
/// These directories intentionally do not contain `plugin.json`, so normal
/// plugin discovery ignores them.
pub fn user_plugin_dependency_dir(id: &str) -> PathBuf {
    user_plugins_dir().join(id)
}

pub fn user_plugin_dependency_bin_path(id: &str, program: &str) -> PathBuf {
    let binary = if cfg!(windows) && !program.ends_with(".exe") {
        format!("{program}.exe")
    } else {
        program.to_string()
    };
    user_plugin_dependency_dir(id).join("bin").join(binary)
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

        load_plugin_from_dir(&plugin_dir, source.clone(), discovered);
    }
}

fn load_dev_checkout_plugins(discovered: &mut HashMap<String, DiscoveredPlugin>) {
    #[cfg(debug_assertions)]
    {
        let Some(parent_dir) = dev_checkout_parent_dir() else {
            return;
        };
        for plugin in crate::resources::PLUGINS.iter() {
            let plugin_dir = parent_dir.join(plugin.install_dir_name());
            load_plugin_from_dir(&plugin_dir, PluginSource::Project, discovered);
        }
    }
    #[cfg(not(debug_assertions))]
    {
        let _ = discovered;
    }
}

#[cfg(debug_assertions)]
fn dev_checkout_parent_dir() -> Option<PathBuf> {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .and_then(Path::parent)
        .map(Path::to_path_buf)
}

fn load_plugin_from_dir(
    plugin_dir: &Path,
    source: PluginSource,
    discovered: &mut HashMap<String, DiscoveredPlugin>,
) {
    let manifest_path = plugin_dir.join(PLUGIN_MANIFEST_NAME);
    let manifest = match read_plugin_manifest(&manifest_path) {
        Some(manifest) => manifest,
        None => return,
    };

    let plugin_id = manifest.id.trim().to_string();
    if plugin_id.is_empty() {
        tracing::info!(
            "[plugins] skipping plugin with empty id: {}",
            manifest_path.display()
        );
        return;
    }

    if manifest.kind.trim().is_empty() {
        tracing::info!(
            "[plugins] skipping plugin '{}' with empty kind: {}",
            plugin_id,
            manifest_path.display()
        );
        return;
    }

    let discovered_plugin = DiscoveredPlugin {
        manifest,
        dir: plugin_dir.to_path_buf(),
        source,
    };

    if let Some(previous) = discovered.get(&plugin_id) {
        report_shadowed(&plugin_id, plugin_dir, &previous.dir);
        return;
    }

    discovered.insert(plugin_id, discovered_plugin);
}

/// Shadowing decisions this process has already reported at INFO.
///
/// Discovery is stateless and re-run by every lookup, so without this gate
/// the same "ignored; already loaded" line would repeat on every pass. Each
/// distinct decision (plugin id, ignored dir, winning dir) is reported once
/// at INFO; repeats are demoted to DEBUG.
static REPORTED_SHADOWS: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();

fn report_shadowed(plugin_id: &str, ignored_dir: &Path, loaded_dir: &Path) {
    let message = format!(
        "[plugins] plugin '{}' from {} ignored; already loaded from {}",
        plugin_id,
        ignored_dir.display(),
        loaded_dir.display()
    );
    let first_report = REPORTED_SHADOWS
        .get_or_init(Default::default)
        .lock()
        .expect("reported shadows mutex")
        .insert(message.clone());
    if first_report {
        tracing::info!("{message}");
    } else {
        tracing::debug!("{message}");
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

fn read_package_version(path: &Path) -> Option<String> {
    let raw = std::fs::read_to_string(path).ok()?;
    let value = serde_json::from_str::<serde_json::Value>(&raw).ok()?;
    value
        .get("version")
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|version| !version.is_empty())
        .map(str::to_string)
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;

    fn manifest_with_capabilities(capabilities: serde_json::Value) -> PluginManifest {
        serde_json::from_value(serde_json::json!({
            "id": "slack",
            "name": "Slack",
            "version": "1.0.0",
            "kind": "channel",
            "runtime": "node",
            "entry": "index.js",
            "capabilities": capabilities
        }))
        .unwrap()
    }

    #[test]
    fn topic_scope_defaults_to_topic() {
        let manifest = manifest_with_capabilities(serde_json::json!({}));

        assert_eq!(
            manifest.capabilities.topic_scope,
            TopicConversationScope::Topic
        );
    }

    #[test]
    fn manifest_can_scope_topics_to_chat() {
        let manifest = manifest_with_capabilities(serde_json::json!({ "topicScope": "chat" }));

        assert_eq!(
            manifest.capabilities.topic_scope,
            TopicConversationScope::Chat
        );
    }

    /// In-memory `MakeWriter` so a test can assert on formatted log lines.
    #[derive(Clone, Default)]
    struct LogBuffer(Arc<Mutex<Vec<u8>>>);

    impl LogBuffer {
        fn lines(&self) -> Vec<String> {
            String::from_utf8(self.0.lock().unwrap().clone())
                .unwrap()
                .lines()
                .map(str::to_string)
                .collect()
        }
    }

    impl std::io::Write for LogBuffer {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(bytes);
            Ok(bytes.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    fn write_channel_manifest(plugin_dir: &Path, id: &str) {
        std::fs::create_dir_all(plugin_dir).unwrap();
        std::fs::write(
            plugin_dir.join(PLUGIN_MANIFEST_NAME),
            serde_json::json!({
                "id": id,
                "name": id,
                "version": "1.0.0",
                "kind": "channel",
                "runtime": "node",
                "entry": "dist/index.js"
            })
            .to_string(),
        )
        .unwrap();
    }

    /// The project-then-user pass `discover_plugins` runs, from explicit roots.
    fn discover_from(project_dir: &Path, user_dir: &Path) -> HashMap<String, DiscoveredPlugin> {
        let mut discovered = HashMap::new();
        load_plugins_from_dir(project_dir, PluginSource::Project, &mut discovered);
        load_plugins_from_dir(user_dir, PluginSource::User, &mut discovered);
        discovered
    }

    #[test]
    fn project_plugin_shadows_user_copy_and_reports_it_once() {
        let root = std::env::temp_dir().join(format!("va-plugins-shadow-{}", uuid::Uuid::new_v4()));
        let project_plugin = root.join("project").join("va-plugin-channel-feishu");
        let user_plugin = root.join("user").join("va-plugin-channel-feishu");
        write_channel_manifest(&project_plugin, "feishu");
        write_channel_manifest(&user_plugin, "feishu");

        let logs = LogBuffer::default();
        let subscriber = tracing_subscriber::fmt()
            .with_max_level(tracing::Level::DEBUG)
            .with_ansi(false)
            .without_time()
            .with_writer({
                let logs = logs.clone();
                move || logs.clone()
            })
            .finish();

        let passes = tracing::subscriber::with_default(subscriber, || {
            (0..3)
                .map(|_| discover_from(&root.join("project"), &root.join("user")))
                .collect::<Vec<_>>()
        });

        for discovered in &passes {
            let feishu = &discovered["feishu"];
            assert_eq!(feishu.dir, project_plugin);
            assert!(matches!(feishu.source, PluginSource::Project));
        }

        let shadow_lines = logs
            .lines()
            .into_iter()
            .filter(|line| line.contains("ignored; already loaded from"))
            .collect::<Vec<_>>();
        assert_eq!(shadow_lines.len(), 3, "{shadow_lines:#?}");
        assert!(shadow_lines[0].contains("INFO"), "{}", shadow_lines[0]);
        assert!(shadow_lines[0].contains(&format!(
            "plugin 'feishu' from {} ignored; already loaded from {}",
            user_plugin.display(),
            project_plugin.display()
        )));
        assert!(
            shadow_lines[1..].iter().all(|line| line.contains("DEBUG")),
            "{shadow_lines:#?}"
        );

        std::fs::remove_dir_all(&root).unwrap();
    }
}
