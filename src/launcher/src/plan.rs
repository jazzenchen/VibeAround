use std::collections::BTreeMap;
use std::path::PathBuf;

use anyhow::Context;
use serde::Serialize;

use crate::{
    default_command_for_agent, native_resume_args, resolve_terminal_choice, NativeLaunchInput,
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
    let workspace = match input.workspace {
        Some(path) => path,
        None => std::env::current_dir().context("resolve current directory")?,
    };
    let terminal = resolve_terminal_choice(input.terminal)?;

    let (command, mut args) = if let Some(path) = input.executable_path {
        (path.to_string_lossy().to_string(), Vec::new())
    } else if let Some(command) = input.command {
        (command, Vec::new())
    } else if let Some(session_id) = input.session_id.as_deref() {
        native_resume_args(&input.agent, session_id)?
    } else {
        (default_command_for_agent(&input.agent)?, Vec::new())
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
        NativeLaunchInput {
            schema_version: 1,
            agent: agent.to_string(),
            profile_id: None,
            launch_target: None,
            workspace: Some(PathBuf::from("/tmp/workspace")),
            session_id: None,
            terminal: Some(TerminalChoice::Terminal),
            command: None,
            executable_path: None,
            window_label: None,
            env: BTreeMap::new(),
            args: NativeLaunchArgs::default(),
            cleanup_paths: Vec::new(),
            macos_app_probe: None,
            windows_process_probe: None,
        }
    }

    #[test]
    fn builds_default_cli_command() {
        let plan = build_execution_plan(input("codex")).unwrap();
        assert_eq!(plan.command, "codex");
        assert!(plan.args.is_empty());
    }

    #[test]
    fn builds_resume_command_when_session_id_is_present() {
        let mut input = input("codex");
        input.session_id = Some("abc123".to_string());

        let plan = build_execution_plan(input).unwrap();

        assert_eq!(plan.command, "codex");
        assert_eq!(plan.args, vec!["resume", "abc123"]);
    }

    #[test]
    fn explicit_command_wins_over_default_resume_command() {
        let mut input = input("codex");
        input.session_id = Some("abc123".to_string());
        input.command = Some("custom-codex".to_string());
        input.args.native = vec!["resume".to_string(), "abc123".to_string()];

        let plan = build_execution_plan(input).unwrap();

        assert_eq!(plan.command, "custom-codex");
        assert_eq!(plan.args, vec!["resume", "abc123"]);
    }

    #[test]
    fn redacts_secret_like_env_values() {
        let mut input = input("claude");
        input
            .env
            .insert("ANTHROPIC_API_KEY".into(), "secret".into());
        input.env.insert("NO_PROXY".into(), "localhost".into());
        let plan = build_execution_plan(input).unwrap();

        let public = redacted_execution_plan(&plan);

        assert_eq!(public.env["ANTHROPIC_API_KEY"], "<redacted>");
        assert_eq!(public.env["NO_PROXY"], "localhost");
    }
}
