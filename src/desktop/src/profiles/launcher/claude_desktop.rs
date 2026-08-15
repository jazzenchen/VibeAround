//! Claude Desktop third-party profile config.
//!
//! Claude Desktop stores third-party inference profiles under the `Claude-3p`
//! user data directory. Profile launches point Claude at VibeAround's local
//! bridge; direct launches remove VibeAround's managed profile and restore the
//! user's previous selection.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use ::common::profiles::{self, connections, schema};
use ::common::{agent_state, auth, config};
use anyhow::{anyhow, Context};
use profiles::ProfileDef;
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

const MANAGED_ENTRY_PREFIX: &str = "VibeAround: ";
const STATE_FILE_NAME: &str = ".vibearound-claude-desktop-state.json";
const CLAUDE_DESKTOP_BRIDGE_MODEL_IDS: &[&str] = &[
    "claude-opus-4-8[1m]",
    "claude-opus-4-7[1m]",
    "claude-opus-4-6[1m]",
    "claude-opus-4-5[1m]",
    "claude-sonnet-4-6[1m]",
    "claude-sonnet-4-5[1m]",
    "claude-sonnet-4[1m]",
    "claude-haiku-4-5[1m]",
    "claude-haiku-3-5[1m]",
    "claude-haiku-3[1m]",
];

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct ConfigLibraryMeta {
    #[serde(
        rename = "appliedId",
        default,
        skip_serializing_if = "String::is_empty"
    )]
    applied_id: String,
    #[serde(default)]
    entries: Vec<ConfigLibraryEntry>,
    #[serde(flatten)]
    extra: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ConfigLibraryEntry {
    id: String,
    name: String,
    #[serde(flatten)]
    extra: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct VibeAroundClaudeDesktopState {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    previous_applied_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    previous_deployment_mode: Option<String>,
}

pub(super) fn apply_profile_config(profile: &ProfileDef) -> anyhow::Result<()> {
    let route = crate::profiles::resolve_profile_agent_route(profile, "claude-desktop")
        .ok_or_else(|| anyhow!("profile '{}' cannot launch Claude Desktop", profile.id))?;
    let target_api_type = route
        .bridge_target_api_type
        .clone()
        .unwrap_or_else(|| route.client_api_type.clone());
    ensure_claude_bridge_agent_model(profile, &route, &target_api_type)
        .with_context(|| format!("prepare Claude Desktop bridge model for '{}'", profile.id))?;
    let scope = format!("claude-desktop-{}", route.client_api_type);
    let base_url = bridge_base_url(&profile.id, &scope, &target_api_type);
    apply_profile_config_at(&claude_3p_user_data_dir(), profile, &base_url)
}

pub(super) fn cleanup_profile_config() -> anyhow::Result<()> {
    cleanup_profile_config_at(&claude_3p_user_data_dir())
}

fn bridge_base_url(profile_id: &str, scope: &str, target_api_type: &str) -> String {
    format!(
        "http://127.0.0.1:{}/va/local-api/{}/{}/{}",
        config::DEFAULT_PORT,
        profile_id,
        scope,
        target_api_type
    )
}

fn ensure_claude_bridge_agent_model(
    profile: &ProfileDef,
    route: &connections::ProfileAgentRoute,
    target_api_type: &str,
) -> anyhow::Result<()> {
    let model_routes = claude_bridge_model_routes(profile, route, target_api_type);
    if model_routes.is_empty() {
        anyhow::bail!(
            "profile '{}' has no models available for Claude Desktop",
            profile.id
        );
    }
    let merged_connections = connections::merged_profile_connections();
    let fallback_preference = merged_connections
        .get(&profile.id)
        .and_then(|items| items.get("claude-desktop"))
        .cloned()
        .unwrap_or_default();
    let profile_id = profile.id.clone();
    let updated = schema::update(&profile_id, |current| {
        let mut preference = current
            .connections
            .get("claude-desktop")
            .cloned()
            .unwrap_or(fallback_preference);
        if upsert_claude_bridge_agent_model_preference(
            &mut preference,
            &route.client_api_type,
            target_api_type,
            &model_routes,
        ) {
            schema::set_connection(current, "claude-desktop", preference);
        }
        Ok(())
    })?;
    if updated.is_none() {
        anyhow::bail!("profile '{}' was removed during launch", profile_id);
    }
    Ok(())
}

fn claude_bridge_model_routes(
    profile: &ProfileDef,
    route: &connections::ProfileAgentRoute,
    target_api_type: &str,
) -> Vec<connections::ProfileBridgeModelRoute> {
    if route.bridge_models.is_empty() {
        connections::bridge_model_routes(profile, None, target_api_type)
    } else {
        route.bridge_models.clone()
    }
}

fn upsert_claude_bridge_agent_model_preference(
    preference: &mut agent_state::ProfileConnectionPreference,
    client_api_type: &str,
    target_api_type: &str,
    model_routes: &[connections::ProfileBridgeModelRoute],
) -> bool {
    let mut changed = false;
    if preference.selected_api_type.as_deref() != Some(client_api_type) {
        preference.selected_api_type = Some(client_api_type.to_string());
        changed = true;
    }

    let bridge = preference
        .bridge
        .entry(client_api_type.to_string())
        .or_default();
    if !bridge.enabled {
        bridge.enabled = true;
        changed = true;
    }
    if bridge.target_api_type.as_deref() != Some(target_api_type) {
        bridge.target_api_type = Some(target_api_type.to_string());
        changed = true;
    }

    let Some(first_route) = model_routes.first() else {
        return changed;
    };

    let generated_models = bridge.models.is_empty();
    if generated_models {
        let legacy_fake_model = bridge
            .fake_model_id
            .as_deref()
            .filter(|value| connections::is_claude_usable_model_id(value))
            .map(ToOwned::to_owned);
        let mut used_agent_models = Vec::new();
        for (index, route) in model_routes.iter().enumerate() {
            let agent_model = if index == 0 {
                legacy_fake_model.clone().unwrap_or_else(|| {
                    claude_desktop_agent_model_id(route, index, &used_agent_models)
                })
            } else {
                claude_desktop_agent_model_id(route, index, &used_agent_models)
            };
            used_agent_models.push(agent_model.clone());
            bridge
                .models
                .push(agent_state::ProfileBridgeModelPreference {
                    upstream_model: Some(route.upstream_model.clone()),
                    fake_model_id: Some(agent_model),
                    capabilities: Default::default(),
                });
        }
        changed = true;
    }

    let mut legacy_fake_model = bridge
        .fake_model_id
        .as_deref()
        .filter(|value| connections::is_claude_usable_model_id(value))
        .map(ToOwned::to_owned);
    let mut used_agent_models: Vec<String> = Vec::new();
    for (index, model) in bridge.models.iter_mut().enumerate() {
        if model
            .fake_model_id
            .as_deref()
            .is_some_and(connections::is_claude_usable_model_id)
        {
            if let Some(fake_model_id) = model.fake_model_id.as_ref() {
                used_agent_models.push(fake_model_id.clone());
            }
            continue;
        }
        let legacy_fake_model = if index == 0 {
            legacy_fake_model.take().filter(|fake_model_id| {
                !used_agent_models
                    .iter()
                    .any(|existing| existing == fake_model_id)
            })
        } else {
            None
        };
        let route = model
            .upstream_model
            .as_deref()
            .and_then(|upstream| {
                model_routes
                    .iter()
                    .find(|route| route.upstream_model == upstream)
            })
            .unwrap_or_else(|| {
                model_routes
                    .get(index)
                    .unwrap_or_else(|| model_routes.last().expect("non-empty model routes"))
            });
        let agent_model = legacy_fake_model
            .unwrap_or_else(|| claude_desktop_agent_model_id(route, index, &used_agent_models));
        model.fake_model_id = Some(agent_model.clone());
        used_agent_models.push(agent_model);
        changed = true;
    }

    for route in model_routes {
        if bridge
            .models
            .iter()
            .any(|model| model.upstream_model.as_deref() == Some(route.upstream_model.as_str()))
        {
            continue;
        }
        let agent_model =
            claude_desktop_agent_model_id(route, bridge.models.len(), &used_agent_models);
        bridge
            .models
            .push(agent_state::ProfileBridgeModelPreference {
                upstream_model: Some(route.upstream_model.clone()),
                fake_model_id: Some(agent_model.clone()),
                capabilities: Default::default(),
            });
        used_agent_models.push(agent_model);
        changed = true;
    }
    if generated_models {
        let Some(first_model) = bridge.models.first() else {
            if bridge.upstream_model.as_deref() != Some(first_route.upstream_model.as_str()) {
                bridge.upstream_model = Some(first_route.upstream_model.clone());
                changed = true;
            }
            return changed;
        };
        if bridge.upstream_model != first_model.upstream_model {
            bridge.upstream_model = first_model.upstream_model.clone();
            changed = true;
        }
        if bridge.fake_model_id != first_model.fake_model_id {
            bridge.fake_model_id = first_model.fake_model_id.clone();
            changed = true;
        }
    }
    changed
}

fn claude_desktop_agent_model_id(
    route: &connections::ProfileBridgeModelRoute,
    index: usize,
    used: &[String],
) -> String {
    if connections::is_claude_usable_model_id(&route.agent_model)
        && !used.iter().any(|existing| existing == &route.agent_model)
    {
        return route.agent_model.clone();
    }
    CLAUDE_DESKTOP_BRIDGE_MODEL_IDS
        .iter()
        .find(|model| !used.iter().any(|existing| existing == **model))
        .map(|model| (*model).to_string())
        .unwrap_or_else(|| format!("claude-sonnet-4-5-{}", index + 1))
}

fn apply_profile_config_at(
    root: &Path,
    profile: &ProfileDef,
    base_url: &str,
) -> anyhow::Result<()> {
    let library_dir = config_library_dir(root);
    std::fs::create_dir_all(&library_dir)
        .with_context(|| format!("create Claude Desktop config library {:?}", library_dir))?;

    let mut meta = read_json_or_default::<ConfigLibraryMeta>(&meta_path(root))
        .with_context(|| format!("read Claude Desktop config library {:?}", meta_path(root)))?;
    let previous_state =
        read_json_or_default::<VibeAroundClaudeDesktopState>(&state_path(root)).ok();
    let previous_applied_id = previous_state
        .as_ref()
        .and_then(|state| state.previous_applied_id.clone())
        .or_else(|| previous_applied_id_from_meta(&meta));
    let previous_deployment_mode = previous_state
        .as_ref()
        .and_then(|state| state.previous_deployment_mode.clone())
        .or_else(|| read_deployment_mode(root).ok().flatten());

    remove_managed_entries(&mut meta, &library_dir)?;

    let entry_id = managed_entry_id(&profile.id);
    let entry_name = format!("{MANAGED_ENTRY_PREFIX}{}", profile.label);
    write_json(
        &library_dir.join(format!("{entry_id}.json")),
        &serde_json::json!({
            "inferenceProvider": "gateway",
            "inferenceGatewayBaseUrl": base_url,
            "inferenceGatewayApiKey": "vibearound-local-bridge",
            "inferenceGatewayAuthScheme": "bearer",
            "modelDiscoveryEnabled": true,
        }),
    )
    .with_context(|| format!("write Claude Desktop profile '{}'", profile.id))?;

    meta.entries.push(ConfigLibraryEntry {
        id: entry_id.clone(),
        name: entry_name,
        extra: BTreeMap::new(),
    });
    meta.applied_id = entry_id;
    write_json(&meta_path(root), &meta).context("write Claude Desktop config library metadata")?;
    write_json(
        &state_path(root),
        &VibeAroundClaudeDesktopState {
            previous_applied_id,
            previous_deployment_mode,
        },
    )
    .context("write Claude Desktop VibeAround state")?;
    set_deployment_mode(root, Some("3p")).context("switch Claude Desktop to third-party mode")?;
    Ok(())
}

fn cleanup_profile_config_at(root: &Path) -> anyhow::Result<()> {
    let library_dir = config_library_dir(root);
    let state_path = state_path(root);
    let has_state = state_path.exists();
    let state =
        read_json_or_default::<VibeAroundClaudeDesktopState>(&state_path).unwrap_or_default();
    let meta_file = meta_path(root);
    if !meta_file.exists() {
        if has_state {
            restore_deployment_mode(root, state.previous_deployment_mode.as_deref())?;
        }
        remove_file_if_exists(&state_path)?;
        return Ok(());
    }
    let mut meta = match read_json_or_default::<ConfigLibraryMeta>(&meta_file) {
        Ok(meta) => meta,
        Err(error) => return Err(error).context("read Claude Desktop config library metadata"),
    };

    let (removed_any, removed_applied) = remove_managed_entries(&mut meta, &library_dir)?;
    if !has_state && !removed_any {
        return Ok(());
    }
    if let Some(previous_id) = state
        .previous_applied_id
        .as_deref()
        .filter(|id| meta.entries.iter().any(|entry| entry.id == **id))
    {
        meta.applied_id = previous_id.to_string();
    } else if removed_applied || !meta.entries.iter().any(|entry| entry.id == meta.applied_id) {
        meta.applied_id = meta
            .entries
            .first()
            .map(|entry| entry.id.clone())
            .unwrap_or_default();
    }

    write_json(&meta_file, &meta)
        .context("write restored Claude Desktop config library metadata")?;
    if has_state {
        restore_deployment_mode(root, state.previous_deployment_mode.as_deref())?;
    }
    remove_file_if_exists(&state_path)?;
    Ok(())
}

fn previous_applied_id_from_meta(meta: &ConfigLibraryMeta) -> Option<String> {
    if meta.applied_id.is_empty()
        || meta
            .entries
            .iter()
            .any(|entry| entry.id == meta.applied_id && is_managed_entry(entry))
    {
        return None;
    }
    Some(meta.applied_id.clone())
}

fn remove_managed_entries(
    meta: &mut ConfigLibraryMeta,
    library_dir: &Path,
) -> anyhow::Result<(bool, bool)> {
    let mut removed_any = false;
    let mut removed_applied = false;
    let applied_id = meta.applied_id.clone();
    let mut managed_ids = Vec::new();
    meta.entries.retain(|entry| {
        if is_managed_entry(entry) {
            removed_any = true;
            if entry.id == applied_id {
                removed_applied = true;
            }
            managed_ids.push(entry.id.clone());
            false
        } else {
            true
        }
    });

    for id in managed_ids {
        remove_file_if_exists(&library_dir.join(format!("{id}.json")))?;
    }
    Ok((removed_any, removed_applied))
}

fn is_managed_entry(entry: &ConfigLibraryEntry) -> bool {
    entry.name.starts_with(MANAGED_ENTRY_PREFIX)
}

fn managed_entry_id(profile_id: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"vibearound:claude-desktop:");
    hasher.update(profile_id.as_bytes());
    let digest = hasher.finalize();
    let mut bytes = [0_u8; 16];
    bytes.copy_from_slice(&digest[..16]);
    bytes[6] = (bytes[6] & 0x0f) | 0x50;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    uuid::Uuid::from_bytes(bytes).to_string()
}

fn read_deployment_mode(root: &Path) -> anyhow::Result<Option<String>> {
    let value = read_json_or_default::<Value>(&deployment_config_path(root))?;
    Ok(value
        .get("deploymentMode")
        .and_then(Value::as_str)
        .map(ToOwned::to_owned))
}

fn set_deployment_mode(root: &Path, mode: Option<&str>) -> anyhow::Result<()> {
    let path = deployment_config_path(root);
    let mut value = read_json_or_default::<Value>(&path)
        .with_context(|| format!("read Claude Desktop deployment config {:?}", path))?;
    if !value.is_object() {
        value = Value::Object(Default::default());
    }
    let object = value
        .as_object_mut()
        .expect("deployment config value was normalized to object");
    match mode {
        Some(mode) => {
            object.insert(
                "deploymentMode".to_string(),
                Value::String(mode.to_string()),
            );
        }
        None => {
            object.remove("deploymentMode");
        }
    }
    write_json(&path, &value)
}

fn restore_deployment_mode(root: &Path, previous: Option<&str>) -> anyhow::Result<()> {
    set_deployment_mode(root, previous)
}

fn read_json_or_default<T>(path: &Path) -> anyhow::Result<T>
where
    T: DeserializeOwned + Default,
{
    match std::fs::read_to_string(path) {
        Ok(contents) => {
            serde_json::from_str(&contents).with_context(|| format!("parse JSON file {:?}", path))
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(T::default()),
        Err(error) => Err(error).with_context(|| format!("read JSON file {:?}", path)),
    }
}

fn write_json<T>(path: &Path, value: &T) -> anyhow::Result<()>
where
    T: Serialize + ?Sized,
{
    let next = serde_json::to_string_pretty(value).context("serialize Claude Desktop JSON")?;
    write_text_if_changed(path, next)
}

fn write_text_if_changed(path: &Path, next: String) -> anyhow::Result<()> {
    let current = std::fs::read_to_string(path).unwrap_or_default();
    if current == next {
        return Ok(());
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).with_context(|| format!("create dir {:?}", parent))?;
    }
    let tmp = path.with_file_name(format!(
        ".{}.{}.tmp",
        path.file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("claude-desktop-config.json"),
        uuid::Uuid::new_v4()
    ));
    std::fs::write(&tmp, next).with_context(|| format!("write temp file {:?}", tmp))?;
    auth::set_owner_only(&tmp).with_context(|| format!("chmod temp file {:?}", tmp))?;
    std::fs::rename(&tmp, path).with_context(|| format!("replace file {:?}", path))?;
    auth::set_owner_only(path).with_context(|| format!("chmod file {:?}", path))?;
    Ok(())
}

fn remove_file_if_exists(path: &Path) -> anyhow::Result<()> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error).with_context(|| format!("remove file {:?}", path)),
    }
}

fn claude_3p_user_data_dir() -> PathBuf {
    #[cfg(test)]
    if let Some(path) = test_user_data_dir() {
        return path;
    }

    #[cfg(target_os = "macos")]
    {
        config::home_dir()
            .join("Library")
            .join("Application Support")
            .join("Claude-3p")
    }
    #[cfg(target_os = "windows")]
    {
        if let Some(local_app_data) = std::env::var_os("LOCALAPPDATA") {
            return PathBuf::from(local_app_data).join("Claude-3p");
        }
        return config::home_dir()
            .join("AppData")
            .join("Local")
            .join("Claude-3p");
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        let base = std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| config::home_dir().join(".config"));
        base.join("Claude-3p")
    }
}

fn config_library_dir(root: &Path) -> PathBuf {
    root.join("configLibrary")
}

fn meta_path(root: &Path) -> PathBuf {
    config_library_dir(root).join("_meta.json")
}

fn state_path(root: &Path) -> PathBuf {
    root.join(STATE_FILE_NAME)
}

fn deployment_config_path(root: &Path) -> PathBuf {
    root.join("claude_desktop_config.json")
}

#[cfg(test)]
fn test_user_data_dir() -> Option<PathBuf> {
    test_root()
        .lock()
        .expect("Claude Desktop test root")
        .clone()
}

#[cfg(test)]
fn test_root() -> &'static std::sync::Mutex<Option<PathBuf>> {
    static ROOT: std::sync::OnceLock<std::sync::Mutex<Option<PathBuf>>> =
        std::sync::OnceLock::new();
    ROOT.get_or_init(|| std::sync::Mutex::new(None))
}

#[cfg(test)]
pub(super) struct TestUserDataDirGuard {
    previous: Option<PathBuf>,
    _lock: std::sync::MutexGuard<'static, ()>,
}

#[cfg(test)]
impl Drop for TestUserDataDirGuard {
    fn drop(&mut self) {
        *test_root().lock().expect("Claude Desktop test root") = self.previous.take();
    }
}

#[cfg(test)]
pub(super) fn set_test_user_data_dir(path: PathBuf) -> TestUserDataDirGuard {
    let lock = test_root_isolation()
        .lock()
        .expect("Claude Desktop test root isolation");
    let mut root = test_root().lock().expect("Claude Desktop test root");
    let previous = root.replace(path);
    TestUserDataDirGuard {
        previous,
        _lock: lock,
    }
}

#[cfg(test)]
fn test_root_isolation() -> &'static std::sync::Mutex<()> {
    static LOCK: std::sync::OnceLock<std::sync::Mutex<()>> = std::sync::OnceLock::new();
    LOCK.get_or_init(|| std::sync::Mutex::new(()))
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use ::common::profiles::schema::{AuthMode, ProfileDef};

    use super::*;

    fn profile() -> ProfileDef {
        ProfileDef {
            id: "minimax-test".to_string(),
            label: "MiniMax Test".to_string(),
            provider: "minimax".to_string(),
            auth_mode: AuthMode::ApiKey,
            api_types: vec!["anthropic".to_string()],
            credentials: BTreeMap::new(),
            overrides: BTreeMap::new(),
            api_configs: Default::default(),
            use_settings_proxy: false,
            provider_settings: Default::default(),
            connections: Default::default(),
        }
    }

    fn temp_root(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "vibearound-claude-desktop-{name}-{}",
            uuid::Uuid::new_v4()
        ))
    }

    #[test]
    fn apply_profile_config_writes_managed_gateway_and_selects_3p() {
        let root = temp_root("apply");
        std::fs::create_dir_all(config_library_dir(&root)).expect("create config library");
        write_json(
            &meta_path(&root),
            &ConfigLibraryMeta {
                applied_id: "default-id".to_string(),
                entries: vec![ConfigLibraryEntry {
                    id: "default-id".to_string(),
                    name: "Default".to_string(),
                    extra: BTreeMap::new(),
                }],
                extra: BTreeMap::new(),
            },
        )
        .expect("write meta");
        write_json(
            &deployment_config_path(&root),
            &serde_json::json!({ "deploymentMode": "1p", "preferences": { "theme": "dark" } }),
        )
        .expect("write deployment config");

        apply_profile_config_at(
            &root,
            &profile(),
            "http://127.0.0.1:12358/va/local-api/minimax-test/claude-desktop-anthropic/anthropic",
        )
        .expect("apply Claude Desktop profile");

        let meta: ConfigLibraryMeta = read_json_or_default(&meta_path(&root)).expect("read meta");
        assert_eq!(meta.entries.len(), 2);
        assert!(meta
            .entries
            .iter()
            .any(|entry| entry.name == "VibeAround: MiniMax Test"));
        let managed_path = config_library_dir(&root).join(format!("{}.json", meta.applied_id));
        let managed: Value = read_json_or_default(&managed_path).expect("read managed config");
        assert_eq!(
            managed.get("inferenceProvider").and_then(Value::as_str),
            Some("gateway")
        );
        assert_eq!(
            managed
                .get("inferenceGatewayBaseUrl")
                .and_then(Value::as_str),
            Some("http://127.0.0.1:12358/va/local-api/minimax-test/claude-desktop-anthropic/anthropic")
        );
        assert_eq!(
            managed
                .get("modelDiscoveryEnabled")
                .and_then(Value::as_bool),
            Some(true)
        );
        assert!(managed.get("inferenceModels").is_none());
        assert_eq!(
            read_deployment_mode(&root).expect("deployment mode"),
            Some("3p".to_string())
        );

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn cleanup_profile_config_restores_previous_selection() {
        let root = temp_root("cleanup");
        std::fs::create_dir_all(config_library_dir(&root)).expect("create config library");
        write_json(
            &meta_path(&root),
            &ConfigLibraryMeta {
                applied_id: "default-id".to_string(),
                entries: vec![ConfigLibraryEntry {
                    id: "default-id".to_string(),
                    name: "Default".to_string(),
                    extra: BTreeMap::new(),
                }],
                extra: BTreeMap::new(),
            },
        )
        .expect("write meta");
        write_json(
            &deployment_config_path(&root),
            &serde_json::json!({ "deploymentMode": "1p" }),
        )
        .expect("write deployment config");
        apply_profile_config_at(
            &root,
            &profile(),
            "http://127.0.0.1:12358/va/local-api/minimax-test/claude-desktop-anthropic/anthropic",
        )
        .expect("apply Claude Desktop profile");

        cleanup_profile_config_at(&root).expect("cleanup Claude Desktop profile");

        let meta: ConfigLibraryMeta = read_json_or_default(&meta_path(&root)).expect("read meta");
        assert_eq!(meta.applied_id, "default-id");
        assert_eq!(meta.entries.len(), 1);
        assert_eq!(meta.entries[0].name, "Default");
        assert_eq!(
            read_deployment_mode(&root).expect("deployment mode"),
            Some("1p".to_string())
        );
        assert!(!state_path(&root).exists());

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn cleanup_without_managed_state_leaves_user_config_unchanged() {
        let root = temp_root("noop-cleanup");
        std::fs::create_dir_all(config_library_dir(&root)).expect("create config library");
        let meta = r#"{"appliedId":"default-id","entries":[{"id":"default-id","name":"Default"}]}"#;
        let deployment = r#"{"deploymentMode":"3p"}"#;
        std::fs::write(meta_path(&root), meta).expect("write meta");
        std::fs::write(deployment_config_path(&root), deployment).expect("write deployment config");

        cleanup_profile_config_at(&root).expect("cleanup should no-op");

        assert_eq!(
            std::fs::read_to_string(meta_path(&root)).expect("read meta"),
            meta
        );
        assert_eq!(
            std::fs::read_to_string(deployment_config_path(&root)).expect("read deployment config"),
            deployment
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn upsert_claude_bridge_agent_model_fills_single_model_fake_id() {
        let mut preference = agent_state::ProfileConnectionPreference {
            selected_api_type: Some("anthropic".to_string()),
            bridge: [(
                "anthropic".to_string(),
                agent_state::ProfileBridgePreference {
                    enabled: true,
                    target_api_type: Some("openai-chat".to_string()),
                    upstream_model: Some("nvidia/nemotron".to_string()),
                    ..Default::default()
                },
            )]
            .into_iter()
            .collect(),
        };

        let changed = upsert_claude_bridge_agent_model_preference(
            &mut preference,
            "anthropic",
            "openai-chat",
            &[connections::ProfileBridgeModelRoute {
                upstream_model: "nvidia/nemotron".to_string(),
                agent_model: "nvidia/nemotron".to_string(),
                capabilities: Default::default(),
            }],
        );

        assert!(changed);
        let bridge = preference.bridge.get("anthropic").expect("bridge");
        assert_eq!(bridge.upstream_model.as_deref(), Some("nvidia/nemotron"));
        assert_eq!(bridge.fake_model_id.as_deref(), Some("claude-opus-4-8[1m]"));
        assert_eq!(bridge.models.len(), 1);
        assert_eq!(
            bridge.models[0].fake_model_id.as_deref(),
            Some("claude-opus-4-8[1m]")
        );
    }

    #[test]
    fn upsert_claude_bridge_agent_model_preserves_existing_claude_style_model() {
        let mut preference = agent_state::ProfileConnectionPreference {
            selected_api_type: Some("anthropic".to_string()),
            bridge: [(
                "anthropic".to_string(),
                agent_state::ProfileBridgePreference {
                    enabled: true,
                    target_api_type: Some("openai-chat".to_string()),
                    models: vec![agent_state::ProfileBridgeModelPreference {
                        upstream_model: Some("deepseek-v4-pro".to_string()),
                        fake_model_id: Some("opus-4.7[1m]".to_string()),
                        capabilities: Default::default(),
                    }],
                    ..Default::default()
                },
            )]
            .into_iter()
            .collect(),
        };

        let changed = upsert_claude_bridge_agent_model_preference(
            &mut preference,
            "anthropic",
            "openai-chat",
            &[connections::ProfileBridgeModelRoute {
                upstream_model: "deepseek-v4-pro".to_string(),
                agent_model: "opus-4.7[1m]".to_string(),
                capabilities: Default::default(),
            }],
        );

        assert!(!changed);
        let bridge = preference.bridge.get("anthropic").expect("bridge");
        assert_eq!(
            bridge.models[0].fake_model_id.as_deref(),
            Some("opus-4.7[1m]")
        );
    }

    #[test]
    fn upsert_claude_bridge_agent_model_fills_model_list_fake_ids() {
        let mut preference = agent_state::ProfileConnectionPreference {
            selected_api_type: Some("anthropic".to_string()),
            bridge: [(
                "anthropic".to_string(),
                agent_state::ProfileBridgePreference {
                    enabled: true,
                    target_api_type: Some("gemini".to_string()),
                    upstream_model: Some("gemini-2.5-flash".to_string()),
                    models: vec![
                        agent_state::ProfileBridgeModelPreference {
                            upstream_model: Some("gemini-2.5-flash".to_string()),
                            fake_model_id: None,
                            capabilities: Default::default(),
                        },
                        agent_state::ProfileBridgeModelPreference {
                            upstream_model: Some("gemini-3.1-flash-lite".to_string()),
                            fake_model_id: None,
                            capabilities: Default::default(),
                        },
                        agent_state::ProfileBridgeModelPreference {
                            upstream_model: Some("gemini-2.5-pro".to_string()),
                            fake_model_id: None,
                            capabilities: Default::default(),
                        },
                    ],
                    ..Default::default()
                },
            )]
            .into_iter()
            .collect(),
        };

        let changed = upsert_claude_bridge_agent_model_preference(
            &mut preference,
            "anthropic",
            "gemini",
            &[
                connections::ProfileBridgeModelRoute {
                    upstream_model: "gemini-2.5-flash".to_string(),
                    agent_model: "gemini-2.5-flash".to_string(),
                    capabilities: Default::default(),
                },
                connections::ProfileBridgeModelRoute {
                    upstream_model: "gemini-3.1-flash-lite".to_string(),
                    agent_model: "gemini-3.1-flash-lite".to_string(),
                    capabilities: Default::default(),
                },
                connections::ProfileBridgeModelRoute {
                    upstream_model: "gemini-2.5-pro".to_string(),
                    agent_model: "gemini-2.5-pro".to_string(),
                    capabilities: Default::default(),
                },
            ],
        );

        assert!(changed);
        let bridge = preference.bridge.get("anthropic").expect("bridge");
        let fake_ids: Vec<_> = bridge
            .models
            .iter()
            .map(|model| model.fake_model_id.as_deref())
            .collect();
        assert_eq!(
            fake_ids,
            vec![
                Some("claude-opus-4-8[1m]"),
                Some("claude-opus-4-7[1m]"),
                Some("claude-opus-4-6[1m]"),
            ]
        );
    }
}
