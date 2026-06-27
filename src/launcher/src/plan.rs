use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::Serialize;

use crate::{
    default_command_for_agent, native_resume_args, resolve_agent_launch_command,
    resolve_executable_path, resolve_terminal_choice, resolve_workspace_path, NativeLaunchInput,
    TerminalChoice,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutionPlan {
    pub agent: String,
    pub profile_id: Option<String>,
    pub launch_target: Option<String>,
    pub terminal: TerminalChoice,
    pub command: String,
    pub args: Vec<String>,
    #[cfg_attr(not(target_os = "windows"), allow(dead_code))]
    pub windows_executable_path: Option<PathBuf>,
    pub workspace: PathBuf,
    pub env: BTreeMap<String, String>,
    pub cleanup_paths: Vec<PathBuf>,
    pub window_label: String,
    pub macos_app_probe: Option<String>,
    pub windows_process_probe: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PublicExecutionPlan {
    pub agent: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub profile_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub launch_target: Option<String>,
    pub terminal: String,
    pub command: String,
    pub args: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub windows_executable_path: Option<PathBuf>,
    pub workspace: PathBuf,
    pub env: BTreeMap<String, String>,
    pub cleanup_paths: Vec<PathBuf>,
    pub window_label: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub macos_app_probe: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub windows_process_probe: Option<String>,
}

pub fn build_execution_plan(input: NativeLaunchInput) -> anyhow::Result<ExecutionPlan> {
    let workspace = resolve_workspace_path(input.workspace)?;
    let terminal = resolve_terminal_choice(input.terminal)?;

    let (command, mut args) = if let Some(path) = input.executable_path {
        let path = resolve_executable_path(path)?;
        (path.to_string_lossy().to_string(), Vec::new())
    } else if let Some(command) = input.command {
        let command = resolve_agent_launch_command(&input.agent, &command)?;
        (command, Vec::new())
    } else if let Some(session_id) = input.session_id.as_deref() {
        let (command, args) = native_resume_args(&input.agent, session_id)?;
        let command = resolve_agent_launch_command(&input.agent, &command)?;
        (command, args)
    } else {
        let command = default_command_for_agent(&input.agent)?;
        let command = resolve_agent_launch_command(&input.agent, &command)?;
        (command, Vec::new())
    };
    args.extend(input.args.native);

    let window_label = input.window_label.unwrap_or_else(|| {
        input
            .profile_id
            .clone()
            .unwrap_or_else(|| format!("{} launch", input.agent))
    });

    Ok(ExecutionPlan {
        agent: input.agent,
        profile_id: input.profile_id,
        launch_target: input.launch_target,
        terminal,
        command,
        args,
        windows_executable_path: input.windows_executable_path,
        workspace,
        env: input.env,
        cleanup_paths: input.cleanup_paths,
        window_label,
        macos_app_probe: input.macos_app_probe,
        windows_process_probe: input.windows_process_probe,
    })
}

pub fn redacted_execution_plan(plan: &ExecutionPlan) -> PublicExecutionPlan {
    PublicExecutionPlan {
        agent: plan.agent.clone(),
        profile_id: plan.profile_id.clone(),
        launch_target: plan.launch_target.clone(),
        terminal: plan.terminal.id().to_string(),
        command: plan.command.clone(),
        args: plan.args.clone(),
        windows_executable_path: plan.windows_executable_path.clone(),
        workspace: plan.workspace.clone(),
        env: plan
            .env
            .iter()
            .map(|(key, value)| {
                let value = if should_redact_env_value(key) {
                    "<redacted>".to_string()
                } else {
                    value.clone()
                };
                (key.clone(), value)
            })
            .collect(),
        cleanup_paths: plan.cleanup_paths.clone(),
        window_label: plan.window_label.clone(),
        macos_app_probe: plan.macos_app_probe.clone(),
        windows_process_probe: plan.windows_process_probe.clone(),
    }
}

fn should_redact_env_value(key: &str) -> bool {
    let normalized = key.to_ascii_uppercase();
    [
        "API_KEY",
        "TOKEN",
        "SECRET",
        "PASSWORD",
        "AUTH",
        "CREDENTIAL",
        "BEARER",
    ]
    .iter()
    .any(|needle| normalized.contains(needle))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::NativeLaunchArgs;

    fn input(agent: &str) -> NativeLaunchInput {
        let workspace = temp_workspace();
        NativeLaunchInput {
            schema_version: 1,
            agent: agent.to_string(),
            profile_id: None,
            launch_target: None,
            workspace: Some(workspace),
            session_id: None,
            terminal: Some(TerminalChoice::Terminal),
            command: None,
            executable_path: None,
            windows_executable_path: None,
            window_label: None,
            env: BTreeMap::new(),
            args: NativeLaunchArgs::default(),
            cleanup_paths: Vec::new(),
            macos_app_probe: None,
            windows_process_probe: None,
        }
    }

    fn temp_workspace() -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "va-launch-plan-workspace-test-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&path).expect("create temp workspace");
        path
    }

    #[test]
    fn builds_default_cli_command() {
        let _guard = crate::env_test_lock().lock().expect("env test lock");
        let fixture = PathFixture::with_command("codex");
        let expected_command = fixture.dir.join("codex").to_string_lossy().to_string();

        let plan = build_execution_plan(input("codex")).unwrap();

        drop(fixture);
        assert_eq!(plan.command, expected_command);
        assert!(plan.args.is_empty());
    }

    #[test]
    fn builds_resume_command_when_session_id_is_present() {
        let _guard = crate::env_test_lock().lock().expect("env test lock");
        let fixture = PathFixture::with_command("codex");
        let expected_command = fixture.dir.join("codex").to_string_lossy().to_string();
        let mut input = input("codex");
        input.session_id = Some("abc123".to_string());

        let plan = build_execution_plan(input).unwrap();

        drop(fixture);
        assert_eq!(plan.command, expected_command);
        assert_eq!(plan.args, vec!["resume", "abc123"]);
    }

    #[test]
    fn explicit_command_wins_over_default_resume_command() {
        let _guard = crate::env_test_lock().lock().expect("env test lock");
        let fixture = PathFixture::with_command("custom-codex");
        let expected_command = fixture
            .dir
            .join("custom-codex")
            .to_string_lossy()
            .to_string();
        let mut input = input("codex");
        input.session_id = Some("abc123".to_string());
        input.command = Some("custom-codex".to_string());
        input.args.native = vec!["resume".to_string(), "abc123".to_string()];

        let plan = build_execution_plan(input).unwrap();

        drop(fixture);
        assert_eq!(plan.command, expected_command);
        assert_eq!(plan.args, vec!["resume", "abc123"]);
    }

    #[test]
    fn redacts_secret_like_env_values() {
        let mut input = input("claude");
        input.executable_path = Some(std::env::current_exe().expect("current test exe"));
        input
            .env
            .insert("ANTHROPIC_API_KEY".into(), "secret".into());
        input.env.insert("NO_PROXY".into(), "localhost".into());
        let plan = build_execution_plan(input).unwrap();

        let public = redacted_execution_plan(&plan);

        assert_eq!(public.env["ANTHROPIC_API_KEY"], "<redacted>");
        assert_eq!(public.env["NO_PROXY"], "localhost");
    }

    #[test]
    fn rejects_missing_agent_executable() {
        let _guard = crate::env_test_lock().lock().expect("env test lock");
        let fixture = PathFixture::empty();

        let error = build_execution_plan(input("codex"))
            .unwrap_err()
            .to_string();

        drop(fixture);
        assert!(error.contains("agent executable 'codex' was not found in PATH"));
    }

    #[test]
    fn rejects_missing_workspace() {
        let mut input = input("codex");
        input.workspace = Some(std::env::temp_dir().join(format!(
            "va-launch-missing-workspace-test-{}",
            uuid::Uuid::new_v4()
        )));

        let error = build_execution_plan(input).unwrap_err().to_string();

        assert!(error.contains("workspace does not exist"));
    }

    struct PathFixture {
        dir: PathBuf,
        previous_path: Option<std::ffi::OsString>,
        previous_data_dir: Option<std::ffi::OsString>,
    }

    impl PathFixture {
        fn empty() -> Self {
            let dir = std::env::temp_dir()
                .join(format!("va-launch-plan-path-test-{}", uuid::Uuid::new_v4()));
            std::fs::create_dir_all(&dir).expect("create temp path dir");
            let previous_path = std::env::var_os("PATH");
            let previous_data_dir = std::env::var_os("VIBEAROUND_DATA_DIR");
            std::env::set_var("PATH", &dir);
            std::env::set_var("VIBEAROUND_DATA_DIR", &dir);
            Self {
                dir,
                previous_path,
                previous_data_dir,
            }
        }

        fn with_command(name: &str) -> Self {
            let fixture = Self::empty();
            let path = fixture.dir.join(name);
            std::fs::write(&path, "#!/bin/sh\n").expect("write fake command");
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700))
                    .expect("chmod fake command");
            }
            fixture
        }
    }

    impl Drop for PathFixture {
        fn drop(&mut self) {
            match &self.previous_path {
                Some(value) => std::env::set_var("PATH", value),
                None => std::env::remove_var("PATH"),
            }
            match &self.previous_data_dir {
                Some(value) => std::env::set_var("VIBEAROUND_DATA_DIR", value),
                None => std::env::remove_var("VIBEAROUND_DATA_DIR"),
            }
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }
}
