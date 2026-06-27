use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{anyhow, bail, Context};
use common::{agent_state, config, profiles};
use profiles::{normalize_legacy_profile, ProfileDef};
use serde::{Deserialize, Serialize};

use crate::{paths, NativeLaunchArgs, NativeLaunchInput, TerminalChoice};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(deny_unknown_fields)]
pub struct LaunchProfile {
    #[serde(default)]
    pub schema_version: Option<u32>,
    #[serde(default)]
    pub id: Option<String>,
    pub agent: String,
    #[serde(default)]
    pub profile_id: Option<String>,
    #[serde(default)]
    pub launch_target: Option<String>,
    #[serde(default)]
    pub workspace: Option<PathBuf>,
    #[serde(default)]
    pub session_id: Option<String>,
    #[serde(default)]
    pub terminal: Option<TerminalChoice>,
    #[serde(default)]
    pub command: Option<String>,
    #[serde(default)]
    pub executable_path: Option<PathBuf>,
    #[serde(default)]
    pub windows_executable_path: Option<PathBuf>,
    #[serde(default)]
    pub window_label: Option<String>,
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    #[serde(default)]
    pub args: NativeLaunchArgs,
    #[serde(default)]
    pub cleanup_paths: Vec<PathBuf>,
    #[serde(default)]
    pub macos_app_probe: Option<String>,
    #[serde(default)]
    pub windows_process_probe: Option<String>,
}

impl LaunchProfile {
    pub fn into_native_input(
        self,
        selected_id: Option<String>,
    ) -> anyhow::Result<NativeLaunchInput> {
        let schema_version = self.schema_version.unwrap_or(1);
        if schema_version != 1 {
            bail!(
                "unsupported launch profile schemaVersion {}",
                schema_version
            );
        }

        Ok(NativeLaunchInput {
            schema_version,
            agent: self.agent,
            profile_id: self.profile_id.or(self.id).or(selected_id),
            launch_target: self.launch_target,
            workspace: self.workspace,
            session_id: self.session_id,
            terminal: self.terminal,
            command: self.command,
            executable_path: self.executable_path,
            windows_executable_path: self.windows_executable_path,
            window_label: self.window_label,
            env: self.env,
            args: self.args,
            cleanup_paths: self.cleanup_paths,
            macos_app_probe: self.macos_app_probe,
            windows_process_probe: self.windows_process_probe,
        })
    }
}

pub fn load_launch_profile(name: &str) -> anyhow::Result<NativeLaunchInput> {
    paths::validate_launch_name(name, "profile")?;
    let profile = profiles::schema::load(name)
        .map(normalize_legacy_profile)
        .ok_or_else(|| anyhow!("profile '{}' not found", name))?;
    model_profile_into_native_input(profile)
}

pub fn load_launch_profile_path(path: &Path) -> anyhow::Result<NativeLaunchInput> {
    let profile = read_profile_file(path)?;
    let selected_id = path
        .file_stem()
        .and_then(|name| name.to_str())
        .filter(|name| !name.trim().is_empty())
        .map(ToString::to_string);
    profile.into_native_input(selected_id)
}

fn read_profile_file(path: &Path) -> anyhow::Result<LaunchProfile> {
    let body = fs::read_to_string(path)
        .with_context(|| format!("read launch profile {}", path.display()))?;
    serde_json::from_str(&body).with_context(|| format!("parse launch profile {}", path.display()))
}

fn model_profile_into_native_input(profile: ProfileDef) -> anyhow::Result<NativeLaunchInput> {
    let cfg = config::ensure_loaded();
    let prefs = agent_state::read_prefs();
    let launch_target = resolve_launch_target_for_profile(&profile, &prefs, &cfg)?;
    let route = profiles::connections::resolve_profile_agent_route(&profile, &launch_target)
        .ok_or_else(|| anyhow!("profile '{}' cannot launch '{}'", profile.id, launch_target))?;
    let launch_id = uuid::Uuid::new_v4().to_string();
    let rendered =
        profiles::runtime::render_for_agent_route(&profile, &launch_target, &launch_id, &route)?;
    let command_args = rendered.command_args.clone();
    let env = profiles::runtime::materialize_env_for_profile(&profile, rendered)?
        .into_iter()
        .collect();
    let workspace = agent_state::resolve_agent_workspace(&prefs, &cfg, &launch_target);

    Ok(NativeLaunchInput {
        schema_version: 1,
        agent: launch_target.clone(),
        profile_id: Some(profile.id.clone()),
        launch_target: Some(launch_target),
        workspace: Some(workspace),
        session_id: None,
        terminal: None,
        command: None,
        executable_path: None,
        windows_executable_path: None,
        window_label: Some(profile.label),
        env,
        args: NativeLaunchArgs {
            native: command_args,
        },
        cleanup_paths: Vec::new(),
        macos_app_probe: None,
        windows_process_probe: None,
    })
}

fn resolve_launch_target_for_profile(
    profile: &ProfileDef,
    prefs: &agent_state::AgentsPrefsFile,
    cfg: &config::Config,
) -> anyhow::Result<String> {
    let default_agent = agent_state::resolve_default_agent(prefs, cfg);
    if profiles::connections::profile_can_launch_agent(profile, &default_agent) {
        return Ok(default_agent);
    }

    cfg.enabled_agents
        .iter()
        .find(|agent_id| profiles::connections::profile_can_launch_agent(profile, agent_id))
        .cloned()
        .ok_or_else(|| anyhow!("profile '{}' has no enabled launch target", profile.id))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn named_profile_loads_api_profile_from_shared_profile_dir() {
        let _guard = crate::env_test_lock().lock().expect("env test lock");
        let dir = temp_dir();
        let profile_dir = dir.join("profiles");
        fs::create_dir_all(&profile_dir).expect("create profile dir");
        fs::write(
            dir.join("settings.json"),
            r#"{ "default_agent": "codex", "enabled_agents": ["codex"] }"#,
        )
        .expect("write settings");
        fs::write(
            profile_dir.join("codex-work.json"),
            r#"{
  "id": "codex-work",
  "label": "Codex Work",
  "provider": "xai",
  "auth_mode": "api_key",
  "api_types": ["openai-responses"],
  "credentials": {
    "api_key": "secret"
  }
}"#,
        )
        .expect("write profile");

        let previous = std::env::var_os("VIBEAROUND_DATA_DIR");
        std::env::set_var("VIBEAROUND_DATA_DIR", &dir);

        let input = load_launch_profile("codex-work").expect("load profile");

        restore_env("VIBEAROUND_DATA_DIR", previous);
        let _ = fs::remove_dir_all(&dir);

        assert_eq!(input.agent, "codex");
        assert_eq!(input.profile_id.as_deref(), Some("codex-work"));
        assert_eq!(input.launch_target.as_deref(), Some("codex"));
        assert_eq!(input.workspace, Some(dir.join("workspaces")));
        assert_eq!(input.env["OPENAI_API_KEY"], "secret");
        assert!(input.cleanup_paths.is_empty());
    }

    #[test]
    fn profile_path_uses_profile_id_before_file_stem() {
        let dir = temp_dir();
        fs::create_dir_all(&dir).expect("create temp dir");
        let path = dir.join("launch.json");
        fs::write(
            &path,
            r#"{
  "agent": "claude",
  "profileId": "anthropic-main",
  "terminal": "terminal"
}"#,
        )
        .expect("write profile");

        let input = load_launch_profile_path(&path).expect("load profile path");
        let _ = fs::remove_dir_all(&dir);

        assert_eq!(input.agent, "claude");
        assert_eq!(input.profile_id.as_deref(), Some("anthropic-main"));
        assert_eq!(input.terminal, Some(TerminalChoice::Terminal));
    }

    #[test]
    fn profile_path_loads_windows_executable_path_separately() {
        let dir = temp_dir();
        fs::create_dir_all(&dir).expect("create temp dir");
        let path = dir.join("launch.json");
        fs::write(
            &path,
            r#"{
  "agent": "codex-desktop",
  "command": "Start-Process Codex",
  "windowsExecutablePath": "OpenAI.Codex_2p2nqsd0c76g0!App"
}"#,
        )
        .expect("write profile");

        let input = load_launch_profile_path(&path).expect("load profile path");
        let _ = fs::remove_dir_all(&dir);

        assert_eq!(input.command.as_deref(), Some("Start-Process Codex"));
        assert_eq!(input.executable_path, None);
        assert_eq!(
            input.windows_executable_path,
            Some(PathBuf::from("OpenAI.Codex_2p2nqsd0c76g0!App"))
        );
    }

    #[test]
    fn rejects_unknown_profile_fields() {
        let dir = temp_dir();
        fs::create_dir_all(&dir).expect("create temp dir");
        let path = dir.join("launch.json");
        fs::write(
            &path,
            r#"{
  "agent": "codex",
  "providerProfileId": "openai"
}"#,
        )
        .expect("write profile");

        let error = format!("{:#}", load_launch_profile_path(&path).unwrap_err());
        let _ = fs::remove_dir_all(&dir);

        assert!(error.contains("unknown field"));
    }

    #[test]
    fn rejects_unsupported_schema_version() {
        let dir = temp_dir();
        fs::create_dir_all(&dir).expect("create temp dir");
        let path = dir.join("launch.json");
        fs::write(
            &path,
            r#"{
  "schemaVersion": 2,
  "agent": "codex"
}"#,
        )
        .expect("write profile");

        let error = load_launch_profile_path(&path).unwrap_err().to_string();
        let _ = fs::remove_dir_all(&dir);

        assert!(error.contains("unsupported launch profile schemaVersion 2"));
    }

    fn temp_dir() -> PathBuf {
        std::env::temp_dir().join(format!("va-launch-profile-test-{}", uuid::Uuid::new_v4()))
    }

    fn restore_env(key: &str, previous: Option<std::ffi::OsString>) {
        match previous {
            Some(value) => std::env::set_var(key, value),
            None => std::env::remove_var(key),
        }
    }
}
