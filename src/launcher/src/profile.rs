use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::Context;
use serde::{Deserialize, Serialize};

use crate::{paths, NativeLaunchArgs, NativeLaunchInput, TerminalChoice};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
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
    pub window_label: Option<String>,
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    #[serde(default)]
    pub args: NativeLaunchArgs,
    #[serde(default)]
    pub macos_app_probe: Option<String>,
    #[serde(default)]
    pub windows_process_probe: Option<String>,
}

impl LaunchProfile {
    pub fn into_native_input(self, selected_id: Option<String>) -> NativeLaunchInput {
        NativeLaunchInput {
            schema_version: self.schema_version.unwrap_or(1),
            agent: self.agent,
            profile_id: self.profile_id.or(self.id).or(selected_id),
            launch_target: self.launch_target,
            workspace: self.workspace,
            session_id: self.session_id,
            terminal: self.terminal,
            command: self.command,
            executable_path: self.executable_path,
            window_label: self.window_label,
            env: self.env,
            args: self.args,
            cleanup_paths: Vec::new(),
            macos_app_probe: self.macos_app_probe,
            windows_process_probe: self.windows_process_probe,
        }
    }
}

pub fn load_launch_profile(name: &str) -> anyhow::Result<NativeLaunchInput> {
    let path = paths::launch_profile_path(name)?;
    let profile = read_profile_file(&path)?;
    Ok(profile.into_native_input(Some(name.to_string())))
}

pub fn load_launch_profile_path(path: &Path) -> anyhow::Result<NativeLaunchInput> {
    let profile = read_profile_file(path)?;
    let selected_id = path
        .file_stem()
        .and_then(|name| name.to_str())
        .filter(|name| !name.trim().is_empty())
        .map(ToString::to_string);
    Ok(profile.into_native_input(selected_id))
}

fn read_profile_file(path: &Path) -> anyhow::Result<LaunchProfile> {
    let body = fs::read_to_string(path)
        .with_context(|| format!("read launch profile {}", path.display()))?;
    serde_json::from_str(&body).with_context(|| format!("parse launch profile {}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn named_profile_loads_from_shared_profile_dir() {
        let _guard = crate::env_test_lock().lock().expect("env test lock");
        let dir = temp_dir();
        let profile_dir = dir.join("launch").join("profiles");
        fs::create_dir_all(&profile_dir).expect("create profile dir");
        fs::write(
            profile_dir.join("codex-work.json"),
            r#"{
  "agent": "codex",
  "workspace": "/tmp/work",
  "env": {
    "OPENAI_API_KEY": "secret"
  },
  "args": {
    "native": ["--model", "gpt-5"]
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
        assert_eq!(input.workspace, Some(PathBuf::from("/tmp/work")));
        assert_eq!(input.env["OPENAI_API_KEY"], "secret");
        assert_eq!(input.args.native, vec!["--model", "gpt-5"]);
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
