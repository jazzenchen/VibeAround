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

use anyhow::{anyhow, bail, Context};
use nanoid::nanoid;
use serde::{Deserialize, Serialize};

use super::catalog::{self, ContentCapabilities, EndpointDef, ModelDef};
use crate::{agent_state, auth, config};

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
    /// Which CLI launch targets this credential is good for. Internally these
    /// are still keyed by the API/config shape each target needs.
    pub api_types: Vec<String>,
    /// Free-form credentials — `api_key` is the only field used by v1
    /// catalog entries, but we keep the bag generic so future plugins can
    /// declare custom field names without a schema migration.
    #[serde(default)]
    pub credentials: BTreeMap<String, String>,
    /// Optional per-api-type overrides for `base_url` / `model`. Empty ==
    /// inherit catalog defaults.
    #[serde(default)]
    pub overrides: BTreeMap<String, ApiTypeOverrides>,
    /// Materialized editable API configs cloned from the provider catalog.
    /// Legacy `api_types` + `overrides` remain the compatibility source for
    /// older profile files and are projected into this map on load/save.
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
    profiles_dir().join(format!("{id}.json"))
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
    if profile.api_types.is_empty() {
        bail!("profile must declare at least one api kind");
    }
    Ok(())
}

pub fn enabled_api_types(profile: &ProfileDef) -> Vec<String> {
    if profile.api_configs.is_empty() {
        return profile.api_types.clone();
    }
    profile
        .api_configs
        .iter()
        .filter_map(|(api_type, config)| config.enabled.then(|| api_type.clone()))
        .collect()
}

pub fn api_config_for(
    profile: &ProfileDef,
    provider: &catalog::ProviderCatalog,
    api_type: &str,
) -> Option<ProfileApiConfig> {
    profile
        .api_configs
        .get(api_type)
        .cloned()
        .or_else(|| legacy_api_config(profile, provider, api_type))
}

pub fn hydrate_api_configs(profile: &mut ProfileDef) {
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
        headers: default_headers(endpoint),
        models: default_models(
            endpoint,
            overrides.model.as_deref(),
            overrides.capabilities.clone(),
        ),
    })
}

fn default_headers(endpoint: &EndpointDef) -> Vec<ProfileHeaderConfig> {
    let mut out = Vec::new();
    for (name, value) in &endpoint.headers {
        out.push(ProfileHeaderConfig {
            name: name.clone(),
            value: value.clone(),
            enabled: true,
            locked: true,
        });
    }
    if let Some((name, value)) = default_auth_header(endpoint) {
        if !out
            .iter()
            .any(|header| header.name.eq_ignore_ascii_case(&name))
        {
            out.push(ProfileHeaderConfig {
                name,
                value,
                enabled: true,
                locked: true,
            });
        }
    }
    out
}

fn default_auth_header(endpoint: &EndpointDef) -> Option<(String, String)> {
    match endpoint.api_type.as_str() {
        "openai-chat" | "openai-responses" => {
            Some(("Authorization".to_string(), "Bearer $apiKey".to_string()))
        }
        "anthropic" if endpoint.auth_header => {
            Some(("Authorization".to_string(), "Bearer $apiKey".to_string()))
        }
        "anthropic" => Some(("x-api-key".to_string(), "$apiKey".to_string())),
        "gemini" => Some(("x-goog-api-key".to_string(), "$apiKey".to_string())),
        _ => None,
    }
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
    let mut profile: ProfileDef =
        serde_json::from_str(&body).with_context(|| format!("parse {:?}", path))?;
    hydrate_api_configs(&mut profile);
    Ok(profile)
}

pub fn save(profile: &ProfileDef) -> anyhow::Result<()> {
    let mut profile = profile.clone();
    hydrate_api_configs(&mut profile);
    validate(&profile)?;
    let dir = profiles_dir();
    std::fs::create_dir_all(&dir).with_context(|| format!("create {:?}", dir))?;
    // Lock down the profiles dir on Unix so other local users can't
    // enumerate or read API keys.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700));
    }

    let target = profile_path(&profile.id);
    let tmp = dir.join(format!(".{}.tmp.{}.json", profile.id, std::process::id()));
    let body = serde_json::to_string_pretty(&profile).context("serialize profile")?;
    std::fs::write(&tmp, body).with_context(|| format!("write {:?}", tmp))?;
    auth::set_owner_only(&tmp).ok();
    std::fs::rename(&tmp, &target).with_context(|| format!("rename to {:?}", target))?;
    Ok(())
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

pub fn delete(id: &str) -> anyhow::Result<()> {
    if !is_valid_id(id) {
        return Err(anyhow!("invalid profile id '{}'", id));
    }
    let path = profile_path(id);
    if !path.exists() {
        return Ok(());
    }
    std::fs::remove_file(&path).with_context(|| format!("remove {:?}", path))?;
    // Best-effort: also drop the per-profile state dir (rendered settings
    // files, future agent session caches). If the user re-creates a profile
    // with the same id later, we want a clean slate.
    let state_dir = config::data_dir().join("profile-state").join(id);
    let _ = std::fs::remove_dir_all(&state_dir);
    Ok(())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

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

        hydrate_api_configs(&mut profile);

        let anthropic = profile.api_configs.get("anthropic").unwrap();
        assert!(anthropic.enabled);
        assert_eq!(
            anthropic.base_url.as_deref(),
            Some("https://token.sensenova.cn")
        );
        assert_eq!(anthropic.model.as_deref(), Some("sensenova-6.7-flash-lite"));
        assert_eq!(anthropic.headers[0].name, "x-api-key");
        assert_eq!(anthropic.headers[0].value, "$apiKey");
        assert!(anthropic.headers[0].locked);
        assert!(anthropic.models[0].custom);

        let chat = profile.api_configs.get("openai-chat").unwrap();
        assert_eq!(
            chat.base_url.as_deref(),
            Some("https://token.sensenova.cn/v1")
        );
        assert_eq!(chat.headers[0].name, "Authorization");
        assert_eq!(chat.headers[0].value, "Bearer $apiKey");
    }
}
