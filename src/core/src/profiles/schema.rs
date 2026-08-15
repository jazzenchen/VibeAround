//! Profile JSON schema + on-disk CRUD.
//!
//! Each profile is a single flat file at `~/.vibearound/profiles/<id>.json`
//! holding the user's third-party API credentials plus the catalog provider
//! id that describes how to render env / settings files for that endpoint.
//!
//! Profile id == filename stem; the schema enforces that they match so
//! a `cp foo.json bar.json` rename doesn't leave a stale internal id.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context};
use nanoid::nanoid;
use serde::{Deserialize, Serialize};

use super::catalog::{self, ContentCapabilities, EndpointDef, ModelDef};
use crate::{agent_state, config};

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AuthMode {
    ApiKey,
    OauthViaCli,
    GoogleOauth,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct ApiTypeOverrides {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub endpoint_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning_effort: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub capabilities: Option<ContentCapabilities>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct ProfileApiConfig {
    #[serde(default, skip_serializing_if = "is_false")]
    pub enabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub endpoint_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub append_v1_path: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning_effort: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub capabilities: Option<ContentCapabilities>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub headers: Vec<ProfileHeaderConfig>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub models: Vec<ProfileModelConfig>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct ProfileHeaderConfig {
    pub name: String,
    pub value: String,
    #[serde(default = "default_true", skip_serializing_if = "is_true")]
    pub enabled: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub locked: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct ProfileModelConfig {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(default = "default_true", skip_serializing_if = "is_true")]
    pub enabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_window: Option<u64>,
    #[serde(default, skip_serializing_if = "ContentCapabilities::is_empty")]
    pub capabilities: ContentCapabilities,
    #[serde(default, skip_serializing_if = "is_false")]
    pub custom: bool,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq, Eq)]
pub struct ProviderSettings {
    #[serde(default, skip_serializing_if = "DeepSeekProviderSettings::is_empty")]
    pub deepseek: DeepSeekProviderSettings,
}

impl ProviderSettings {
    pub fn is_empty(&self) -> bool {
        self.deepseek.is_empty()
    }
}

#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq, Eq)]
pub struct DeepSeekProviderSettings {
    #[serde(default, skip_serializing_if = "is_false")]
    pub thinking: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub replay_reasoning_content: bool,
}

impl DeepSeekProviderSettings {
    pub fn is_empty(&self) -> bool {
        !self.thinking && !self.replay_reasoning_content
    }
}

fn is_false(value: &bool) -> bool {
    !*value
}

fn is_true(value: &bool) -> bool {
    *value
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ProfileDef {
    pub id: String,
    pub label: String,
    /// Catalog provider id (e.g. `"moonshot"`). Reserved value `"custom"` is
    /// not yet supported in v1; UI gates this.
    pub provider: String,
    pub auth_mode: AuthMode,
    /// Legacy migration input. Normal profile code uses `api_configs` only.
    pub api_types: Vec<String>,
    /// Free-form credentials — `api_key` is the only field used by v1
    /// catalog entries, but we keep the bag generic so future plugins can
    /// declare custom field names without a schema migration.
    #[serde(default)]
    pub credentials: BTreeMap<String, String>,
    /// Legacy migration input. Normal profile code uses `api_configs` only.
    #[serde(default)]
    pub overrides: BTreeMap<String, ApiTypeOverrides>,
    /// Editable API configs cloned from the provider catalog.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub api_configs: BTreeMap<String, ProfileApiConfig>,
    /// When true, provider-bound requests for this profile use the global
    /// Settings HTTP proxy when that proxy is enabled.
    #[serde(default, skip_serializing_if = "is_false")]
    pub use_settings_proxy: bool,
    /// Provider-specific behavior. Missing fields intentionally deserialize
    /// to false/empty so existing profile JSON never gains new behavior
    /// unless the user explicitly saves it.
    #[serde(default, skip_serializing_if = "ProviderSettings::is_empty")]
    pub provider_settings: ProviderSettings,
    /// Per-agent API bridge/client protocol preferences for this profile.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub connections: BTreeMap<String, agent_state::ProfileConnectionPreference>,
}

// ---------------------------------------------------------------------------
// Filesystem layout
// ---------------------------------------------------------------------------

pub fn profiles_dir() -> PathBuf {
    config::data_dir().join("profiles")
}

fn profile_path(id: &str) -> PathBuf {
    profile_path_in(&profiles_dir(), id)
}

fn profile_path_in(dir: &Path, id: &str) -> PathBuf {
    dir.join(format!("{id}.json"))
}

fn profile_lock_path(dir: &Path, id: &str) -> PathBuf {
    dir.join(format!(".{id}.json.lock"))
}

// ---------------------------------------------------------------------------
// Validation
// ---------------------------------------------------------------------------

const MAX_PROFILE_ID_LEN: usize = 64;
const GENERATED_ID_SUFFIX_LEN: usize = 12;
const GENERATED_ID_ATTEMPTS: usize = 16;
const GENERATED_ID_ALPHABET: [char; 36] = [
    'a', 'b', 'c', 'd', 'e', 'f', 'g', 'h', 'i', 'j', 'k', 'l', 'm', 'n', 'o', 'p', 'q', 'r', 's',
    't', 'u', 'v', 'w', 'x', 'y', 'z', '0', '1', '2', '3', '4', '5', '6', '7', '8', '9',
];

/// Profile ids form filenames + are exposed to shells; constrain them to a
/// safe alphabet so a malicious id can't escape the profiles directory or
/// confuse downstream consumers.
pub fn is_valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= MAX_PROFILE_ID_LEN
        && id
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '-')
}

pub fn validate(profile: &ProfileDef) -> anyhow::Result<()> {
    if !is_valid_id(&profile.id) {
        bail!(
            "invalid profile id '{}': must match ^[a-z0-9_-]{{1,64}}$",
            profile.id
        );
    }
    if profile.label.trim().is_empty() {
        bail!("profile label must not be empty");
    }
    if !profile.api_configs.values().any(|config| config.enabled) {
        bail!("profile must declare at least one api kind");
    }
    Ok(())
}

pub fn enabled_api_types(profile: &ProfileDef) -> Vec<String> {
    profile
        .api_configs
        .iter()
        .filter(|(_, config)| config.enabled)
        .map(|(api_type, _)| api_type.clone())
        .collect()
}

pub fn api_config_for(profile: &ProfileDef, api_type: &str) -> Option<ProfileApiConfig> {
    profile.api_configs.get(api_type).cloned()
}

pub(crate) fn hydrate_legacy_api_configs(profile: &mut ProfileDef) {
    let Some(provider) = catalog::get(&profile.provider) else {
        return;
    };
    let api_types = profile.api_types.clone();
    for api_type in api_types {
        if profile.api_configs.contains_key(&api_type) {
            continue;
        }
        if let Some(config) = legacy_api_config(profile, provider, &api_type) {
            profile.api_configs.insert(api_type, config);
        }
    }
}

fn legacy_api_config(
    profile: &ProfileDef,
    provider: &catalog::ProviderCatalog,
    api_type: &str,
) -> Option<ProfileApiConfig> {
    if !profile.api_types.iter().any(|item| item == api_type) {
        return None;
    }
    let overrides = profile.overrides.get(api_type).cloned().unwrap_or_default();
    let endpoint = catalog::find_endpoint(provider, api_type, overrides.endpoint_id.as_deref())?;
    Some(ProfileApiConfig {
        enabled: true,
        endpoint_id: overrides
            .endpoint_id
            .clone()
            .or_else(|| endpoint.id.clone())
            .or_else(|| Some(endpoint.api_type.clone())),
        base_url: overrides.base_url.clone().or_else(|| {
            (!endpoint.default_base_url.is_empty()).then(|| endpoint.default_base_url.clone())
        }),
        append_v1_path: Some(endpoint.append_v1_path),
        model: overrides
            .model
            .clone()
            .or_else(|| endpoint.models.first().map(|model| model.id.clone())),
        reasoning_effort: overrides.reasoning_effort.clone(),
        capabilities: overrides.capabilities.clone(),
        headers: Vec::new(),
        models: default_models(
            endpoint,
            overrides.model.as_deref(),
            overrides.capabilities.clone(),
        ),
    })
}

fn default_models(
    endpoint: &EndpointDef,
    selected_model: Option<&str>,
    capability_overrides: Option<ContentCapabilities>,
) -> Vec<ProfileModelConfig> {
    let mut models: Vec<_> = endpoint
        .models
        .iter()
        .map(model_config_from_catalog)
        .collect();
    if let Some(selected_model) = selected_model
        .map(str::trim)
        .filter(|model| !model.is_empty())
    {
        if catalog::canonical_model_id(endpoint, selected_model).is_none() {
            models.insert(
                0,
                ProfileModelConfig {
                    id: selected_model.to_string(),
                    label: None,
                    enabled: true,
                    context_window: None,
                    capabilities: capability_overrides.unwrap_or_default(),
                    custom: true,
                },
            );
        }
    }
    models
}

fn model_config_from_catalog(model: &ModelDef) -> ProfileModelConfig {
    ProfileModelConfig {
        id: model.id.clone(),
        label: model.label.clone(),
        enabled: true,
        context_window: model.context_window,
        capabilities: model.capabilities.clone(),
        custom: false,
    }
}

pub fn generate_unique_id(provider_id: &str) -> anyhow::Result<String> {
    let prefix = generated_id_prefix(provider_id)?;
    for _ in 0..GENERATED_ID_ATTEMPTS {
        let id = format!(
            "{prefix}-{}",
            nanoid!(GENERATED_ID_SUFFIX_LEN, &GENERATED_ID_ALPHABET)
        );
        if !profile_path(&id).exists() {
            return Ok(id);
        }
    }

    bail!(
        "failed to generate a unique profile id for provider '{}' after {} attempts",
        provider_id,
        GENERATED_ID_ATTEMPTS
    )
}

fn generated_id_prefix(provider_id: &str) -> anyhow::Result<String> {
    let provider_id = provider_id.trim();
    if !is_valid_id(provider_id) {
        bail!(
            "invalid provider id '{}': generated profile ids require ^[a-z0-9_-]{{1,64}}$ provider ids",
            provider_id
        );
    }

    let max_prefix_len = MAX_PROFILE_ID_LEN - GENERATED_ID_SUFFIX_LEN - 1;
    Ok(provider_id.chars().take(max_prefix_len).collect())
}

// ---------------------------------------------------------------------------
// CRUD
// ---------------------------------------------------------------------------

pub fn list() -> Vec<ProfileDef> {
    let dir = profiles_dir();
    let entries = match std::fs::read_dir(&dir) {
        Ok(e) => e,
        Err(_) => return Vec::new(),
    };

    let mut out = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("json") {
            continue;
        }
        match load_path(&path) {
            Ok(profile) => {
                let stem = path
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or_default();
                if profile.id != stem {
                    tracing::warn!(
                        "[profiles] skipping {:?}: id '{}' != filename stem '{}'",
                        path,
                        profile.id,
                        stem
                    );
                    continue;
                }
                out.push(profile);
            }
            Err(e) => {
                tracing::warn!("[profiles] skipping {:?}: {}", path, e);
            }
        }
    }
    out.sort_by(|a, b| a.label.to_lowercase().cmp(&b.label.to_lowercase()));
    out
}

pub fn load(id: &str) -> Option<ProfileDef> {
    if !is_valid_id(id) {
        return None;
    }
    load_path(&profile_path(id)).ok()
}

fn load_path(path: &Path) -> anyhow::Result<ProfileDef> {
    let body = std::fs::read_to_string(path).with_context(|| format!("read {:?}", path))?;
    let profile: ProfileDef =
        serde_json::from_str(&body).with_context(|| format!("parse {:?}", path))?;
    Ok(profile)
}

#[cfg(test)]
fn save_at(dir: &Path, profile: &ProfileDef) -> anyhow::Result<()> {
    let locked = LockedProfileFile::acquire_at(dir, &profile.id)?;
    locked.save(profile)
}

/// Load, mutate, and atomically replace one profile under its per-profile lock.
///
/// Returns `None` when the profile does not exist when the lock is acquired.
/// The mutator must not call another writer for the same profile id.
pub fn update<T>(
    id: &str,
    mutator: impl FnOnce(&mut ProfileDef) -> anyhow::Result<T>,
) -> anyhow::Result<Option<T>> {
    update_at(&profiles_dir(), id, mutator)
}

fn update_at<T>(
    dir: &Path,
    id: &str,
    mutator: impl FnOnce(&mut ProfileDef) -> anyhow::Result<T>,
) -> anyhow::Result<Option<T>> {
    if !is_valid_id(id) {
        bail!("invalid profile id '{}'", id);
    }
    if !dir
        .try_exists()
        .with_context(|| format!("inspect {:?}", dir))?
    {
        return Ok(None);
    }

    let _lock = LockedProfileFile::acquire_at(dir, id)?;
    let target = profile_path_in(dir, id);
    let body = match std::fs::read_to_string(&target) {
        Ok(body) => body,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error).with_context(|| format!("read {:?}", target)),
    };
    let mut profile: ProfileDef =
        serde_json::from_str(&body).with_context(|| format!("parse {:?}", target))?;
    if profile.id != id {
        bail!(
            "profile id '{}' does not match filename stem '{}'",
            profile.id,
            id
        );
    }

    let result = mutator(&mut profile)?;
    if profile.id != id {
        bail!(
            "profile update cannot change id '{}' to '{}'",
            id,
            profile.id
        );
    }
    let body = serialize_profile(&profile)?;
    crate::file_replace::write_private(&target, body)
        .with_context(|| format!("save profile '{}'", id))?;
    Ok(Some(result))
}

fn serialize_profile(profile: &ProfileDef) -> anyhow::Result<String> {
    validate(&profile)?;
    serde_json::to_string_pretty(profile).context("serialize profile")
}

fn ensure_profiles_dir(dir: &Path) -> anyhow::Result<()> {
    std::fs::create_dir_all(dir).with_context(|| format!("create {:?}", dir))?;
    // Lock down the profiles dir on Unix so other local users can't
    // enumerate or read API keys.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700));
    }
    Ok(())
}

fn acquire_profile_lock(
    dir: &Path,
    id: &str,
) -> anyhow::Result<crate::file_lock::ExclusiveFileLock> {
    let path = profile_lock_path(dir, id);
    crate::file_lock::ExclusiveFileLock::acquire(&path)
        .with_context(|| format!("lock profile '{}' at {}", id, path.display()))
}

/// One profile file held under its existing per-profile lock.
///
/// High-level profile operations may keep this guard alive while updating the
/// profile's settings references so save/delete calls for the same id remain
/// serializable across both files.
pub(crate) struct LockedProfileFile {
    dir: PathBuf,
    id: String,
    _lock: crate::file_lock::ExclusiveFileLock,
}

impl LockedProfileFile {
    pub(crate) fn acquire(id: &str) -> anyhow::Result<Self> {
        Self::acquire_at(&profiles_dir(), id)
    }

    fn acquire_at(dir: &Path, id: &str) -> anyhow::Result<Self> {
        if !is_valid_id(id) {
            bail!("invalid profile id '{}'", id);
        }
        ensure_profiles_dir(dir)?;
        let lock = acquire_profile_lock(dir, id)?;
        Ok(Self {
            dir: dir.to_path_buf(),
            id: id.to_string(),
            _lock: lock,
        })
    }

    pub(crate) fn save(&self, profile: &ProfileDef) -> anyhow::Result<()> {
        if profile.id != self.id {
            bail!(
                "locked profile id '{}' does not match profile id '{}'",
                self.id,
                profile.id
            );
        }
        let body = serialize_profile(profile)?;
        let target = profile_path_in(&self.dir, &self.id);
        crate::file_replace::write_private(&target, body)
            .with_context(|| format!("save profile '{}'", self.id))
    }

    pub(crate) fn delete(&self) -> anyhow::Result<()> {
        let path = profile_path_in(&self.dir, &self.id);
        match std::fs::remove_file(&path) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(error) => return Err(error).with_context(|| format!("remove {:?}", path)),
        }
        // Best-effort: also drop the per-profile state dir (rendered settings
        // files, future agent session caches). If the user re-creates a profile
        // with the same id later, we want a clean slate.
        let state_dir = config::data_dir().join("profile-state").join(&self.id);
        let _ = std::fs::remove_dir_all(&state_dir);
        Ok(())
    }
}

pub fn set_connection(
    profile: &mut ProfileDef,
    agent_id: &str,
    preference: agent_state::ProfileConnectionPreference,
) {
    if agent_state::connection_preference_is_empty(&preference) {
        profile.connections.remove(agent_id);
    } else {
        profile.connections.insert(agent_id.to_string(), preference);
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn test_dir(label: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "vibearound-profile-schema-{label}-{}-{}",
            std::process::id(),
            nanoid::nanoid!(8)
        ))
    }

    fn test_profile(id: &str, label: &str) -> ProfileDef {
        serde_json::from_value(serde_json::json!({
            "id": id,
            "label": label,
            "provider": "test",
            "auth_mode": "api_key",
            "api_types": ["openai-chat"],
            "api_configs": {
                "openai-chat": { "enabled": true }
            }
        }))
        .unwrap()
    }

    #[test]
    fn id_alphabet_accepts_lowercase_alnum_dash_underscore() {
        assert!(is_valid_id("kimi"));
        assert!(is_valid_id("kimi-personal"));
        assert!(is_valid_id("kimi_personal"));
        assert!(is_valid_id("a1"));
    }

    #[test]
    fn id_alphabet_rejects_unsafe_chars() {
        assert!(!is_valid_id(""));
        assert!(!is_valid_id("Kimi"));
        assert!(!is_valid_id("kimi/etc"));
        assert!(!is_valid_id("../etc"));
        assert!(!is_valid_id("kimi.personal"));
        assert!(!is_valid_id(&"a".repeat(65)));
    }

    #[test]
    fn generated_id_prefix_preserves_valid_provider_id() {
        assert_eq!(generated_id_prefix("deepseek").unwrap(), "deepseek");
        assert_eq!(generated_id_prefix("qwen-coding").unwrap(), "qwen-coding");
    }

    #[test]
    fn generated_id_prefix_truncates_to_leave_suffix_room() {
        assert_eq!(generated_id_prefix(&"a".repeat(64)).unwrap().len(), 51);
    }

    #[test]
    fn provider_settings_default_to_empty_for_existing_profiles() {
        let profile: ProfileDef = serde_json::from_str(
            r#"{
                "id": "deepseek",
                "label": "DeepSeek",
                "provider": "deepseek",
                "auth_mode": "api_key",
                "api_types": ["openai-chat"]
            }"#,
        )
        .unwrap();

        assert!(!profile.provider_settings.deepseek.thinking);
        assert!(!profile.provider_settings.deepseek.replay_reasoning_content);
        assert!(profile.connections.is_empty());

        let body = serde_json::to_string(&profile).unwrap();
        assert!(!body.contains("provider_settings"));
        assert!(!body.contains("connections"));
    }

    #[test]
    fn hydrates_legacy_custom_profile_api_configs() {
        let mut profile: ProfileDef = serde_json::from_str(
            r#"{
                "id": "sensenova",
                "label": "SenseNova",
                "provider": "custom",
                "auth_mode": "api_key",
                "api_types": ["anthropic", "openai-chat"],
                "credentials": { "api_key": "sk-test" },
                "overrides": {
                    "anthropic": {
                        "base_url": "https://token.sensenova.cn",
                        "model": "sensenova-6.7-flash-lite"
                    },
                    "openai-chat": {
                        "base_url": "https://token.sensenova.cn/v1",
                        "model": "sensenova-6.7-flash-lite"
                    }
                }
            }"#,
        )
        .unwrap();

        hydrate_legacy_api_configs(&mut profile);

        let anthropic = profile.api_configs.get("anthropic").unwrap();
        assert!(anthropic.enabled);
        assert_eq!(
            anthropic.base_url.as_deref(),
            Some("https://token.sensenova.cn")
        );
        assert_eq!(anthropic.model.as_deref(), Some("sensenova-6.7-flash-lite"));
        assert!(anthropic.headers.is_empty());
        assert!(anthropic.models[0].custom);

        let chat = profile.api_configs.get("openai-chat").unwrap();
        assert_eq!(
            chat.base_url.as_deref(),
            Some("https://token.sensenova.cn/v1")
        );
        assert!(chat.headers.is_empty());
    }

    #[test]
    fn concurrent_profile_saves_leave_one_complete_private_file() {
        use std::sync::{Arc, Barrier};

        let dir = test_dir("concurrent-save");
        let start = Arc::new(Barrier::new(9));
        let handles = (0..8)
            .map(|value| {
                let dir = dir.clone();
                let start = Arc::clone(&start);
                std::thread::spawn(move || {
                    let mut profile = test_profile("shared", &format!("Profile {value}"));
                    profile.credentials.insert(
                        "payload".to_string(),
                        format!("{value}:{}", "x".repeat(64 * 1024)),
                    );
                    start.wait();
                    save_at(&dir, &profile)
                })
            })
            .collect::<Vec<_>>();

        start.wait();
        for handle in handles {
            handle.join().unwrap().unwrap();
        }

        let profile = load_path(&profile_path_in(&dir, "shared")).unwrap();
        let value = profile.label.strip_prefix("Profile ").unwrap();
        assert!(profile.credentials["payload"].starts_with(&format!("{value}:")));
        let mut entries = std::fs::read_dir(&dir)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().to_string())
            .collect::<Vec<_>>();
        entries.sort();
        assert_eq!(entries, [".shared.json.lock", "shared.json"]);

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(profile_path_in(&dir, "shared"))
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777,
                0o600
            );
        }
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn concurrent_profile_updates_preserve_every_mutation() {
        use std::sync::{Arc, Barrier};

        let dir = test_dir("concurrent-update");
        let mut profile = test_profile("shared", "Shared");
        profile
            .credentials
            .insert("count".to_string(), "0".to_string());
        save_at(&dir, &profile).unwrap();

        let start = Arc::new(Barrier::new(9));
        let handles = (0..8)
            .map(|_| {
                let dir = dir.clone();
                let start = Arc::clone(&start);
                std::thread::spawn(move || {
                    start.wait();
                    update_at(&dir, "shared", |profile| {
                        let count = profile.credentials["count"].parse::<u8>().unwrap();
                        std::thread::sleep(std::time::Duration::from_millis(5));
                        profile
                            .credentials
                            .insert("count".to_string(), (count + 1).to_string());
                        Ok(())
                    })
                })
            })
            .collect::<Vec<_>>();

        start.wait();
        for handle in handles {
            assert_eq!(handle.join().unwrap().unwrap(), Some(()));
        }

        let profile = load_path(&profile_path_in(&dir, "shared")).unwrap();
        assert_eq!(profile.credentials["count"], "8");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn profile_lock_can_cover_file_and_followup_updates() {
        use std::sync::mpsc;
        use std::time::Duration;

        let dir = test_dir("lock-followup");
        save_at(&dir, &test_profile("shared", "Before")).unwrap();
        let first = LockedProfileFile::acquire_at(&dir, "shared").unwrap();
        first.delete().unwrap();

        let (ready_tx, ready_rx) = mpsc::channel();
        let (acquired_tx, acquired_rx) = mpsc::channel();
        let writer_dir = dir.clone();
        let writer = std::thread::spawn(move || {
            ready_tx.send(()).unwrap();
            let locked = LockedProfileFile::acquire_at(&writer_dir, "shared").unwrap();
            acquired_tx.send(()).unwrap();
            locked.save(&test_profile("shared", "After")).unwrap();
        });

        ready_rx.recv_timeout(Duration::from_secs(1)).unwrap();
        assert!(acquired_rx.recv_timeout(Duration::from_millis(50)).is_err());
        drop(first);
        acquired_rx.recv_timeout(Duration::from_secs(1)).unwrap();
        writer.join().unwrap();

        assert_eq!(
            load_path(&profile_path_in(&dir, "shared")).unwrap().label,
            "After"
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn failed_profile_update_leaves_existing_file_unchanged() {
        let dir = test_dir("failed-update");
        let profile = test_profile("shared", "Before");
        save_at(&dir, &profile).unwrap();

        let result: anyhow::Result<Option<()>> = update_at(&dir, "shared", |profile| {
            profile.label = "After".to_string();
            bail!("reject update")
        });

        assert!(result.is_err());
        assert_eq!(
            load_path(&profile_path_in(&dir, "shared")).unwrap().label,
            "Before"
        );
        std::fs::remove_dir_all(dir).unwrap();
    }
}
