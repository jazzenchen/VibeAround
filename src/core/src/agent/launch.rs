//! Helpers for launching ACP agents with VibeAround profiles applied.

use std::path::Path;

use anyhow::anyhow;

use crate::profiles;
use crate::routing::RouteKey;

pub struct AppliedProfile {
    pub env: Vec<(String, String)>,
    pub command_args: Vec<String>,
}

pub const VIBEAROUND_PROFILE_ID_ENV: &str = "VIBEAROUND_PROFILE_ID";
pub const VIBEAROUND_AGENT_DIR_ENV: &str = "VIBEAROUND_AGENT_DIR";
pub const VIBEAROUND_DATA_DIR_ENV: &str = "VIBEAROUND_DATA_DIR";
/// Session a terminal launch resumes; the TUI opens on it for built-in agents.
pub const VIBEAROUND_SESSION_ID_ENV: &str = "VIBEAROUND_SESSION_ID";
pub const DIRECT_PROFILE_ID: &str = "direct";

/// Built-in agents get their private state dir and VibeAround's data dir; the
/// latter lets them refuse reads of product data without guessing where it is.
pub fn append_agent_runtime_env(env: &mut Vec<(String, String)>, agent_id: &str) {
    if !crate::resources::agent_by_id(agent_id).is_some_and(|agent| agent.built_in) {
        return;
    }
    let data_dir = crate::config::data_dir();
    env.retain(|(key, _)| key != VIBEAROUND_AGENT_DIR_ENV && key != VIBEAROUND_DATA_DIR_ENV);
    env.push((
        VIBEAROUND_AGENT_DIR_ENV.to_string(),
        data_dir
            .join("agents")
            .join(agent_id)
            .to_string_lossy()
            .into_owned(),
    ));
    env.push((
        VIBEAROUND_DATA_DIR_ENV.to_string(),
        data_dir.to_string_lossy().into_owned(),
    ));
}

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
    _workspace: &Path,
    _channel_route: &RouteKey,
) -> anyhow::Result<AppliedProfile> {
    let profile = profiles::load_profile(profile_id)
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

    #[test]
    fn built_in_agents_receive_their_state_dir_and_the_data_dir() {
        let mut env = vec![(VIBEAROUND_AGENT_DIR_ENV.to_string(), "/wrong".to_string())];
        append_agent_runtime_env(&mut env, "va-agent");

        assert_eq!(env.len(), 2);
        let agent_dir = std::path::Path::new(&env[0].1);
        let data_dir = std::path::Path::new(&env[1].1);
        assert_eq!(env[0].0, VIBEAROUND_AGENT_DIR_ENV);
        assert_eq!(env[1].0, VIBEAROUND_DATA_DIR_ENV);
        assert_eq!(agent_dir, data_dir.join("agents").join("va-agent"));

        let mut other = Vec::new();
        append_agent_runtime_env(&mut other, "claude");
        assert!(other.is_empty());
    }
}
