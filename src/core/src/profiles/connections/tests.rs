use std::collections::BTreeMap;

use super::*;
use crate::profiles::schema::{AuthMode, ProviderSettings};

fn profile(api_types: &[&str]) -> ProfileDef {
    ProfileDef {
        id: "profile-test".to_string(),
        label: "Profile Test".to_string(),
        provider: "custom".to_string(),
        auth_mode: AuthMode::ApiKey,
        api_types: api_types.iter().map(|value| (*value).to_string()).collect(),
        credentials: BTreeMap::new(),
        overrides: BTreeMap::new(),
        use_settings_proxy: false,
        provider_settings: ProviderSettings::default(),
        connections: Default::default(),
    }
}

fn connections(
    profile_id: &str,
    agent_id: &str,
    preference: agent_state::ProfileConnectionPreference,
) -> agent_state::ProfileConnectionPreferences {
    [(
        profile_id.to_string(),
        [(agent_id.to_string(), preference)].into_iter().collect(),
    )]
    .into_iter()
    .collect()
}

#[test]
fn native_route_uses_profile_api_type() {
    let profile = profile(&["openai-responses"]);
    let route = resolve_profile_agent_route_with_connections(&profile, "codex", &BTreeMap::new())
        .expect("codex route");

    assert_eq!(route.client_api_type, "openai-responses");
    assert_eq!(route.bridge_target_api_type, None);
}

#[test]
fn bridge_route_enables_agent_for_other_profile_api_type() {
    let profile = profile(&["anthropic"]);
    let prefs = connections(
        &profile.id,
        "codex",
        agent_state::ProfileConnectionPreference {
            selected_api_type: Some("openai-responses".to_string()),
            bridge: [(
                "openai-responses".to_string(),
                agent_state::ProfileBridgePreference {
                    enabled: true,
                    target_api_type: Some("anthropic".to_string()),
                    ..Default::default()
                },
            )]
            .into_iter()
            .collect(),
        },
    );
    let route = resolve_profile_agent_route_with_connections(&profile, "codex", &prefs)
        .expect("codex bridge route");

    assert_eq!(route.client_api_type, "openai-responses");
    assert_eq!(route.bridge_target_api_type.as_deref(), Some("anthropic"));
}

#[test]
fn bridge_launch_target_carries_bridge_hint() {
    let profile = profile(&["anthropic"]);
    let prefs = connections(
        &profile.id,
        "codex",
        agent_state::ProfileConnectionPreference {
            selected_api_type: Some("openai-responses".to_string()),
            bridge: [(
                "openai-responses".to_string(),
                agent_state::ProfileBridgePreference {
                    enabled: true,
                    target_api_type: Some("anthropic".to_string()),
                    ..Default::default()
                },
            )]
            .into_iter()
            .collect(),
        },
    );

    let targets = launch_targets_for_profile_with_connections(&profile, &prefs);
    let target = targets
        .iter()
        .find(|target| target.id == "codex")
        .expect("codex bridge target");

    assert_eq!(target.api_type, "openai-responses");
    assert_eq!(target.bridge_target_api_type.as_deref(), Some("anthropic"));
}

#[test]
fn unsupported_without_native_or_bridge_route() {
    let profile = profile(&["anthropic"]);

    assert!(
        resolve_profile_agent_route_with_connections(&profile, "codex", &BTreeMap::new()).is_none()
    );
}

#[test]
fn gemini_profile_has_native_launch_target() {
    let profile = profile(&["gemini"]);
    let targets = launch_targets_for_profile_with_connections(&profile, &BTreeMap::new());

    assert_eq!(targets.len(), 1);
    assert_eq!(targets[0].id, "gemini");
    assert_eq!(targets[0].api_type, "gemini");
    assert_eq!(targets[0].bridge_target_api_type, None);
}

#[test]
fn pi_can_launch_openai_chat_profile_natively() {
    let profile = profile(&["openai-chat"]);
    let route = resolve_profile_agent_route_with_connections(&profile, "pi", &BTreeMap::new())
        .expect("pi route");

    assert_eq!(route.client_api_type, "openai-chat");
    assert_eq!(route.bridge_target_api_type, None);

    let targets = launch_targets_for_profile_with_connections(&profile, &BTreeMap::new());
    assert!(targets.iter().any(|target| {
        target.id == "pi"
            && target.api_type == "openai-chat"
            && target.bridge_target_api_type.is_none()
    }));
}

#[test]
fn gemini_cli_can_launch_openai_chat_profile_via_bridge() {
    let profile = profile(&["openai-chat"]);
    let prefs = connections(
        &profile.id,
        "gemini",
        agent_state::ProfileConnectionPreference {
            selected_api_type: Some("gemini".to_string()),
            bridge: [(
                "gemini".to_string(),
                agent_state::ProfileBridgePreference {
                    enabled: true,
                    target_api_type: Some("openai-chat".to_string()),
                    upstream_model: Some("gpt-test".to_string()),
                    ..Default::default()
                },
            )]
            .into_iter()
            .collect(),
        },
    );

    let route = resolve_profile_agent_route_with_connections(&profile, "gemini", &prefs)
        .expect("gemini bridge route");

    assert_eq!(route.client_api_type, "gemini");
    assert_eq!(route.bridge_target_api_type.as_deref(), Some("openai-chat"));
    assert_eq!(route.bridge_upstream_model.as_deref(), Some("gpt-test"));
}

#[test]
fn codex_can_launch_gemini_profile_via_bridge() {
    let profile = profile(&["gemini"]);
    let prefs = connections(
        &profile.id,
        "codex",
        agent_state::ProfileConnectionPreference {
            selected_api_type: Some("openai-responses".to_string()),
            bridge: [(
                "openai-responses".to_string(),
                agent_state::ProfileBridgePreference {
                    enabled: true,
                    target_api_type: Some("gemini".to_string()),
                    upstream_model: Some("gemini-2.5-flash".to_string()),
                    ..Default::default()
                },
            )]
            .into_iter()
            .collect(),
        },
    );

    let route = resolve_profile_agent_route_with_connections(&profile, "codex", &prefs)
        .expect("codex gemini bridge route");

    assert_eq!(route.client_api_type, "openai-responses");
    assert_eq!(route.bridge_target_api_type.as_deref(), Some("gemini"));
    assert_eq!(
        route.bridge_upstream_model.as_deref(),
        Some("gemini-2.5-flash")
    );
}

#[test]
fn codex_desktop_uses_desktop_bridge_connection() {
    let profile = profile(&["gemini"]);
    let prefs = connections(
        &profile.id,
        "codex-desktop",
        agent_state::ProfileConnectionPreference {
            selected_api_type: Some("openai-responses".to_string()),
            bridge: [(
                "openai-responses".to_string(),
                agent_state::ProfileBridgePreference {
                    enabled: true,
                    target_api_type: Some("gemini".to_string()),
                    upstream_model: Some("gemini-2.5-flash".to_string()),
                    ..Default::default()
                },
            )]
            .into_iter()
            .collect(),
        },
    );

    let route = resolve_profile_agent_route_with_connections(&profile, "codex-desktop", &prefs)
        .expect("codex desktop bridge route");
    let targets = launch_targets_for_profile_with_connections(&profile, &prefs);

    assert_eq!(route.client_api_type, "openai-responses");
    assert_eq!(route.bridge_target_api_type.as_deref(), Some("gemini"));
    assert!(targets.iter().any(|target| {
        target.id == "codex-desktop"
            && target.api_type == "openai-responses"
            && target.bridge_target_api_type.as_deref() == Some("gemini")
    }));
}

#[test]
fn codex_desktop_does_not_reuse_codex_bridge_connection() {
    let profile = profile(&["gemini"]);
    let prefs = connections(
        &profile.id,
        "codex",
        agent_state::ProfileConnectionPreference {
            selected_api_type: Some("openai-responses".to_string()),
            bridge: [(
                "openai-responses".to_string(),
                agent_state::ProfileBridgePreference {
                    enabled: true,
                    target_api_type: Some("gemini".to_string()),
                    upstream_model: Some("gemini-2.5-flash".to_string()),
                    ..Default::default()
                },
            )]
            .into_iter()
            .collect(),
        },
    );

    assert!(
        resolve_profile_agent_route_with_connections(&profile, "codex-desktop", &prefs).is_none()
    );
}

#[test]
fn claude_desktop_uses_desktop_bridge_connection() {
    let profile = profile(&["openai-responses"]);
    let prefs = connections(
        &profile.id,
        "claude-desktop",
        agent_state::ProfileConnectionPreference {
            selected_api_type: Some("anthropic".to_string()),
            bridge: [(
                "anthropic".to_string(),
                agent_state::ProfileBridgePreference {
                    enabled: true,
                    target_api_type: Some("openai-responses".to_string()),
                    upstream_model: Some("gpt-5.1-codex".to_string()),
                    ..Default::default()
                },
            )]
            .into_iter()
            .collect(),
        },
    );

    let route = resolve_profile_agent_route_with_connections(&profile, "claude-desktop", &prefs)
        .expect("claude desktop bridge route");
    let targets = launch_targets_for_profile_with_connections(&profile, &prefs);

    assert_eq!(route.client_api_type, "anthropic");
    assert_eq!(
        route.bridge_target_api_type.as_deref(),
        Some("openai-responses")
    );
    assert!(targets.iter().any(|target| {
        target.id == "claude-desktop"
            && target.api_type == "anthropic"
            && target.bridge_target_api_type.as_deref() == Some("openai-responses")
    }));
}

#[test]
fn claude_desktop_does_not_reuse_claude_bridge_connection() {
    let profile = profile(&["openai-responses"]);
    let prefs = connections(
        &profile.id,
        "claude",
        agent_state::ProfileConnectionPreference {
            selected_api_type: Some("anthropic".to_string()),
            bridge: [(
                "anthropic".to_string(),
                agent_state::ProfileBridgePreference {
                    enabled: true,
                    target_api_type: Some("openai-responses".to_string()),
                    upstream_model: Some("gpt-5.1-codex".to_string()),
                    ..Default::default()
                },
            )]
            .into_iter()
            .collect(),
        },
    );

    assert!(
        resolve_profile_agent_route_with_connections(&profile, "claude-desktop", &prefs).is_none()
    );
}

#[test]
fn bridge_recommendation_can_target_gemini() {
    let profile = profile(&["gemini", "openai-chat"]);
    let prefs = connections(
        &profile.id,
        "codex",
        agent_state::ProfileConnectionPreference {
            selected_api_type: Some("openai-responses".to_string()),
            bridge: [(
                "openai-responses".to_string(),
                agent_state::ProfileBridgePreference {
                    enabled: true,
                    ..Default::default()
                },
            )]
            .into_iter()
            .collect(),
        },
    );

    let route = resolve_profile_agent_route_with_connections(&profile, "codex", &prefs)
        .expect("codex recommended gemini bridge route");

    assert_eq!(route.bridge_target_api_type.as_deref(), Some("gemini"));
}

#[test]
fn bridge_route_carries_model_list() {
    let profile = profile(&["openai-chat"]);
    let prefs = connections(
        &profile.id,
        "codex",
        agent_state::ProfileConnectionPreference {
            selected_api_type: Some("openai-responses".to_string()),
            bridge: [(
                "openai-responses".to_string(),
                agent_state::ProfileBridgePreference {
                    enabled: true,
                    target_api_type: Some("openai-chat".to_string()),
                    models: vec![
                        agent_state::ProfileBridgeModelPreference {
                            upstream_model: Some("gpt-real".to_string()),
                            fake_model_id: Some("gpt-fake".to_string()),
                            capabilities: Default::default(),
                        },
                        agent_state::ProfileBridgeModelPreference {
                            upstream_model: Some("provider-extra".to_string()),
                            fake_model_id: None,
                            capabilities: Default::default(),
                        },
                    ],
                    ..Default::default()
                },
            )]
            .into_iter()
            .collect(),
        },
    );

    let route = resolve_profile_agent_route_with_connections(&profile, "codex", &prefs)
        .expect("codex bridge route");

    assert_eq!(
        route.bridge_models,
        vec![
            ProfileBridgeModelRoute {
                upstream_model: "gpt-real".to_string(),
                agent_model: "gpt-fake".to_string(),
                capabilities: Default::default(),
            },
            ProfileBridgeModelRoute {
                upstream_model: "provider-extra".to_string(),
                agent_model: "provider-extra".to_string(),
                capabilities: Default::default(),
            },
        ]
    );
}

#[test]
fn claude_usable_model_id_accepts_claude_style_ids() {
    assert!(is_claude_usable_model_id("claude-sonnet-4-5"));
    assert!(is_claude_usable_model_id("opus-4.7[1m]"));
    assert!(!is_claude_usable_model_id(
        "nvidia/nemotron-3-super-120b-a12b"
    ));
}
