//! Profile-to-agent connection routing shared by desktop launch and web
//! terminal launch.
//!
//! A profile's enabled API configs tell us which provider protocols it exposes.
//! A launch target also depends on per-profile agent preferences: which client
//! protocol the agent should speak and whether VibeAround should bridge that
//! client protocol to another provider protocol.

use std::collections::BTreeMap;

use crate::agent_state;

use super::{
    catalog,
    schema::{self, ProfileDef},
};

pub const DEFAULT_CLAUDE_BRIDGE_MODEL_ID: &str = "claude-sonnet-4-5";

#[derive(Debug, Clone)]
pub struct ProfileAgentRoute {
    pub client_api_type: String,
    pub bridge_target_api_type: Option<String>,
    pub bridge_models: Vec<ProfileBridgeModelRoute>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProfileBridgeModelRoute {
    pub upstream_model: String,
    pub agent_model: String,
    pub capabilities: catalog::ContentCapabilities,
}

#[derive(Debug, Clone)]
pub struct ProfileLaunchTarget {
    pub id: &'static str,
    pub label: &'static str,
    pub api_type: String,
    pub bridge_target_api_type: Option<String>,
}

pub fn sanitize_profile_connection_preference(
    profile: &ProfileDef,
    agent_id: &str,
    preference: agent_state::ProfileConnectionPreference,
) -> Result<agent_state::ProfileConnectionPreference, String> {
    let supported = agent_client_api_types(agent_id);
    if supported.is_empty() {
        return Err(format!("unsupported connection target: '{}'", agent_id));
    }
    let enabled_api_types = schema::enabled_api_types(profile);
    let selected_api_type = preference
        .selected_api_type
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| recommended_client_api_type(profile, agent_id).unwrap_or(supported[0]));
    if !supported.contains(&selected_api_type) {
        return Err(format!(
            "{} does not support api kind '{}'",
            agent_id, selected_api_type
        ));
    }

    let mut bridge = BTreeMap::new();
    for (client_api_type, bridge_preference) in preference.bridge {
        let client_api_type = client_api_type.trim().to_string();
        if client_api_type.is_empty() {
            continue;
        }
        if !supported.contains(&client_api_type.as_str()) {
            return Err(format!(
                "{} does not support api kind '{}'",
                agent_id, client_api_type
            ));
        }
        let target_api_type = bridge_preference
            .target_api_type
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty());
        let target_api_type = if bridge_preference.enabled {
            let target_api_type = target_api_type.or_else(|| {
                recommended_bridge_target(&enabled_api_types, agent_id, &client_api_type)
            });
            let target_api_type = target_api_type.ok_or_else(|| {
                format!(
                    "profile '{}' has no API kind that can be used as a bridge target",
                    profile.id
                )
            })?;
            validate_bridge_target(profile, &target_api_type)?;
            Some(target_api_type)
        } else {
            target_api_type.filter(|api_type| validate_bridge_target(profile, api_type).is_ok())
        };
        let models = sanitize_bridge_models(bridge_preference.models);
        let headers = if bridge_preference.enabled {
            prune_bridge_headers(bridge_preference.headers)
        } else {
            BTreeMap::new()
        };
        if bridge_preference.enabled
            || target_api_type.is_some()
            || !models.is_empty()
            || !headers.is_empty()
        {
            bridge.insert(
                client_api_type,
                agent_state::ProfileBridgePreference {
                    enabled: bridge_preference.enabled,
                    target_api_type,
                    models,
                    headers,
                },
            );
        }
    }

    Ok(agent_state::ProfileConnectionPreference {
        selected_api_type: Some(selected_api_type.to_string()),
        bridge,
    })
}

pub fn profile_can_launch_agent(profile: &ProfileDef, agent_id: &str) -> bool {
    resolve_profile_agent_route(profile, agent_id).is_some()
}

pub fn resolve_profile_agent_route(
    profile: &ProfileDef,
    agent_id: &str,
) -> Option<ProfileAgentRoute> {
    let connections = merged_profile_connections();
    let mut route = resolve_profile_agent_route_with_connections(profile, agent_id, &connections)?;
    if crate::config::ensure_loaded()
        .service_side
        .image_input
        .is_configured()
    {
        for model in &mut route.bridge_models {
            model.capabilities.image_input = true;
        }
    }
    Some(route)
}

pub fn resolve_profile_agent_route_with_connections(
    profile: &ProfileDef,
    agent_id: &str,
    connections: &agent_state::ProfileConnectionPreferences,
) -> Option<ProfileAgentRoute> {
    let supported = agent_client_api_types(agent_id);
    if supported.is_empty() {
        return None;
    }
    let connection_agent_id = connection_agent_id(agent_id);

    let preference = connections
        .get(&profile.id)
        .and_then(|items| items.get(connection_agent_id));
    let preferred_client_api_type = preference
        .and_then(|preference| preference.selected_api_type.as_deref())
        .filter(|api_type| supported.contains(api_type))
        .filter(|api_type| client_route_available(profile, agent_id, preference, api_type))
        .map(ToString::to_string);
    let client_api_type = preferred_client_api_type
        .or_else(|| recommended_client_api_type(profile, agent_id).map(ToString::to_string))?;

    let bridge_preference =
        preference.and_then(|preference| preference.bridge.get(&client_api_type));
    if let Some(bridge_preference) = bridge_preference.filter(|bridge| bridge.enabled) {
        let enabled_api_types = schema::enabled_api_types(profile);
        let target_api_type = bridge_preference.target_api_type.clone().or_else(|| {
            recommended_bridge_target(&enabled_api_types, agent_id, &client_api_type)
        })?;
        if validate_bridge_target(profile, &target_api_type).is_ok() {
            let bridge_models =
                bridge_model_routes(profile, Some(bridge_preference), &target_api_type);
            return Some(ProfileAgentRoute {
                client_api_type,
                bridge_target_api_type: Some(target_api_type),
                bridge_models,
            });
        }
    }

    if schema::enabled_api_types(profile)
        .iter()
        .any(|api_type| api_type == &client_api_type)
    {
        return Some(ProfileAgentRoute {
            client_api_type,
            bridge_target_api_type: None,
            bridge_models: Vec::new(),
        });
    }

    None
}

pub fn bridge_model_routes(
    profile: &ProfileDef,
    bridge: Option<&agent_state::ProfileBridgePreference>,
    target_api_type: &str,
) -> Vec<ProfileBridgeModelRoute> {
    if let Some(models) = bridge
        .map(|bridge| bridge.models.as_slice())
        .filter(|models| !models.is_empty())
    {
        return dedupe_model_routes(
            models
                .iter()
                .filter_map(|entry| {
                    let upstream = clean_optional_string(entry.upstream_model.as_deref())?;
                    let fake = clean_optional_string(entry.fake_model_id.as_deref());
                    Some(model_route(
                        profile,
                        target_api_type,
                        upstream,
                        fake,
                        entry.capabilities.clone(),
                    ))
                })
                .collect(),
        );
    }

    let preferred = default_model(profile, target_api_type);
    let mut routes = Vec::new();
    if let Some(preferred) = preferred {
        routes.push(model_route(
            profile,
            target_api_type,
            preferred,
            None,
            catalog::ContentCapabilities::default(),
        ));
    }
    if let Some(models) =
        api_config_models(profile, target_api_type).filter(|models| !models.is_empty())
    {
        routes.extend(models.into_iter().filter_map(|model| {
            clean_optional_string(Some(model.id.as_str()))
                .map(|id| model_route(profile, target_api_type, id, None, model.capabilities))
        }));
    } else if let Some(endpoint) = endpoint_for(profile, target_api_type) {
        routes.extend(endpoint.models.iter().filter_map(|model| {
            clean_optional_string(Some(model.id.as_str())).map(|id| {
                model_route(
                    profile,
                    target_api_type,
                    id,
                    None,
                    catalog::ContentCapabilities::default(),
                )
            })
        }));
    }
    dedupe_model_routes(routes)
}

pub fn is_claude_usable_model_id(model_id: &str) -> bool {
    let model_id = catalog::strip_bracket_suffix(model_id).unwrap_or(model_id);
    let model_id = model_id.trim().to_ascii_lowercase();
    if model_id.is_empty() {
        return false;
    }
    let excluded = [
        "deepseek", "gemini", "glm", "gpt", "grok", "kimi", "llama", "mimo", "minimax", "moonshot",
        "nemotron", "nvidia", "openai", "qwen",
    ];
    if excluded.iter().any(|needle| model_id.contains(needle)) {
        return false;
    }
    model_id.contains("claude")
        || model_id.contains("anthropic")
        || ["sonnet", "opus", "haiku", "fable", "mythos"]
            .iter()
            .any(|prefix| {
                model_id == *prefix
                    || model_id
                        .strip_prefix(prefix)
                        .is_some_and(|rest| rest.starts_with('-'))
            })
}

fn sanitize_bridge_models(
    models: Vec<agent_state::ProfileBridgeModelPreference>,
) -> Vec<agent_state::ProfileBridgeModelPreference> {
    let mut out = Vec::new();
    for entry in models {
        let upstream_model = clean_optional_string(entry.upstream_model.as_deref());
        let Some(upstream_model) = upstream_model else {
            continue;
        };
        out.push(agent_state::ProfileBridgeModelPreference {
            upstream_model: Some(upstream_model),
            fake_model_id: clean_optional_string(entry.fake_model_id.as_deref()),
            capabilities: entry.capabilities,
        });
    }
    out
}

fn model_route(
    profile: &ProfileDef,
    target_api_type: &str,
    upstream_model: String,
    fake_model_id: Option<String>,
    capabilities: catalog::ContentCapabilities,
) -> ProfileBridgeModelRoute {
    let requested_upstream_model = upstream_model.trim().to_string();
    let upstream_model = canonical_model(profile, target_api_type, &requested_upstream_model)
        .unwrap_or_else(|| requested_upstream_model.clone());
    let agent_model = fake_model_id
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or(requested_upstream_model);
    ProfileBridgeModelRoute {
        upstream_model,
        agent_model,
        capabilities,
    }
}

fn dedupe_model_routes(routes: Vec<ProfileBridgeModelRoute>) -> Vec<ProfileBridgeModelRoute> {
    let mut out: Vec<ProfileBridgeModelRoute> = Vec::new();
    for route in routes {
        if route.upstream_model.is_empty() || route.agent_model.is_empty() {
            continue;
        }
        if let Some(existing) = out
            .iter_mut()
            .find(|existing| existing.agent_model == route.agent_model)
        {
            existing.capabilities = existing.capabilities.merge(&route.capabilities);
            continue;
        }
        out.push(route);
    }
    out
}

fn default_model(profile: &ProfileDef, target_api_type: &str) -> Option<String> {
    api_config_for(profile, target_api_type)
        .and_then(|config| {
            clean_optional_string(config.model.as_deref()).or_else(|| {
                config
                    .models
                    .iter()
                    .filter(|model| model.enabled)
                    .find_map(|model| clean_optional_string(Some(model.id.as_str())))
            })
        })
        .or_else(|| {
            endpoint_for(profile, target_api_type)?
                .models
                .first()
                .and_then(|model| clean_optional_string(Some(model.id.as_str())))
        })
}

fn canonical_model(profile: &ProfileDef, target_api_type: &str, model: &str) -> Option<String> {
    if let Some(model_id) = api_config_models(profile, target_api_type)
        .and_then(|models| canonical_api_config_model_id(&models, model))
    {
        return Some(model_id);
    }
    let endpoint = endpoint_for(profile, target_api_type)?;
    catalog::canonical_model_id(endpoint, model)
}

fn endpoint_for<'a>(
    profile: &'a ProfileDef,
    target_api_type: &str,
) -> Option<&'a catalog::EndpointDef> {
    let provider = catalog::get(&profile.provider)?;
    let endpoint_id =
        api_config_for(profile, target_api_type).and_then(|config| config.endpoint_id);
    catalog::find_endpoint(provider, target_api_type, endpoint_id.as_deref())
}

fn api_config_for(profile: &ProfileDef, target_api_type: &str) -> Option<schema::ProfileApiConfig> {
    schema::api_config_for(profile, target_api_type).filter(|config| config.enabled)
}

fn api_config_models(
    profile: &ProfileDef,
    target_api_type: &str,
) -> Option<Vec<schema::ProfileModelConfig>> {
    let models: Vec<_> = api_config_for(profile, target_api_type)?
        .models
        .into_iter()
        .filter(|model| model.enabled)
        .collect();
    Some(models)
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
        if let Some(model) = models.iter().find(|model| model.id == base_model) {
            return Some(model.id.clone());
        }
    }
    models
        .iter()
        .find(|model| model.id == model_id)
        .map(|model| model.id.clone())
}

fn clean_optional_string(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

pub fn launch_targets_for_profile(profile: &ProfileDef) -> Vec<ProfileLaunchTarget> {
    let connections = merged_profile_connections();
    launch_targets_for_profile_with_connections(profile, &connections)
}

pub fn launch_targets_for_profile_with_connections(
    profile: &ProfileDef,
    connections: &agent_state::ProfileConnectionPreferences,
) -> Vec<ProfileLaunchTarget> {
    launch_target_defs()
        .iter()
        .filter_map(|(agent_id, label)| {
            let agent_id = *agent_id;
            let label = *label;
            let route =
                resolve_profile_agent_route_with_connections(profile, agent_id, connections)?;
            Some(ProfileLaunchTarget {
                id: agent_id,
                label,
                api_type: route.client_api_type,
                bridge_target_api_type: route.bridge_target_api_type,
            })
        })
        .collect()
}

pub fn merged_profile_connections() -> agent_state::ProfileConnectionPreferences {
    let mut out = agent_state::ProfileConnectionPreferences::new();
    for profile in schema::list() {
        if !profile.connections.is_empty() {
            out.insert(profile.id, profile.connections);
        }
    }
    out
}

fn client_route_available(
    profile: &ProfileDef,
    agent_id: &str,
    preference: Option<&agent_state::ProfileConnectionPreference>,
    client_api_type: &str,
) -> bool {
    let enabled_api_types = schema::enabled_api_types(profile);
    if enabled_api_types
        .iter()
        .any(|api_type| api_type == client_api_type)
    {
        return true;
    }
    let Some(bridge_preference) =
        preference.and_then(|preference| preference.bridge.get(client_api_type))
    else {
        return false;
    };
    if !bridge_preference.enabled {
        return false;
    }
    let Some(target_api_type) = bridge_preference
        .target_api_type
        .clone()
        .or_else(|| recommended_bridge_target(&enabled_api_types, agent_id, client_api_type))
    else {
        return false;
    };
    validate_bridge_target(profile, &target_api_type).is_ok()
}

fn is_bridge_target_api_type(api_type: &str) -> bool {
    matches!(
        api_type,
        "anthropic" | "openai-responses" | "openai-chat" | "gemini"
    )
}

pub(crate) fn recommended_bridge_target(
    api_types: &[String],
    agent_id: &str,
    client_api_type: &str,
) -> Option<String> {
    let order: &[&str] = match (agent_id, client_api_type) {
        ("claude", "anthropic")
        | ("claude-desktop", "anthropic")
        | ("opencode", "anthropic")
        | ("pi", "anthropic") => &["openai-responses", "gemini", "openai-chat", "anthropic"],
        ("codex", "openai-responses")
        | ("codex-desktop", "openai-responses")
        | ("opencode", "openai-responses")
        | ("opencode", "openai-chat")
        | ("pi", "openai-responses")
        | ("pi", "openai-chat") => &["anthropic", "gemini", "openai-chat", "openai-responses"],
        ("gemini", "gemini") => &["openai-chat", "openai-responses", "anthropic"],
        _ => &[],
    };
    order
        .iter()
        .find(|candidate| api_types.iter().any(|api_type| api_type == *candidate))
        .map(|candidate| (*candidate).to_string())
}

fn validate_bridge_target(profile: &ProfileDef, target_api_type: &str) -> Result<(), String> {
    if !schema::enabled_api_types(profile)
        .iter()
        .any(|api_type| api_type == target_api_type)
    {
        return Err(format!(
            "profile '{}' does not expose api kind '{}'",
            profile.id, target_api_type
        ));
    }
    if !is_bridge_target_api_type(target_api_type) {
        return Err(format!(
            "api kind '{}' cannot be used as a bridge target",
            target_api_type
        ));
    }
    Ok(())
}

fn prune_bridge_headers(headers: BTreeMap<String, String>) -> BTreeMap<String, String> {
    headers
        .into_iter()
        .filter_map(|(name, value)| {
            let name = name.trim().to_string();
            (!name.is_empty()).then_some((name, value))
        })
        .collect()
}

fn agent_client_api_types(agent_id: &str) -> &'static [&'static str] {
    match agent_id {
        "claude" | "claude-desktop" => &["anthropic"],
        "codex" | "codex-desktop" => &["openai-responses"],
        "gemini" => &["gemini"],
        "opencode" => &["openai-responses", "openai-chat", "anthropic"],
        "pi" => &["anthropic", "openai-responses", "openai-chat"],
        _ => &[],
    }
}

fn recommended_client_api_type(profile: &ProfileDef, agent_id: &str) -> Option<&'static str> {
    let enabled_api_types = schema::enabled_api_types(profile);
    agent_client_api_types(agent_id)
        .iter()
        .find(|api_type| enabled_api_types.iter().any(|value| value == *api_type))
        .copied()
        .or_else(|| agent_client_api_types(agent_id).first().copied())
}

fn launch_target_defs() -> &'static [(&'static str, &'static str)] {
    &[
        ("claude", "Claude Code"),
        ("claude-desktop", "Claude Desktop"),
        ("codex", "Codex"),
        ("codex-desktop", "ChatGPT Desktop (Codex)"),
        ("gemini", "Gemini CLI"),
        ("pi", "Pi"),
        ("opencode", "OpenCode"),
    ]
}

fn connection_agent_id(agent_id: &str) -> &str {
    agent_id
}

#[cfg(test)]
mod tests;
