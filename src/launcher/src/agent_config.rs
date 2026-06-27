use std::fs;
use std::path::{Path, PathBuf};

use anyhow::Context;
use serde_json::{Map, Value};

use crate::paths;

pub fn resolve_configured_agent_executable(agent_id: &str) -> anyhow::Result<Option<PathBuf>> {
    let path = paths::settings_path()?;
    let config = read_settings_config(&path)?;
    Ok(agent_entry(&config, agent_id).and_then(executable_path_from_entry))
}

pub fn write_scanned_agent_executable(agent_id: &str, path: &Path) -> anyhow::Result<()> {
    let config_path = paths::settings_path()?;
    let mut config = read_settings_config(&config_path)?;
    let root = ensure_object(&mut config);
    let launcher = ensure_child_object(root, "launcher");
    let agents = ensure_child_object(launcher, "agents");
    let agent = ensure_child_object(agents, agent_id);
    let executable = executable_object(path);
    agent.insert("executable".to_string(), Value::Object(executable));
    write_settings_config(&config_path, &config)
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

fn read_settings_config(path: &Path) -> anyhow::Result<Value> {
    let body = match fs::read_to_string(path) {
        Ok(body) => body,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(Value::Object(Map::new()))
        }
        Err(error) => return Err(error).with_context(|| format!("read {}", path.display())),
    };
    serde_json::from_str(&body).with_context(|| format!("parse {}", path.display()))
}

fn write_settings_config(path: &Path, config: &Value) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    }
    let body = serde_json::to_string_pretty(config).context("serialize settings config")?;
    let tmp = path.with_extension("tmp");
    fs::write(&tmp, body).with_context(|| format!("write {}", tmp.display()))?;
    set_owner_only(&tmp).ok();
    fs::rename(&tmp, path)
        .with_context(|| format!("rename {} to {}", tmp.display(), path.display()))?;
    Ok(())
}

fn ensure_object(value: &mut Value) -> &mut Map<String, Value> {
    if !value.is_object() {
        *value = Value::Object(Map::new());
    }
    value.as_object_mut().expect("object just inserted")
}

fn ensure_child_object<'a>(
    parent: &'a mut Map<String, Value>,
    key: &str,
) -> &'a mut Map<String, Value> {
    let value = parent
        .entry(key.to_string())
        .or_insert_with(|| Value::Object(Map::new()));
    if !value.is_object() {
        *value = Value::Object(Map::new());
    }
    value.as_object_mut().expect("object just inserted")
}

fn executable_object(path: &Path) -> Map<String, Value> {
    let mut object = Map::new();
    object.insert(
        "path".to_string(),
        Value::String(path.to_string_lossy().to_string()),
    );
    if let Ok(realpath) = fs::canonicalize(path) {
        object.insert(
            "realpath".to_string(),
            Value::String(realpath.to_string_lossy().to_string()),
        );
    }
    object.insert("source".to_string(), Value::String("path_scan".to_string()));
    object.insert(
        "source_label".to_string(),
        Value::String("PATH scan".to_string()),
    );
    object.insert("rank".to_string(), Value::Number(4000.into()));
    object
}

fn set_owner_only(path: &Path) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    }
    #[cfg(not(unix))]
    {
        let _ = path;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

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
