//! Native launch input and planning for the `@va/launcher` boundary.
//!
//! This crate intentionally owns host-native terminal/app launch details. It
//! does not call VibeAround server launcher APIs and should not grow a
//! server-side `LaunchPlan` contract.

mod plan;

use std::collections::BTreeMap;
use std::path::PathBuf;

use anyhow::{anyhow, bail};
use clap::ValueEnum;
use serde::{Deserialize, Serialize};

pub use plan::{build_execution_plan, redacted_execution_plan, ExecutionPlan, PublicExecutionPlan};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NativeLaunchInput {
    pub schema_version: u32,
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
    pub cleanup_paths: Vec<PathBuf>,
    #[serde(default)]
    pub macos_app_probe: Option<String>,
    #[serde(default)]
    pub windows_process_probe: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NativeLaunchArgs {
    #[serde(default)]
    pub native: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ValueEnum)]
#[serde(rename_all = "kebab-case")]
#[clap(rename_all = "kebab-case")]
pub enum TerminalChoice {
    SystemTerminal,
    Terminal,
    Iterm2,
    #[serde(rename = "powershell", alias = "power-shell")]
    #[value(name = "powershell", alias = "power-shell")]
    PowerShell,
    GnomeTerminal,
    Konsole,
    XfceTerminal,
    Xterm,
    Kitty,
    Alacritty,
    #[serde(rename = "wezterm", alias = "wez-term")]
    #[value(name = "wezterm", alias = "wez-term")]
    WezTerm,
}

impl TerminalChoice {
    pub fn default_for_current_platform() -> Self {
        if cfg!(target_os = "macos") {
            Self::Terminal
        } else if cfg!(target_os = "windows") {
            Self::PowerShell
        } else {
            Self::SystemTerminal
        }
    }

    pub fn id(self) -> &'static str {
        match self {
            Self::SystemTerminal => "system-terminal",
            Self::Terminal => "terminal",
            Self::Iterm2 => "iterm2",
            Self::PowerShell => "powershell",
            Self::GnomeTerminal => "gnome-terminal",
            Self::Konsole => "konsole",
            Self::XfceTerminal => "xfce-terminal",
            Self::Xterm => "xterm",
            Self::Kitty => "kitty",
            Self::Alacritty => "alacritty",
            Self::WezTerm => "wezterm",
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LaunchOutput {
    pub status: LaunchStatus,
    pub plan: PublicExecutionPlan,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum LaunchStatus {
    DryRun,
}

pub fn dry_run(input: NativeLaunchInput) -> anyhow::Result<LaunchOutput> {
    let plan = build_execution_plan(input)?;
    Ok(LaunchOutput {
        status: LaunchStatus::DryRun,
        plan: redacted_execution_plan(&plan),
    })
}

pub fn parse_env_pair(raw: &str) -> anyhow::Result<(String, String)> {
    let Some((key, value)) = raw.split_once('=') else {
        bail!("env value must be KEY=VALUE");
    };
    let key = key.trim();
    if !is_valid_env_key(key) {
        bail!("invalid env key '{}'", key);
    }
    Ok((key.to_string(), value.to_string()))
}

pub fn native_resume_args(agent: &str, session_id: &str) -> anyhow::Result<(String, Vec<String>)> {
    let command = match agent {
        "claude" => (
            "claude",
            vec!["--resume", session_id, "--permission-mode", "acceptEdits"],
        ),
        "codex" => ("codex", vec!["resume", session_id]),
        "pi" => ("pi", vec!["--session", session_id]),
        "gemini" => ("gemini", vec!["--resume", session_id]),
        "opencode" => ("opencode", vec!["--session", session_id]),
        "cursor" => ("cursor-agent", vec!["--resume", session_id]),
        "qwen-code" => ("qwen", vec!["--resume", session_id]),
        other => bail!("resume launch is not supported for agent '{}'", other),
    };
    Ok((
        command.0.to_string(),
        command.1.into_iter().map(str::to_string).collect(),
    ))
}

pub fn default_command_for_agent(agent: &str) -> anyhow::Result<String> {
    match agent {
        "claude" => Ok("claude".to_string()),
        "codex" => Ok("codex".to_string()),
        "pi" => Ok("pi".to_string()),
        "gemini" => Ok("gemini".to_string()),
        "opencode" => Ok("opencode".to_string()),
        "cursor" => Ok("cursor-agent".to_string()),
        "kiro" => Ok("kiro-cli".to_string()),
        "qwen-code" => Ok("qwen".to_string()),
        other => Err(anyhow!(
            "agent '{}' needs an explicit command or executablePath",
            other
        )),
    }
}

fn is_valid_env_key(key: &str) -> bool {
    !key.is_empty()
        && key
            .chars()
            .next()
            .is_some_and(|ch| ch.is_ascii_alphabetic() || ch == '_')
        && key
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_env_pair() {
        assert_eq!(
            parse_env_pair("ANTHROPIC_API_KEY=abc").unwrap(),
            ("ANTHROPIC_API_KEY".to_string(), "abc".to_string())
        );
    }

    #[test]
    fn rejects_invalid_env_pair() {
        assert!(parse_env_pair("1BAD=value").is_err());
        assert!(parse_env_pair("NO_EQUALS").is_err());
    }

    #[test]
    fn native_resume_args_match_existing_desktop_routes() {
        let (command, args) = native_resume_args("codex", "session-123").unwrap();
        assert_eq!(command, "codex");
        assert_eq!(args, vec!["resume", "session-123"]);
    }
}
