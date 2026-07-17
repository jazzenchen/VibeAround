use std::path::{Path, PathBuf};

use common::agent_state::{self, AgentExecutablePreference};
use serde_json::{Map, Value};

pub fn resolve_configured_agent_executable(agent_id: &str) -> anyhow::Result<Option<PathBuf>> {
    let config = common::config::read_settings_json().map_err(anyhow::Error::msg)?;
    Ok(agent_entry(&config, agent_id).and_then(executable_path_from_entry))
}

pub fn write_scanned_agent_executable(agent_id: &str, path: &Path) -> anyhow::Result<()> {
    agent_state::write_agent_executable(
        agent_id,
        Some(AgentExecutablePreference::path_scan(path.to_path_buf())),
    )
}

fn agent_entry<'a>(config: &'a Value, agent_id: &str) -> Option<&'a Map<String, Value>> {
    config
        .get("launcher")?
        .as_object()?
        .get("agents")?
        .as_object()?
        .get(agent_id)?
        .as_object()
}

fn executable_path_from_entry(entry: &Map<String, Value>) -> Option<PathBuf> {
    entry
        .get("executable")
        .and_then(|value| value.as_object())
        .and_then(|value| value.get("path"))
        .and_then(|value| value.as_str())
        .filter(|value| !value.trim().is_empty())
        .map(PathBuf::from)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn reads_configured_executable_from_settings() {
        let _guard = crate::env_test_lock().lock().expect("env test lock");
        let dir = temp_dir();
        fs::create_dir_all(&dir).expect("create temp dir");
        fs::write(
            dir.join("settings.json"),
            r#"{
  "launcher": {
    "agents": {
      "codex": {
        "executable": {
          "path": "/opt/homebrew/bin/codex",
          "source": "npm_global",
          "source_label": "npm global",
          "rank": 2000
        }
      }
    }
  }
}"#,
        )
        .expect("write settings config");
        let previous = std::env::var_os("VIBEAROUND_DATA_DIR");
        std::env::set_var("VIBEAROUND_DATA_DIR", &dir);

        let path = resolve_configured_agent_executable("codex").expect("read config");

        restore_env("VIBEAROUND_DATA_DIR", previous);
        let _ = fs::remove_dir_all(&dir);

        assert_eq!(path, Some(PathBuf::from("/opt/homebrew/bin/codex")));
    }

    #[test]
    fn writes_scanned_executable_without_dropping_other_settings() {
        let _guard = crate::env_test_lock().lock().expect("env test lock");
        let dir = temp_dir();
        fs::create_dir_all(&dir).expect("create temp dir");
        fs::write(
            dir.join("settings.json"),
            r#"{
  "enabled_agents": ["codex"],
  "launcher": {
    "selected_agent": "codex",
    "agents": {
      "codex": {
        "profile_id": "openai"
      }
    }
  }
}"#,
        )
        .expect("write settings config");
        let bin = dir.join("codex");
        fs::write(&bin, "#!/bin/sh\n").expect("write fake bin");
        let previous = std::env::var_os("VIBEAROUND_DATA_DIR");
        std::env::set_var("VIBEAROUND_DATA_DIR", &dir);

        write_scanned_agent_executable("codex", &bin).expect("write scanned executable");
        let body = fs::read_to_string(dir.join("settings.json")).expect("read settings config");
        let value: Value = serde_json::from_str(&body).expect("parse settings config");
        let agents_json_exists = dir.join("agents.json").exists();

        restore_env("VIBEAROUND_DATA_DIR", previous);
        let _ = fs::remove_dir_all(&dir);

        assert_eq!(value["enabled_agents"][0], "codex");
        assert_eq!(value["launcher"]["selected_agent"], "codex");
        assert_eq!(value["launcher"]["agents"]["codex"]["profile_id"], "openai");
        assert_eq!(
            value["launcher"]["agents"]["codex"]["executable"]["path"].as_str(),
            Some(bin.to_string_lossy().as_ref())
        );
        assert_eq!(
            value["launcher"]["agents"]["codex"]["executable"]["source"],
            "path_scan"
        );
        assert!(!agents_json_exists);
    }

    #[test]
    fn reads_settings_from_core_expanded_data_dir() {
        let _guard = crate::env_test_lock().lock().expect("env test lock");
        let home = temp_dir();
        fs::create_dir_all(&home).expect("create temp home");
        fs::write(
            home.join("settings.json"),
            r#"{
  "launcher": {
    "agents": {
      "codex": {
        "executable": { "path": "/usr/local/bin/codex" }
      }
    }
  }
}"#,
        )
        .expect("write settings config");
        let previous_home = std::env::var_os("HOME");
        let previous_data_dir = std::env::var_os("VIBEAROUND_DATA_DIR");
        std::env::set_var("HOME", &home);
        std::env::set_var("VIBEAROUND_DATA_DIR", "~");

        let path = resolve_configured_agent_executable("codex").expect("read config");

        restore_env("HOME", previous_home);
        restore_env("VIBEAROUND_DATA_DIR", previous_data_dir);
        let _ = fs::remove_dir_all(&home);

        assert_eq!(path, Some(PathBuf::from("/usr/local/bin/codex")));
    }

    fn temp_dir() -> PathBuf {
        std::env::temp_dir().join(format!(
            "va-launch-agents-config-test-{}",
            uuid::Uuid::new_v4()
        ))
    }

    fn restore_env(key: &str, previous: Option<std::ffi::OsString>) {
        match previous {
            Some(value) => std::env::set_var(key, value),
            None => std::env::remove_var(key),
        }
    }
}
