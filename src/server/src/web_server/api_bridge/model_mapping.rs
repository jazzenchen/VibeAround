use common::agent_state;
use common::profiles::catalog::ContentCapabilities;
use common::profiles::{
    catalog, connections,
    schema::{self, ProfileDef},
};

#[derive(Debug, Clone)]
pub(super) struct BridgeModelMapping {
    pub(super) upstream_model: String,
    pub(super) agent_model: String,
    pub(super) capabilities: ContentCapabilities,
}

pub(super) fn bridge_route_preference(
    profile: &ProfileDef,
    route_scope: Option<&str>,
    client_api_type: &str,
    target_api_type: &str,
) -> Option<agent_state::ProfileBridgePreference> {
    let agent_id = agent_id_from_scope(route_scope?, client_api_type)?;
    let connections = connections::merged_profile_connections();
    let preference = connections.get(&profile.id)?.get(agent_id)?;
    let bridge = preference.bridge.get(client_api_type)?;
    if !bridge.enabled {
        return None;
    }
    let configured_target = bridge.target_api_type.as_deref().unwrap_or(target_api_type);
    if configured_target != target_api_type {
        return None;
    }
    Some(bridge.clone())
}

pub(super) fn bridge_model_mapping(
    profile: &ProfileDef,
    bridge: Option<&agent_state::ProfileBridgePreference>,
    target_api_type: &str,
    requested_agent_model: Option<&str>,
) -> Option<BridgeModelMapping> {
    let bridge = bridge?;
    let routes = connections::bridge_model_routes(profile, Some(bridge), target_api_type);
    if let Some(requested_agent_model) = clean_model_id(requested_agent_model) {
        if requested_agent_model.eq_ignore_ascii_case("default") {
            if let Some(route) = routes.first() {
                return Some(mapping_from_route(route));
            }
        }
        if let Some(route) = routes
            .iter()
            .find(|route| agent_model_matches(&route.agent_model, &requested_agent_model))
        {
            return Some(mapping_from_route(route));
        }
        if is_claude_short_model_alias(&requested_agent_model) && routes_have_claude_models(&routes)
        {
            if let Some(route) = routes.first() {
                return Some(mapping_from_route(route));
            }
        }
        let upstream_model = canonical_model(profile, target_api_type, &requested_agent_model)
            .unwrap_or_else(|| requested_agent_model.clone());
        return Some(BridgeModelMapping {
            upstream_model,
            agent_model: requested_agent_model,
            capabilities: ContentCapabilities::default(),
        });
    }

    let route = routes.into_iter().next()?;
    Some(BridgeModelMapping {
        upstream_model: route.upstream_model,
        agent_model: route.agent_model,
        capabilities: route.capabilities,
    })
}

fn mapping_from_route(route: &connections::ProfileBridgeModelRoute) -> BridgeModelMapping {
    BridgeModelMapping {
        upstream_model: route.upstream_model.clone(),
        agent_model: route.agent_model.clone(),
        capabilities: route.capabilities.clone(),
    }
}

fn agent_id_from_scope(scope: &str, client_api_type: &str) -> Option<&'static str> {
    for (scope_agent_id, preference_agent_id) in route_scope_agent_ids() {
        let prefix = format!("{scope_agent_id}-");
        if scope.strip_prefix(&prefix) == Some(client_api_type) {
            return Some(preference_agent_id);
        }
    }
    None
}

fn route_scope_agent_ids() -> &'static [(&'static str, &'static str)] {
    &[
        ("claude-desktop", "claude-desktop"),
        ("codex-desktop", "codex-desktop"),
        ("claude", "claude"),
        ("codex", "codex"),
        ("gemini", "gemini"),
        ("opencode", "opencode"),
        ("pi", "pi"),
    ]
}

fn canonical_model(profile: &ProfileDef, target_api_type: &str, model: &str) -> Option<String> {
    let provider = catalog::get(&profile.provider)?;
    if let Some(model_id) = schema::api_config_for(profile, target_api_type)
        .filter(|config| config.enabled)
        .and_then(|config| canonical_api_config_model_id(&config.models, model))
    {
        return Some(model_id);
    }
    let endpoint_id =
        schema::api_config_for(profile, target_api_type).and_then(|config| config.endpoint_id);
    let endpoint = catalog::find_endpoint(provider, target_api_type, endpoint_id.as_deref())?;
    catalog::canonical_model_id(endpoint, model)
}

fn canonical_api_config_model_id(
    models: &[schema::ProfileModelConfig],
    model_id: &str,
) -> Option<String> {
    let model_id = model_id.trim();
    if model_id.is_empty() {
        return None;
    }
    if let Some(base_model) = catalog::strip_bracket_suffix(model_id) {
        if let Some(model) = models
            .iter()
            .filter(|model| model.enabled)
            .find(|model| model.id == base_model)
        {
            return Some(model.id.clone());
        }
    }
    models
        .iter()
        .filter(|model| model.enabled)
        .find(|model| model.id == model_id)
        .map(|model| model.id.clone())
}

fn agent_model_matches(configured: &str, requested: &str) -> bool {
    model_id_matches(configured, requested)
        || catalog::strip_bracket_suffix(configured)
            .map(|base| model_id_matches(base, requested))
            .unwrap_or(false)
        || catalog::strip_bracket_suffix(requested)
            .map(|base| model_id_matches(base, configured))
            .unwrap_or(false)
        || claude_short_model_alias_matches(configured, requested)
}

fn model_id_matches(left: &str, right: &str) -> bool {
    left == right || left.eq_ignore_ascii_case(right)
}

fn claude_short_model_alias_matches(configured: &str, requested: &str) -> bool {
    let Some(requested_family) = claude_model_family(requested) else {
        return false;
    };
    let Some(configured_family) = claude_model_family(configured) else {
        return false;
    };
    if requested_family != configured_family {
        return false;
    }
    match (bracket_suffix(configured), bracket_suffix(requested)) {
        (_, None) => true,
        (Some(configured_suffix), Some(requested_suffix)) => {
            configured_suffix.eq_ignore_ascii_case(requested_suffix)
        }
        (None, Some(_)) => true,
    }
}

fn is_claude_short_model_alias(model: &str) -> bool {
    claude_model_family(model).is_some()
}

fn routes_have_claude_models(routes: &[connections::ProfileBridgeModelRoute]) -> bool {
    routes
        .iter()
        .any(|route| claude_model_family(&route.agent_model).is_some())
}

fn claude_model_family(model: &str) -> Option<&'static str> {
    let base = catalog::strip_bracket_suffix(model).unwrap_or(model);
    let normalized = base.trim().to_ascii_lowercase();
    if normalized == "opus" || normalized.starts_with("opus-") || normalized.contains("-opus-") {
        return Some("opus");
    }
    if normalized == "sonnet"
        || normalized.starts_with("sonnet-")
        || normalized.contains("-sonnet-")
    {
        return Some("sonnet");
    }
    if normalized == "haiku" || normalized.starts_with("haiku-") || normalized.contains("-haiku-") {
        return Some("haiku");
    }
    None
}

fn bracket_suffix(model: &str) -> Option<&str> {
    let (_, suffix) = model.trim().rsplit_once('[')?;
    suffix.strip_suffix(']').map(str::trim)
}

fn clean_model_id(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use common::profiles::schema::{AuthMode, ProfileApiConfig};

    use super::*;

    #[test]
    fn bridge_mapping_canonicalizes_gemini_alias_for_upstream() {
        let profile = ProfileDef {
            id: "gemini-test".to_string(),
            label: "Gemini Test".to_string(),
            provider: "gemini".to_string(),
            auth_mode: AuthMode::ApiKey,
            credentials: BTreeMap::new(),
            api_configs: [(
                "openai-chat".to_string(),
                ProfileApiConfig {
                    enabled: true,
                    endpoint_id: Some("gemini-api".to_string()),
                    model: Some("gemini-3.1-pro".to_string()),
                    ..Default::default()
                },
            )]
            .into_iter()
            .collect(),
            use_settings_proxy: false,
            provider_settings: Default::default(),
            connections: Default::default(),
        };
        let bridge = agent_state::ProfileBridgePreference {
            enabled: true,
            target_api_type: Some("openai-chat".to_string()),
            upstream_model: Some("gemini-3.1-pro".to_string()),
            fake_model_id: None,
            models: Vec::new(),
            headers: BTreeMap::new(),
        };

        let mapping = bridge_model_mapping(&profile, Some(&bridge), "openai-chat", None)
            .expect("mapping should resolve");

        assert_eq!(mapping.upstream_model, "gemini-3.1-pro-preview");
        assert_eq!(mapping.agent_model, "gemini-3.1-pro");
    }

    #[test]
    fn bridge_mapping_passes_unknown_requested_model_through() {
        let profile = ProfileDef {
            id: "custom-test".to_string(),
            label: "Custom Test".to_string(),
            provider: "custom".to_string(),
            auth_mode: AuthMode::ApiKey,
            credentials: BTreeMap::new(),
            api_configs: BTreeMap::new(),
            use_settings_proxy: false,
            provider_settings: Default::default(),
            connections: Default::default(),
        };
        let bridge = agent_state::ProfileBridgePreference {
            enabled: true,
            target_api_type: Some("openai-chat".to_string()),
            upstream_model: Some("gpt-4o".to_string()),
            fake_model_id: None,
            models: Vec::new(),
            headers: BTreeMap::new(),
        };

        let mapping = bridge_model_mapping(
            &profile,
            Some(&bridge),
            "openai-chat",
            Some("provider-new-model"),
        )
        .expect("mapping should resolve");

        assert_eq!(mapping.upstream_model, "provider-new-model");
        assert_eq!(mapping.agent_model, "provider-new-model");
    }

    #[test]
    fn bridge_mapping_matches_claude_context_suffix_alias() {
        let profile = ProfileDef {
            id: "deepseek-test".to_string(),
            label: "DeepSeek Test".to_string(),
            provider: "deepseek".to_string(),
            auth_mode: AuthMode::ApiKey,
            credentials: BTreeMap::new(),
            api_configs: BTreeMap::new(),
            use_settings_proxy: false,
            provider_settings: Default::default(),
            connections: Default::default(),
        };
        let bridge = agent_state::ProfileBridgePreference {
            enabled: true,
            target_api_type: Some("openai-chat".to_string()),
            upstream_model: Some("deepseek-v4-pro".to_string()),
            fake_model_id: Some("opus-4.7[1m]".to_string()),
            models: vec![agent_state::ProfileBridgeModelPreference {
                upstream_model: Some("deepseek-v4-pro".to_string()),
                fake_model_id: Some("opus-4.7[1m]".to_string()),
                capabilities: Default::default(),
            }],
            headers: BTreeMap::new(),
        };

        let mapping =
            bridge_model_mapping(&profile, Some(&bridge), "openai-chat", Some("opus-4.7"))
                .expect("mapping should resolve");

        assert_eq!(mapping.upstream_model, "deepseek-v4-pro");
        assert_eq!(mapping.agent_model, "opus-4.7[1m]");
    }

    #[test]
    fn bridge_mapping_matches_claude_short_model_alias() {
        let profile = deepseek_test_profile();
        let bridge = claude_deepseek_bridge();

        let mapping =
            bridge_model_mapping(&profile, Some(&bridge), "openai-chat", Some("opus[1m]"))
                .expect("mapping should resolve");

        assert_eq!(mapping.upstream_model, "deepseek-v4-pro");
        assert_eq!(mapping.agent_model, "claude-opus-4-8[1m]");
    }

    #[test]
    fn bridge_mapping_default_uses_first_configured_route() {
        let profile = deepseek_test_profile();
        let bridge = claude_deepseek_bridge();

        let mapping = bridge_model_mapping(&profile, Some(&bridge), "openai-chat", Some("default"))
            .expect("mapping should resolve");

        assert_eq!(mapping.upstream_model, "deepseek-v4-pro");
        assert_eq!(mapping.agent_model, "claude-opus-4-8[1m]");
    }

    #[test]
    fn bridge_mapping_matches_codex_case_folded_alias() {
        let profile = ProfileDef {
            id: "deepseek-test".to_string(),
            label: "DeepSeek Test".to_string(),
            provider: "deepseek".to_string(),
            auth_mode: AuthMode::ApiKey,
            credentials: BTreeMap::new(),
            api_configs: BTreeMap::new(),
            use_settings_proxy: false,
            provider_settings: Default::default(),
            connections: Default::default(),
        };
        let bridge = agent_state::ProfileBridgePreference {
            enabled: true,
            target_api_type: Some("openai-chat".to_string()),
            upstream_model: Some("deepseek-v4-pro".to_string()),
            fake_model_id: Some("GPT-5.5".to_string()),
            models: vec![agent_state::ProfileBridgeModelPreference {
                upstream_model: Some("deepseek-v4-pro".to_string()),
                fake_model_id: Some("GPT-5.5".to_string()),
                capabilities: Default::default(),
            }],
            headers: BTreeMap::new(),
        };

        let mapping = bridge_model_mapping(&profile, Some(&bridge), "openai-chat", Some("gpt-5.5"))
            .expect("mapping should resolve");

        assert_eq!(mapping.upstream_model, "deepseek-v4-pro");
        assert_eq!(mapping.agent_model, "GPT-5.5");
    }

    #[test]
    fn bridge_scope_parser_keeps_desktop_preferences_separate() {
        assert_eq!(
            agent_id_from_scope("codex-desktop-openai-responses", "openai-responses"),
            Some("codex-desktop")
        );
        assert_eq!(
            agent_id_from_scope("claude-desktop-anthropic", "anthropic"),
            Some("claude-desktop")
        );
    }

    fn deepseek_test_profile() -> ProfileDef {
        ProfileDef {
            id: "deepseek-test".to_string(),
            label: "DeepSeek Test".to_string(),
            provider: "deepseek".to_string(),
            auth_mode: AuthMode::ApiKey,
            credentials: BTreeMap::new(),
            api_configs: BTreeMap::new(),
            use_settings_proxy: false,
            provider_settings: Default::default(),
            connections: Default::default(),
        }
    }

    fn claude_deepseek_bridge() -> agent_state::ProfileBridgePreference {
        agent_state::ProfileBridgePreference {
            enabled: true,
            target_api_type: Some("openai-chat".to_string()),
            upstream_model: Some("deepseek-v4-pro".to_string()),
            fake_model_id: None,
            models: vec![
                agent_state::ProfileBridgeModelPreference {
                    upstream_model: Some("deepseek-v4-pro".to_string()),
                    fake_model_id: Some("claude-opus-4-8[1m]".to_string()),
                    capabilities: Default::default(),
                },
                agent_state::ProfileBridgeModelPreference {
                    upstream_model: Some("deepseek-v4-flash".to_string()),
                    fake_model_id: Some("claude-opus-4-7[1m]".to_string()),
                    capabilities: Default::default(),
                },
            ],
            headers: BTreeMap::new(),
        }
    }
}
