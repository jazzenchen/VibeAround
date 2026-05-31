//! Profile-to-agent connection routing shared by desktop launch and web
//! terminal launch.
//!
//! A profile's raw `api_types` tell us which provider protocols it exposes.
//! A launch target also depends on per-profile agent preferences: which client
//! protocol the agent should speak and whether VibeAround should bridge that
//! client protocol to another provider protocol.

use std::collections::BTreeMap;

use crate::agent_state;

mod legacy;

use super::{catalog, schema::ProfileDef};

#[derive(Debug, Clone)]
pub struct ProfileAgentRoute {
    pub client_api_type: String,
    pub bridge_target_api_type: Option<String>,
    pub bridge_upstream_model: Option<String>,
    pub bridge_fake_model_id: Option<String>,
    pub bridge_models: Vec<ProfileBridgeModelRoute>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProfileBridgeModelRoute {
    pub upstream_model: String,
    pub agent_model: String,
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
                recommended_bridge_target(&profile.api_types, agent_id, &client_api_type)
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
        let upstream_model = bridge_preference
            .upstream_model
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty());
        let fake_model_id = bridge_preference
            .fake_model_id
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty());
        let models = sanitize_bridge_models(bridge_preference.models);
        let headers = if bridge_preference.enabled {
            prune_bridge_headers(bridge_preference.headers)
        } else {
            BTreeMap::new()
        };
        if bridge_preference.enabled
            || target_api_type.is_some()
            || upstream_model.is_some()
            || fake_model_id.is_some()
            || !models.is_empty()
            || !headers.is_empty()
        {
            bridge.insert(
                client_api_type,
                agent_state::ProfileBridgePreference {
                    enabled: bridge_preference.enabled,
                    target_api_type,
                    upstream_model,
                    fake_model_id,
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
    let connections = merged_profile_connections(&agent_state::read_prefs());
    resolve_profile_agent_route_with_connections(profile, agent_id, &connections)
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

    let preference = connections
        .get(&profile.id)
        .and_then(|items| items.get(agent_id));
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
        let target_api_type = bridge_preference.target_api_type.clone().or_else(|| {
            recommended_bridge_target(&profile.api_types, agent_id, &client_api_type)
        })?;
        if validate_bridge_target(profile, &target_api_type).is_ok() {
            let bridge_models =
                bridge_model_routes(profile, Some(bridge_preference), &target_api_type);
            return Some(ProfileAgentRoute {
                client_api_type,
                bridge_target_api_type: Some(target_api_type),
                bridge_upstream_model: bridge_preference.upstream_model.clone(),
                bridge_fake_model_id: bridge_preference.fake_model_id.clone(),
                bridge_models,
            });
        }
    }

    if profile
        .api_types
        .iter()
        .any(|api_type| api_type == &client_api_type)
    {
        return Some(ProfileAgentRoute {
            client_api_type,
            bridge_target_api_type: None,
            bridge_upstream_model: None,
            bridge_fake_model_id: None,
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
                    Some(model_route(profile, target_api_type, upstream, fake))
                })
                .collect(),
        );
    }

    let legacy_upstream =
        bridge.and_then(|bridge| clean_optional_string(bridge.upstream_model.as_deref()));
    let legacy_fake =
        bridge.and_then(|bridge| clean_optional_string(bridge.fake_model_id.as_deref()));
    if legacy_fake.is_some() {
        return legacy_upstream
            .or_else(|| default_model(profile, target_api_type))
            .map(|upstream| vec![model_route(profile, target_api_type, upstream, legacy_fake)])
            .unwrap_or_default();
    }

    let preferred = legacy_upstream.or_else(|| default_model(profile, target_api_type));
    let mut routes = Vec::new();
    if let Some(preferred) = preferred {
        routes.push(model_route(profile, target_api_type, preferred, None));
    }
    if let Some(endpoint) = endpoint_for(profile, target_api_type) {
        routes.extend(endpoint.models.iter().filter_map(|model| {
            clean_optional_string(Some(model.id.as_str()))
                .map(|id| model_route(profile, target_api_type, id, None))
        }));
    }
    dedupe_model_routes(routes)
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
        });
    }
    out
}

fn model_route(
    profile: &ProfileDef,
    target_api_type: &str,
    upstream_model: String,
    fake_model_id: Option<String>,
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
    }
}

fn dedupe_model_routes(routes: Vec<ProfileBridgeModelRoute>) -> Vec<ProfileBridgeModelRoute> {
    let mut out = Vec::new();
    for route in routes {
        if route.upstream_model.is_empty() || route.agent_model.is_empty() {
            continue;
        }
        if out
            .iter()
            .any(|existing: &ProfileBridgeModelRoute| existing.agent_model == route.agent_model)
        {
            continue;
        }
        out.push(route);
    }
    out
}

fn default_model(profile: &ProfileDef, target_api_type: &str) -> Option<String> {
    profile
        .overrides
        .get(target_api_type)
        .and_then(|overrides| clean_optional_string(overrides.model.as_deref()))
        .or_else(|| {
            endpoint_for(profile, target_api_type)?
                .models
                .first()
                .and_then(|model| clean_optional_string(Some(model.id.as_str())))
        })
}

fn canonical_model(profile: &ProfileDef, target_api_type: &str, model: &str) -> Option<String> {
    let endpoint = endpoint_for(profile, target_api_type)?;
    catalog::canonical_model_id(endpoint, model)
}

fn endpoint_for<'a>(
    profile: &'a ProfileDef,
    target_api_type: &str,
) -> Option<&'a catalog::EndpointDef> {
    let provider = catalog::get(&profile.provider)?;
    let endpoint_id = profile
        .overrides
        .get(target_api_type)
        .and_then(|overrides| overrides.endpoint_id.as_deref());
    catalog::find_endpoint(provider, target_api_type, endpoint_id)
}

fn clean_optional_string(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

pub fn launch_targets_for_profile(profile: &ProfileDef) -> Vec<ProfileLaunchTarget> {
    let connections = merged_profile_connections(&agent_state::read_prefs());
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

pub fn merged_profile_connections(
    agent_prefs: &agent_state::AgentsPrefsFile,
) -> agent_state::ProfileConnectionPreferences {
    let mut out = legacy::profile_connections();
    for (profile_id, by_agent) in &agent_prefs.profile_connections {
        let entry = out.entry(profile_id.clone()).or_default();
        for (agent_id, preference) in by_agent {
            entry.insert(agent_id.clone(), preference.clone());
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
    if profile
        .api_types
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
        .or_else(|| recommended_bridge_target(&profile.api_types, agent_id, client_api_type))
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

fn recommended_bridge_target(
    api_types: &[String],
    agent_id: &str,
    client_api_type: &str,
) -> Option<String> {
    let order: &[&str] = match (agent_id, client_api_type) {
        ("claude", "anthropic") | ("opencode", "anthropic") | ("pi", "anthropic") => {
            &["openai-responses", "gemini", "openai-chat", "anthropic"]
        }
        ("codex", "openai-responses")
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
    if !profile
        .api_types
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
        "claude" => &["anthropic"],
        "codex" => &["openai-responses"],
        "gemini" => &["gemini"],
        "opencode" => &["openai-responses", "openai-chat", "anthropic"],
        "pi" => &["anthropic", "openai-responses", "openai-chat"],
        _ => &[],
    }
}

fn recommended_client_api_type(profile: &ProfileDef, agent_id: &str) -> Option<&'static str> {
    agent_client_api_types(agent_id)
        .iter()
        .find(|api_type| profile.api_types.iter().any(|value| value == *api_type))
        .copied()
        .or_else(|| agent_client_api_types(agent_id).first().copied())
}

fn launch_target_defs() -> &'static [(&'static str, &'static str)] {
    &[
        ("claude", "Claude Code"),
        ("codex", "Codex"),
        ("gemini", "Gemini CLI"),
        ("pi", "Pi"),
        ("opencode", "OpenCode"),
    ]
}

#[cfg(test)]
mod tests;
