use std::path::Path;

use anyhow::Context;

pub fn install_for_launch(agent_id: &str, workspace: &Path) -> anyhow::Result<()> {
    let integration_agent_id = project_integration_agent_id(agent_id);
    common::agent::auto_install_project_integrations(integration_agent_id, workspace)
        .with_context(|| format!("install project integrations for {}", integration_agent_id))
}

fn project_integration_agent_id(agent_id: &str) -> &str {
    match agent_id {
        "claude-desktop" => "claude",
        "codex-desktop" => "codex",
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    #[test]
    fn desktop_agents_install_companion_cli_integrations() {
        assert_eq!(project_integration_agent_id("codex-desktop"), "codex");
        assert_eq!(project_integration_agent_id("claude-desktop"), "claude");
        assert_eq!(project_integration_agent_id("gemini"), "gemini");
    }

    #[test]
    fn installs_project_scoped_integrations_for_companion_cli() {
        let _guard = crate::env_test_lock().lock().expect("env test lock");
        let dir = temp_dir();
        let workspace = dir.join("workspace");
        let data_dir = dir.join("data");
        std::fs::create_dir_all(&workspace).expect("create workspace");
        std::fs::create_dir_all(&data_dir).expect("create data dir");
        let previous = std::env::var_os("VIBEAROUND_DATA_DIR");
        std::env::set_var("VIBEAROUND_DATA_DIR", &data_dir);
        common::config::reload();

        install_for_launch("codex-desktop", &workspace).expect("install integrations");

        restore_env("VIBEAROUND_DATA_DIR", previous);
        common::config::reload();
        let codex_config = workspace.join(".codex").join("config.toml");
        let codex_skill = workspace
            .join(".agents")
            .join("skills")
            .join("vibearound")
            .join("SKILL.md");

        assert!(codex_config.is_file());
        assert!(codex_skill.is_file());
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn temp_dir() -> PathBuf {
        std::env::temp_dir().join(format!(
            "va-launch-project-integration-test-{}",
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
