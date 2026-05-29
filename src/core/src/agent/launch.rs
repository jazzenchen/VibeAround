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

pub fn profile_uses_vibearound_credentials(profile: &str) -> bool {
    !matches!(profile, "default" | "none" | "off" | "direct")
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
    env.push(("VIBEAROUND_PROFILE_ID".to_string(), profile.id.clone()));
    env.push(("VIBEAROUND_LAUNCH_TARGET".to_string(), agent_id.to_string()));

    Ok(AppliedProfile { env, command_args })
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
