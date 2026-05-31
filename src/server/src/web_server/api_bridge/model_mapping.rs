use common::agent_state;
use common::profiles::{catalog, connections, schema::ProfileDef};

#[derive(Debug, Clone)]
pub(super) struct BridgeModelMapping {
    pub(super) upstream_model: String,
    pub(super) agent_model: String,
}

pub(super) fn bridge_route_preference(
    profile: &ProfileDef,
    route_scope: Option<&str>,
    client_api_type: &str,
    target_api_type: &str,
) -> Option<agent_state::ProfileBridgePreference> {
    let agent_id = agent_id_from_scope(route_scope?, client_api_type)?;
    let prefs = agent_state::read_prefs();
    let preference = prefs.profile_connections.get(&profile.id)?.get(agent_id)?;
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
        if let Some(route) = routes
            .iter()
            .find(|route| agent_model_matches(&route.agent_model, &requested_agent_model))
        {
            return Some(BridgeModelMapping {
                upstream_model: route.upstream_model.clone(),
                agent_model: route.agent_model.clone(),
            });
        }
        let upstream_model = canonical_model(profile, target_api_type, &requested_agent_model)
            .unwrap_or_else(|| requested_agent_model.clone());
        return Some(BridgeModelMapping {
            upstream_model,
            agent_model: requested_agent_model,
        });
    }

    let route = routes.into_iter().next()?;
    Some(BridgeModelMapping {
        upstream_model: route.upstream_model,
        agent_model: route.agent_model,
    })
}

fn agent_id_from_scope(scope: &str, client_api_type: &str) -> Option<&'static str> {
    for agent_id in ["claude", "codex", "gemini", "opencode", "pi"] {
        let prefix = format!("{agent_id}-");
        if scope.strip_prefix(&prefix) == Some(client_api_type) {
            return Some(agent_id);
        }
    }
    None
}

fn canonical_model(profile: &ProfileDef, target_api_type: &str, model: &str) -> Option<String> {
    let provider = catalog::get(&profile.provider)?;
    let endpoint_id = profile
        .overrides
        .get(target_api_type)
        .and_then(|overrides| overrides.endpoint_id.as_deref());
    let endpoint = catalog::find_endpoint(provider, target_api_type, endpoint_id)?;
    catalog::canonical_model_id(endpoint, model)
}

fn agent_model_matches(configured: &str, requested: &str) -> bool {
    configured == requested
        || catalog::strip_bracket_suffix(configured)
            .map(|base| base == requested)
            .unwrap_or(false)
        || catalog::strip_bracket_suffix(requested)
            .map(|base| base == configured)
            .unwrap_or(false)
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

    use common::profiles::schema::{ApiTypeOverrides, AuthMode};

    use super::*;

    #[test]
    fn bridge_mapping_canonicalizes_gemini_alias_for_upstream() {
        let profile = ProfileDef {
            id: "gemini-test".to_string(),
            label: "Gemini Test".to_string(),
            provider: "gemini".to_string(),
            auth_mode: AuthMode::ApiKey,
            api_types: vec!["openai-chat".to_string()],
            credentials: BTreeMap::new(),
            overrides: [(
                "openai-chat".to_string(),
                ApiTypeOverrides {
                    endpoint_id: Some("gemini-api".to_string()),
                    base_url: None,
                    model: Some("gemini-3.1-pro".to_string()),
                    reasoning_effort: None,
                    capabilities: None,
                },
            )]
            .into_iter()
            .collect(),
            use_settings_proxy: false,
            provider_settings: Default::default(),
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
            api_types: vec!["openai-chat".to_string()],
            credentials: BTreeMap::new(),
            overrides: BTreeMap::new(),
            use_settings_proxy: false,
            provider_settings: Default::default(),
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
            api_types: vec!["openai-chat".to_string()],
            credentials: BTreeMap::new(),
            overrides: BTreeMap::new(),
            use_settings_proxy: false,
            provider_settings: Default::default(),
        };
        let bridge = agent_state::ProfileBridgePreference {
            enabled: true,
            target_api_type: Some("openai-chat".to_string()),
            upstream_model: Some("deepseek-v4-pro".to_string()),
            fake_model_id: Some("opus-4.7[1m]".to_string()),
            models: vec![agent_state::ProfileBridgeModelPreference {
                upstream_model: Some("deepseek-v4-pro".to_string()),
                fake_model_id: Some("opus-4.7[1m]".to_string()),
            }],
            headers: BTreeMap::new(),
        };

        let mapping =
            bridge_model_mapping(&profile, Some(&bridge), "openai-chat", Some("opus-4.7"))
                .expect("mapping should resolve");

        assert_eq!(mapping.upstream_model, "deepseek-v4-pro");
        assert_eq!(mapping.agent_model, "opus-4.7[1m]");
    }
}
