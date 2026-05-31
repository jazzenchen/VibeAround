//! Shared profile runtime.
//!
//! Profiles are user-managed provider credentials plus the catalog metadata
//! needed to render env vars and profile-local config files for coding CLIs.
//! Desktop owns the UI and terminal window launch; core owns the reusable
//! schema/catalog/rendering path so IM-started agents can use the same
//! profiles.

mod bridge_launch;
pub mod catalog;
pub mod codex_metadata;
pub mod connections;
pub mod google_oauth;
pub mod headers;
mod pi_launch;
pub mod render;
pub mod runtime;
pub mod schema;

pub use schema::{AuthMode, ProfileDef};

use crate::config;

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
        if let Err(error) = schema::save(&profile) {
            tracing::warn!(
                "[profiles] failed to persist legacy profile migration for '{}': {}",
                profile.id,
                error
            );
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
            use_settings_proxy: false,
            provider_settings: ProviderSettings::default(),
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
            use_settings_proxy: false,
            provider_settings: ProviderSettings::default(),
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
            use_settings_proxy: false,
            provider_settings: ProviderSettings::default(),
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
            use_settings_proxy: false,
            provider_settings: ProviderSettings::default(),
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
            use_settings_proxy: false,
            provider_settings: ProviderSettings::default(),
        };

        let profile = normalize_legacy_profile(profile);

        assert_eq!(profile.auth_mode, AuthMode::GoogleOauth);
    }
}
