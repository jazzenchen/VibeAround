use std::path::Path;

use anyhow::Context;

pub fn install_for_launch(agent_id: &str, workspace: &Path) -> anyhow::Result<()> {
    let integration_agent_id = project_integration_agent_id(agent_id);
    common::agent::sync_project_skills(integration_agent_id, workspace)
        .with_context(|| format!("sync project skills for {}", integration_agent_id))?;
    common::agent::install_project_mcp(integration_agent_id, workspace)
        .with_context(|| format!("install project MCP for {}", integration_agent_id))?;
    Ok(())
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
        write_mcp_auth_file(&data_dir, 12358);
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

    #[test]
    fn syncs_skills_without_an_mcp_credential() {
        let _guard = crate::env_test_lock().lock().expect("env test lock");
        let dir = temp_dir();
        let workspace = dir.join("workspace");
        let data_dir = dir.join("data");
        std::fs::create_dir_all(&workspace).expect("create workspace");
        std::fs::create_dir_all(&data_dir).expect("create data dir");
        let previous = std::env::var_os("VIBEAROUND_DATA_DIR");
        std::env::set_var("VIBEAROUND_DATA_DIR", &data_dir);
        common::config::reload();

        install_for_launch("codex", &workspace).expect("sync skills");

        restore_env("VIBEAROUND_DATA_DIR", previous);
        common::config::reload();
        assert!(workspace
            .join(".agents")
            .join("skills")
            .join("vibearound")
            .exists());
        assert!(!workspace.join(".codex").join("config.toml").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn temp_dir() -> PathBuf {
        std::env::temp_dir().join(format!(
            "va-launch-project-integration-test-{}",
            uuid::Uuid::new_v4()
        ))
    }

    fn write_mcp_auth_file(data_dir: &Path, port: u16) {
        std::fs::write(
            data_dir.join("auth.json"),
            format!(r#"{{"port":{port},"token":"dashboard","mcp_token":"test-token"}}"#),
        )
        .expect("write auth file");
    }

    fn restore_env(key: &str, previous: Option<std::ffi::OsString>) {
        match previous {
            Some(value) => std::env::set_var(key, value),
            None => std::env::remove_var(key),
        }
    }
}
