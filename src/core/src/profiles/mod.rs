//! Shared profile runtime.
//!
//! Profiles are user-managed provider credentials plus the catalog metadata
//! needed to render env vars and profile-local config files for coding CLIs.
//! Desktop owns the UI and terminal window launch; core owns the reusable
//! schema/catalog/rendering path so IM-started agents can use the same
//! profiles.

mod bridge_launch;
pub mod bridge_url;
pub mod catalog;
pub mod codex_metadata;
pub mod connections;
pub mod endpoint_url;
pub mod google_oauth;
pub mod headers;
mod pi_launch;
pub mod render;
pub mod runtime;
pub mod schema;

pub use schema::{AuthMode, ProfileDef};

use std::collections::HashSet;

use crate::{agent_state, config};

const DASHSCOPE_PROVIDER_ID: &str = "dashscope";
const DASHSCOPE_LABEL: &str = "Alibaba DashScope";
const LEGACY_QWEN_PROVIDER_ID: &str = "qwen";
const LEGACY_QWEN_LABEL: &str = "Qwen / DashScope";
const MOONSHOT_PROVIDER_ID: &str = "moonshot";
const LEGACY_KIMI_PROVIDER_ID: &str = "kimi";
const KIMI_CODING_ENDPOINT_ID: &str = "kimi-coding";
const KIMI_CODING_LEGACY_BASE_URL: &str = "https://api.kimi.com/coding";
const GEMINI_PROVIDER_ID: &str = "gemini";
const GEMINI_API_ENDPOINT_ID: &str = "gemini-api";
const LEGACY_GEMINI_OPENAI_ENDPOINT_ID: &str = "openai-compatible";

#[derive(Debug, thiserror::Error)]
pub enum ProfileStoreError {
    #[error("{0}")]
    Invalid(String),
    #[error("{0}")]
    Storage(String),
}

pub fn load_profile(id: &str) -> Option<ProfileDef> {
    schema::load(id).map(normalize_legacy_profile_and_persist)
}

pub fn save_profile(profile: &ProfileDef) -> Result<(), ProfileStoreError> {
    validate_profile(profile)?;
    schema::save(profile).map_err(|error| ProfileStoreError::Storage(error.to_string()))?;
    config::mutate_settings_json(|root| ensure_profile_order_contains(root, &profile.id))
        .map_err(ProfileStoreError::Storage)
}

pub fn delete_profile(id: &str) -> Result<(), ProfileStoreError> {
    if !schema::is_valid_id(id) {
        return Err(ProfileStoreError::Invalid(format!(
            "invalid profile id '{id}'"
        )));
    }
    schema::delete(id).map_err(|error| ProfileStoreError::Storage(error.to_string()))?;
    config::mutate_settings_json(|root| clear_profile_references(root, id))
        .map_err(ProfileStoreError::Storage)
}

pub fn reorder_profiles(requested_ids: &[String]) -> Result<(), ProfileStoreError> {
    let available_ids = schema::list()
        .into_iter()
        .map(|profile| profile.id)
        .collect::<Vec<_>>();
    config::mutate_settings_json(|root| {
        reorder_profiles_in_settings(root, requested_ids, &available_ids)
    })
    .map_err(ProfileStoreError::Storage)
}

pub fn set_profile_connection(
    profile_id: &str,
    agent_id: &str,
    preference: agent_state::ProfileConnectionPreference,
) -> Result<bool, ProfileStoreError> {
    let mut invalid = None;
    let updated = schema::update(profile_id, |profile| {
        *profile = normalize_legacy_profile(profile.clone());
        let preference =
            connections::sanitize_profile_connection_preference(profile, agent_id, preference)
                .map_err(|error| {
                    invalid = Some(error.clone());
                    anyhow::Error::msg(error)
                })?;
        schema::set_connection(profile, agent_id, preference);
        Ok(())
    });
    if let Some(error) = invalid {
        return Err(ProfileStoreError::Invalid(error));
    }
    updated
        .map(|result| result.is_some())
        .map_err(|error| ProfileStoreError::Storage(error.to_string()))
}

fn validate_profile(profile: &ProfileDef) -> Result<(), ProfileStoreError> {
    schema::validate(profile).map_err(|error| ProfileStoreError::Invalid(error.to_string()))?;
    let provider = catalog::get(&profile.provider).ok_or_else(|| {
        ProfileStoreError::Invalid(format!("unknown provider '{}'", profile.provider))
    })?;
    for api_type in &profile.api_types {
        let endpoint_id = profile
            .overrides
            .get(api_type)
            .and_then(|overrides| overrides.endpoint_id.as_deref());
        if catalog::find_endpoint(provider, api_type, endpoint_id).is_none() {
            let suffix = endpoint_id
                .map(|id| format!(" endpoint_id '{id}'"))
                .unwrap_or_default();
            return Err(ProfileStoreError::Invalid(format!(
                "provider '{}' does not support api kind '{}'{}",
                profile.provider, api_type, suffix
            )));
        }
    }
    Ok(())
}

fn ensure_profile_order_contains(
    root: &mut serde_json::Value,
    profile_id: &str,
) -> Result<(), String> {
    let obj = root
        .as_object_mut()
        .ok_or_else(|| "settings.json root must be a JSON object".to_string())?;
    let order = obj
        .entry("profile_order".to_string())
        .or_insert_with(|| serde_json::json!([]));
    if !order.is_array() {
        *order = serde_json::json!([]);
    }
    let order = order
        .as_array_mut()
        .ok_or_else(|| "settings.json profile_order must be an array".to_string())?;
    if !order.iter().any(|id| id.as_str() == Some(profile_id)) {
        order.push(serde_json::Value::String(profile_id.to_string()));
    }
    Ok(())
}

fn reorder_profiles_in_settings(
    root: &mut serde_json::Value,
    requested_ids: &[String],
    available_ids: &[String],
) -> Result<(), String> {
    let obj = root
        .as_object_mut()
        .ok_or_else(|| "settings.json root must be a JSON object".to_string())?;
    let latest_order = obj
        .get("profile_order")
        .and_then(|value| value.as_array())
        .into_iter()
        .flatten()
        .filter_map(|value| value.as_str())
        .map(str::trim)
        .filter(|id| !id.is_empty())
        .map(ToOwned::to_owned)
        .collect::<Vec<_>>();
    let available = available_ids
        .iter()
        .map(String::as_str)
        .collect::<HashSet<_>>();
    let mut seen = HashSet::new();
    let mut ordered_ids = Vec::new();

    for id in requested_ids {
        let id = id.trim();
        if available.contains(id) && seen.insert(id.to_string()) {
            ordered_ids.push(id.to_string());
        }
    }
    for id in latest_order
        .iter()
        .filter(|id| available.contains(id.as_str()))
    {
        if seen.insert(id.clone()) {
            ordered_ids.push(id.clone());
        }
    }
    for id in available_ids {
        if seen.insert(id.clone()) {
            ordered_ids.push(id.clone());
        }
    }

    obj.insert(
        "profile_order".to_string(),
        serde_json::Value::Array(
            ordered_ids
                .into_iter()
                .map(serde_json::Value::String)
                .collect(),
        ),
    );
    Ok(())
}

fn clear_profile_references(root: &mut serde_json::Value, profile_id: &str) -> Result<(), String> {
    {
        let obj = root
            .as_object_mut()
            .ok_or_else(|| "settings.json root must be a JSON object".to_string())?;
        if let Some(order) = obj
            .get_mut("profile_order")
            .and_then(|value| value.as_array_mut())
        {
            order.retain(|value| value.as_str() != Some(profile_id));
            if order.is_empty() {
                obj.remove("profile_order");
            }
        }
    }
    agent_state::remove_profile_references_from_settings(root, profile_id)
}

pub fn normalize_legacy_profile(mut profile: ProfileDef) -> ProfileDef {
    normalize_legacy_dashscope_profile(&mut profile);
    normalize_legacy_kimi_profile(&mut profile);
    normalize_legacy_gemini_profile(&mut profile);

    if profile.provider == "azure" && profile.api_types.iter().any(|t| t == "openai-chat") {
        let chat_overrides = profile.overrides.remove("openai-chat");
        profile.api_types.retain(|t| t != "openai-chat");
        if !profile.api_types.iter().any(|t| t == "openai-responses") {
            profile.api_types.push("openai-responses".to_string());
            if let Some(overrides) = chat_overrides {
                profile
                    .overrides
                    .entry("openai-responses".to_string())
                    .or_insert(overrides);
            }
        }
    }
    profile
}

pub fn normalize_legacy_profile_and_persist(profile: ProfileDef) -> ProfileDef {
    let should_persist_profile_migration = needs_dashscope_profile_persist(&profile)
        || needs_kimi_profile_persist(&profile)
        || needs_gemini_profile_persist(&profile);
    let profile = normalize_legacy_profile(profile);

    // TODO(0.6.x): remove these legacy provider migrations once old profile
    // files have had a release window to be rewritten on load.
    if should_persist_profile_migration {
        let id = profile.id.clone();
        match schema::update(&id, |current| {
            *current = normalize_legacy_profile(current.clone());
            Ok(current.clone())
        }) {
            Ok(Some(current)) => return current,
            Ok(None) => {}
            Err(error) => {
                tracing::warn!(
                    "[profiles] failed to persist legacy profile migration for '{}': {}",
                    profile.id,
                    error
                );
            }
        }
    }

    profile
}

/// List saved profiles using the user's Launch/Tray ordering.
///
/// `schema::list()` intentionally has a stable fallback sort by label for
/// raw storage reads. Product surfaces should call this helper instead so
/// the `settings.json.profile_order` preference is respected consistently.
pub fn ordered_profiles() -> Vec<ProfileDef> {
    let mut remaining: Vec<_> = schema::list()
        .into_iter()
        .map(normalize_legacy_profile_and_persist)
        .collect();
    let mut out = Vec::new();

    for id in read_profile_order() {
        if let Some(index) = remaining.iter().position(|profile| profile.id == id) {
            out.push(remaining.remove(index));
        }
    }

    out.extend(remaining);
    out
}

fn read_profile_order() -> Vec<String> {
    let path = config::data_dir().join("settings.json");
    std::fs::read_to_string(path)
        .ok()
        .and_then(|data| serde_json::from_str::<serde_json::Value>(&data).ok())
        .and_then(|root| {
            root.get("profile_order")
                .and_then(|value| value.as_array())
                .map(|items| {
                    items
                        .iter()
                        .filter_map(|item| item.as_str())
                        .map(str::trim)
                        .filter(|id| !id.is_empty())
                        .map(ToOwned::to_owned)
                        .collect()
                })
        })
        .unwrap_or_default()
}

fn normalize_legacy_dashscope_profile(profile: &mut ProfileDef) {
    if profile.provider == LEGACY_QWEN_PROVIDER_ID {
        profile.provider = DASHSCOPE_PROVIDER_ID.to_string();
    }
    if profile.provider != DASHSCOPE_PROVIDER_ID {
        return;
    }

    if profile.label == LEGACY_QWEN_LABEL {
        profile.label = DASHSCOPE_LABEL.to_string();
    }

    for overrides in profile.overrides.values_mut() {
        let Some(endpoint_id) = overrides.endpoint_id.as_deref() else {
            continue;
        };
        let next = match endpoint_id {
            "coding-global" => "coding-plan",
            "coding-cn" => "coding-plan-cn",
            "standard-global" => "token-plan",
            "standard-cn" => "token-plan-cn",
            _ => continue,
        };
        overrides.endpoint_id = Some(next.to_string());
    }
}

fn normalize_legacy_kimi_profile(profile: &mut ProfileDef) {
    if profile.provider != LEGACY_KIMI_PROVIDER_ID {
        return;
    }

    profile.provider = MOONSHOT_PROVIDER_ID.to_string();
    if profile
        .api_types
        .iter()
        .any(|api_type| api_type == "anthropic")
    {
        let overrides = profile
            .overrides
            .entry("anthropic".to_string())
            .or_default();
        if matches!(overrides.endpoint_id.as_deref(), None | Some("anthropic")) {
            overrides.endpoint_id = Some(KIMI_CODING_ENDPOINT_ID.to_string());
        }
        if overrides
            .base_url
            .as_deref()
            .map(|base_url| base_url.trim_end_matches('/'))
            == Some(KIMI_CODING_LEGACY_BASE_URL)
        {
            overrides.base_url = None;
        }
    }
}

fn normalize_legacy_gemini_profile(profile: &mut ProfileDef) {
    if profile.provider != GEMINI_PROVIDER_ID {
        return;
    }

    if profile.auth_mode == schema::AuthMode::OauthViaCli {
        profile.auth_mode = schema::AuthMode::GoogleOauth;
    }

    for overrides in profile.overrides.values_mut() {
        if overrides.endpoint_id.as_deref() == Some(LEGACY_GEMINI_OPENAI_ENDPOINT_ID) {
            overrides.endpoint_id = Some(GEMINI_API_ENDPOINT_ID.to_string());
        }
    }
}

fn needs_dashscope_profile_persist(profile: &ProfileDef) -> bool {
    profile.provider == LEGACY_QWEN_PROVIDER_ID
        || (profile.provider == DASHSCOPE_PROVIDER_ID
            && (profile.label == LEGACY_QWEN_LABEL
                || profile.overrides.values().any(|overrides| {
                    matches!(
                        overrides.endpoint_id.as_deref(),
                        Some("coding-global" | "coding-cn" | "standard-global" | "standard-cn")
                    )
                })))
}

fn needs_kimi_profile_persist(profile: &ProfileDef) -> bool {
    profile.provider == LEGACY_KIMI_PROVIDER_ID
}

fn needs_gemini_profile_persist(profile: &ProfileDef) -> bool {
    profile.provider == GEMINI_PROVIDER_ID
        && (profile.auth_mode == schema::AuthMode::OauthViaCli
            || profile.overrides.values().any(|overrides| {
                overrides.endpoint_id.as_deref() == Some(LEGACY_GEMINI_OPENAI_ENDPOINT_ID)
            }))
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;
    use schema::{ApiTypeOverrides, AuthMode, ProviderSettings};

    #[test]
    fn normalizes_legacy_qwen_provider_and_endpoint_ids() {
        let mut overrides = BTreeMap::new();
        overrides.insert(
            "openai-chat".to_string(),
            ApiTypeOverrides {
                endpoint_id: Some("standard-cn".to_string()),
                base_url: None,
                model: None,
                reasoning_effort: None,
                capabilities: None,
            },
        );
        let profile = ProfileDef {
            id: "qwen-old".to_string(),
            label: LEGACY_QWEN_LABEL.to_string(),
            provider: "qwen".to_string(),
            auth_mode: AuthMode::ApiKey,
            api_types: vec!["openai-chat".to_string()],
            credentials: BTreeMap::new(),
            overrides,
            api_configs: Default::default(),
            use_settings_proxy: false,
            provider_settings: ProviderSettings::default(),
            connections: Default::default(),
        };

        let profile = normalize_legacy_profile(profile);

        assert_eq!(profile.provider, "dashscope");
        assert_eq!(profile.label, DASHSCOPE_LABEL);
        assert_eq!(
            profile
                .overrides
                .get("openai-chat")
                .and_then(|overrides| overrides.endpoint_id.as_deref()),
            Some("token-plan-cn")
        );
    }

    #[test]
    fn preserves_custom_legacy_qwen_profile_label() {
        let profile = ProfileDef {
            id: "qwen-custom".to_string(),
            label: "Work DashScope".to_string(),
            provider: "qwen".to_string(),
            auth_mode: AuthMode::ApiKey,
            api_types: vec!["openai-chat".to_string()],
            credentials: BTreeMap::new(),
            overrides: BTreeMap::new(),
            api_configs: Default::default(),
            use_settings_proxy: false,
            provider_settings: ProviderSettings::default(),
            connections: Default::default(),
        };

        let profile = normalize_legacy_profile(profile);

        assert_eq!(profile.provider, "dashscope");
        assert_eq!(profile.label, "Work DashScope");
    }

    #[test]
    fn normalizes_legacy_kimi_profile_to_moonshot_kimi_coding_endpoint() {
        let mut overrides = BTreeMap::new();
        overrides.insert(
            "anthropic".to_string(),
            ApiTypeOverrides {
                endpoint_id: None,
                base_url: Some("https://api.kimi.com/coding/".to_string()),
                model: Some("kimi-for-coding".to_string()),
                reasoning_effort: None,
                capabilities: None,
            },
        );
        let profile = ProfileDef {
            id: "kimi-old".to_string(),
            label: "Kimi Coding".to_string(),
            provider: "kimi".to_string(),
            auth_mode: AuthMode::ApiKey,
            api_types: vec!["anthropic".to_string()],
            credentials: BTreeMap::new(),
            overrides,
            api_configs: Default::default(),
            use_settings_proxy: false,
            provider_settings: ProviderSettings::default(),
            connections: Default::default(),
        };

        let profile = normalize_legacy_profile(profile);

        assert_eq!(profile.provider, "moonshot");
        let overrides = profile
            .overrides
            .get("anthropic")
            .expect("anthropic overrides");
        assert_eq!(overrides.endpoint_id.as_deref(), Some("kimi-coding"));
        assert_eq!(overrides.base_url, None);
        assert_eq!(overrides.model.as_deref(), Some("kimi-for-coding"));
    }

    #[test]
    fn normalizes_legacy_gemini_openai_endpoint_id() {
        let mut overrides = BTreeMap::new();
        overrides.insert(
            "openai-chat".to_string(),
            ApiTypeOverrides {
                endpoint_id: Some("openai-compatible".to_string()),
                base_url: None,
                model: Some("gemini-3.1-pro".to_string()),
                reasoning_effort: None,
                capabilities: None,
            },
        );
        let profile = ProfileDef {
            id: "gemini-old".to_string(),
            label: "Gemini".to_string(),
            provider: "gemini".to_string(),
            auth_mode: AuthMode::ApiKey,
            api_types: vec!["openai-chat".to_string()],
            credentials: BTreeMap::new(),
            overrides,
            api_configs: Default::default(),
            use_settings_proxy: false,
            provider_settings: ProviderSettings::default(),
            connections: Default::default(),
        };

        let profile = normalize_legacy_profile(profile);

        assert_eq!(
            profile
                .overrides
                .get("openai-chat")
                .and_then(|overrides| overrides.endpoint_id.as_deref()),
            Some("gemini-api")
        );
    }

    #[test]
    fn normalizes_legacy_gemini_oauth_mode() {
        let profile = ProfileDef {
            id: "gemini-google-account".to_string(),
            label: "Gemini Google".to_string(),
            provider: "gemini".to_string(),
            auth_mode: AuthMode::OauthViaCli,
            api_types: vec!["gemini".to_string()],
            credentials: BTreeMap::new(),
            overrides: BTreeMap::new(),
            api_configs: Default::default(),
            use_settings_proxy: false,
            provider_settings: ProviderSettings::default(),
            connections: Default::default(),
        };

        let profile = normalize_legacy_profile(profile);

        assert_eq!(profile.auth_mode, AuthMode::GoogleOauth);
    }

    #[test]
    fn adding_profile_order_uses_latest_settings_value() {
        let mut settings = serde_json::json!({
            "profile_order": ["existing"],
            "workspaces": ["/tmp/work"]
        });

        ensure_profile_order_contains(&mut settings, "new-profile").unwrap();

        assert_eq!(
            settings["profile_order"],
            serde_json::json!(["existing", "new-profile"])
        );
        assert_eq!(settings["workspaces"], serde_json::json!(["/tmp/work"]));
    }

    #[test]
    fn reorder_preserves_latest_and_new_profile_ids() {
        let mut settings = serde_json::json!({
            "profile_order": ["first", "concurrent"],
            "workspaces": ["/tmp/work"]
        });
        let requested = vec![
            "second".to_string(),
            "second".to_string(),
            "unknown".to_string(),
        ];
        let available = vec![
            "first".to_string(),
            "second".to_string(),
            "concurrent".to_string(),
            "new-on-disk".to_string(),
        ];

        reorder_profiles_in_settings(&mut settings, &requested, &available).unwrap();

        assert_eq!(
            settings["profile_order"],
            serde_json::json!(["second", "first", "concurrent", "new-on-disk"])
        );
        assert_eq!(settings["workspaces"], serde_json::json!(["/tmp/work"]));
    }

    #[test]
    fn deleting_profile_clears_order_and_launcher_references_together() {
        let mut settings = serde_json::json!({
            "profile_order": ["removed", "kept"],
            "launcher": {
                "default_profile_id": "removed",
                "terminal": "terminal",
                "agents": {
                    "codex": { "profile_id": "removed" },
                    "claude": { "profile_id": "kept" }
                }
            }
        });

        clear_profile_references(&mut settings, "removed").unwrap();

        assert_eq!(settings["profile_order"], serde_json::json!(["kept"]));
        assert!(settings["launcher"].get("default_profile_id").is_none());
        assert!(settings["launcher"]["agents"].get("codex").is_none());
        assert_eq!(settings["launcher"]["terminal"], "terminal");
    }
}
