use std::collections::BTreeMap;

use crate::agent_state;

#[derive(Debug, Default, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct LegacyLauncherPrefsFile {
    #[serde(default)]
    profile_connections: LegacyProfileConnectionPreferences,
}

#[derive(Debug, Clone, Default, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct LegacyProfileConnectionPreference {
    #[serde(default, alias = "proxyEnabled")]
    bridge_enabled: bool,
    #[serde(default)]
    target_api_type: Option<String>,
}

type LegacyProfileConnectionPreferences =
    BTreeMap<String, BTreeMap<String, LegacyProfileConnectionPreference>>;

pub(super) fn profile_connections() -> agent_state::ProfileConnectionPreferences {
    let body = match std::fs::read_to_string(crate::config::data_dir().join("launcher.json")) {
        Ok(body) => body,
        Err(_) => return BTreeMap::new(),
    };
    let legacy: LegacyLauncherPrefsFile = match serde_json::from_str(&body) {
        Ok(value) => value,
        Err(error) => {
            tracing::warn!("[profiles] launcher.json parse error: {}", error);
            return BTreeMap::new();
        }
    };
    let mut out = agent_state::ProfileConnectionPreferences::new();
    for (profile_id, by_agent) in legacy.profile_connections {
        let entry = out.entry(profile_id).or_default();
        for (agent_id, preference) in by_agent {
            let Some(selected_api_type) = default_client_api_type(&agent_id) else {
                continue;
            };
            let mut bridge = BTreeMap::new();
            if preference.bridge_enabled || preference.target_api_type.is_some() {
                bridge.insert(
                    selected_api_type.to_string(),
                    agent_state::ProfileBridgePreference {
                        enabled: preference.bridge_enabled,
                        target_api_type: preference.target_api_type,
                        upstream_model: None,
                        fake_model_id: None,
                        models: Vec::new(),
                        headers: BTreeMap::new(),
                    },
                );
            }
            entry.insert(
                agent_id,
                agent_state::ProfileConnectionPreference {
                    selected_api_type: Some(selected_api_type.to_string()),
                    bridge,
                },
            );
        }
    }
    out
}

fn default_client_api_type(agent_id: &str) -> Option<&'static str> {
    match agent_id {
        "claude" => Some("anthropic"),
        "codex" => Some("openai-responses"),
        "gemini" => Some("gemini"),
        "opencode" => Some("openai-responses"),
        _ => None,
    }
}
