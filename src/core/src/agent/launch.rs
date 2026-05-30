//! Helpers for launching ACP agents with VibeAround profiles applied.

use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::anyhow;

use crate::profiles;
use crate::routing::RouteKey;

pub struct AppliedProfile {
    pub env: Vec<(String, String)>,
    pub command_args: Vec<String>,
}

pub const VIBEAROUND_PROFILE_ID_ENV: &str = "VIBEAROUND_PROFILE_ID";
pub const DIRECT_PROFILE_ID: &str = "direct";

pub fn profile_uses_vibearound_credentials(profile: &str) -> bool {
    !matches!(
        profile.trim().to_ascii_lowercase().as_str(),
        "default" | "none" | "off" | DIRECT_PROFILE_ID
    )
}

pub fn normalize_launch_profile_id(profile_id: Option<&str>) -> String {
    let Some(profile_id) = profile_id
        .map(str::trim)
        .filter(|profile| !profile.is_empty())
    else {
        return DIRECT_PROFILE_ID.to_string();
    };
    match profile_id.to_ascii_lowercase().as_str() {
        "default" | "none" | "off" | DIRECT_PROFILE_ID => DIRECT_PROFILE_ID.to_string(),
        _ => profile_id.to_string(),
    }
}

pub fn append_profile_id_env(env: &mut Vec<(String, String)>, profile_id: Option<&str>) {
    let profile_id = normalize_launch_profile_id(profile_id);
    env.retain(|(key, _)| key != VIBEAROUND_PROFILE_ID_ENV);
    env.push((VIBEAROUND_PROFILE_ID_ENV.to_string(), profile_id));
}

pub fn materialize_profile_for_agent(
    profile_id: &str,
    agent_id: &str,
    workspace: &Path,
    channel_route: &RouteKey,
) -> anyhow::Result<AppliedProfile> {
    let profile = profiles::schema::load(profile_id)
        .map(profiles::normalize_legacy_profile_and_persist)
        .ok_or_else(|| anyhow!("profile '{}' not found", profile_id))?;
    let route = profiles::connections::resolve_profile_agent_route(&profile, agent_id).ok_or_else(
        || {
            anyhow!(
                "profile '{}' cannot launch agent '{}'",
                profile.id,
                agent_id
            )
        },
    )?;
    let launch_id = uuid::Uuid::new_v4().to_string();
    let rendered =
        profiles::runtime::render_for_agent_route(&profile, agent_id, &launch_id, &route)?;
    if route.bridge_target_api_type.is_some() {
        write_bridge_launch_metadata(
            &launch_id,
            &profile.id,
            agent_id,
            workspace,
            channel_route,
            &route,
        )?;
    }
    let command_args = rendered.command_args.clone();
    let mut env = profiles::runtime::materialize_env(&profile.id, rendered)?;
    if route.bridge_target_api_type.is_none() {
        profiles::runtime::append_settings_proxy_env(&profile, &mut env)?;
    }
    env.push(("VIBEAROUND_LAUNCH_ID".to_string(), launch_id));
    append_profile_id_env(&mut env, Some(&profile.id));
    env.push(("VIBEAROUND_LAUNCH_TARGET".to_string(), agent_id.to_string()));

    Ok(AppliedProfile { env, command_args })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn append_profile_id_env_preserves_direct_launch_profile() {
        let mut env = vec![("OTHER".to_string(), "1".to_string())];
        append_profile_id_env(&mut env, Some(" direct "));

        assert_eq!(
            env,
            vec![
                ("OTHER".to_string(), "1".to_string()),
                (VIBEAROUND_PROFILE_ID_ENV.to_string(), "direct".to_string()),
            ]
        );
    }

    #[test]
    fn normalize_launch_profile_id_defaults_external_sessions_to_direct() {
        assert_eq!(normalize_launch_profile_id(None), DIRECT_PROFILE_ID);
        assert_eq!(normalize_launch_profile_id(Some("")), DIRECT_PROFILE_ID);
        assert_eq!(
            normalize_launch_profile_id(Some("DEFAULT")),
            DIRECT_PROFILE_ID
        );
        assert_eq!(
            normalize_launch_profile_id(Some("profile-a")),
            "profile-a".to_string()
        );
    }

    #[test]
    fn append_profile_id_env_replaces_existing_value() {
        let mut env = vec![
            (VIBEAROUND_PROFILE_ID_ENV.to_string(), "old".to_string()),
            ("OTHER".to_string(), "1".to_string()),
        ];
        append_profile_id_env(&mut env, Some("profile-a"));

        assert_eq!(
            env,
            vec![
                ("OTHER".to_string(), "1".to_string()),
                (
                    VIBEAROUND_PROFILE_ID_ENV.to_string(),
                    "profile-a".to_string()
                ),
            ]
        );
    }
}

fn write_bridge_launch_metadata(
    launch_id: &str,
    profile_id: &str,
    agent_id: &str,
    workspace: &Path,
    channel_route: &RouteKey,
    route: &profiles::connections::ProfileAgentRoute,
) -> anyhow::Result<()> {
    let dir = crate::config::data_dir()
        .join("api-bridge")
        .join("launches");
    std::fs::create_dir_all(&dir)?;
    let body = serde_json::json!({
        "schemaVersion": 1,
        "createdAtUnix": SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_secs())
            .unwrap_or_default(),
        "launchId": launch_id,
        "profileId": profile_id,
        "agent": agent_id,
        "workspace": workspace.to_string_lossy(),
        "channelKind": channel_route.channel_kind,
        "chatId": channel_route.chat_id,
        "clientProtocol": route.client_api_type,
        "upstreamProtocol": route.bridge_target_api_type,
    });
    let path = dir.join(format!("{launch_id}.json"));
    std::fs::write(path, serde_json::to_vec_pretty(&body)?)?;
    Ok(())
}
