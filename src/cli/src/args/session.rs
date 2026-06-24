use clap::{Args, Subcommand};
use va_client::sessions::PtyTool;

use super::Command;
use crate::error::CliError;

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct SessionCreateArgs {
    pub(crate) tool: Option<PtyTool>,
    pub(crate) profile_id: Option<String>,
    pub(crate) launch_target: Option<String>,
    pub(crate) resume_session_id: Option<String>,
    pub(crate) project_path: Option<String>,
    pub(crate) tmux_session: Option<String>,
    pub(crate) theme: Option<String>,
    pub(crate) cols: Option<u16>,
    pub(crate) rows: Option<u16>,
    pub(crate) attach: bool,
}

#[derive(Debug, Subcommand)]
#[command(rename_all = "kebab-case")]
pub(super) enum SessionCommand {
    Create(SessionCreateCli),
    Attach { session_id: String },
    Kill { session_id: String },
}

#[derive(Debug, Args)]
pub(super) struct SessionCreateCli {
    #[arg(long, value_parser = parse_tool)]
    tool: Option<PtyTool>,
    #[arg(value_name = "TOOL", value_parser = parse_tool)]
    positional_tool: Option<PtyTool>,
    #[arg(long = "profile", alias = "profile-id")]
    profile_id: Option<String>,
    #[arg(long = "target", alias = "launch-target")]
    launch_target: Option<String>,
    #[arg(long = "resume", aliases = ["resume-session", "resume-session-id"])]
    resume_session_id: Option<String>,
    #[arg(long = "project", aliases = ["project-path", "cwd"])]
    project_path: Option<String>,
    #[arg(long = "tmux", alias = "tmux-session")]
    tmux_session: Option<String>,
    #[arg(
        long,
        default_value_t = false,
        action = clap::ArgAction::Set,
        num_args = 0..=1,
        require_equals = true,
        default_missing_value = "true",
        value_parser = clap::builder::BoolishValueParser::new()
    )]
    attach: bool,
    #[arg(long)]
    theme: Option<String>,
    #[arg(long, value_parser = parse_positive_u16)]
    cols: Option<u16>,
    #[arg(long, value_parser = parse_positive_u16)]
    rows: Option<u16>,
}

pub(super) fn command_into_command(command: SessionCommand) -> Result<Command, CliError> {
    Ok(match command {
        SessionCommand::Create(args) => Command::SessionCreate(args.into_args()?),
        SessionCommand::Attach { session_id } => Command::SessionAttach { session_id },
        SessionCommand::Kill { session_id } => Command::SessionKill { session_id },
    })
}

impl SessionCreateCli {
    fn into_args(self) -> Result<SessionCreateArgs, CliError> {
        if self.tool.is_some() && self.positional_tool.is_some() {
            return Err(CliError::Usage(
                "session create cannot specify TOOL both positionally and with --tool".into(),
            ));
        }
        let mut create = SessionCreateArgs {
            tool: self.tool.or(self.positional_tool),
            profile_id: self.profile_id,
            launch_target: self.launch_target,
            resume_session_id: self.resume_session_id,
            project_path: self.project_path,
            tmux_session: self.tmux_session,
            theme: self.theme,
            cols: self.cols,
            rows: self.rows,
            attach: self.attach,
        };

        if create.profile_id.is_some() != create.launch_target.is_some() {
            return Err(CliError::Usage(
                "session create requires --profile and --target together".into(),
            ));
        }
        if create.profile_id.is_some() && create.tool.is_some() {
            return Err(CliError::Usage(
                "session create cannot combine --tool with --profile/--target".into(),
            ));
        }
        if create.profile_id.is_some() && create.tmux_session.is_some() {
            return Err(CliError::Usage(
                "session create cannot combine --tmux with --profile/--target".into(),
            ));
        }
        if create.resume_session_id.is_some() && create.tmux_session.is_some() {
            return Err(CliError::Usage(
                "session create cannot combine --resume with --tmux".into(),
            ));
        }
        if matches!(create.tool, Some(PtyTool::Generic)) && create.resume_session_id.is_some() {
            return Err(CliError::Usage(
                "session create --resume requires a coding-agent tool".into(),
            ));
        }
        if create.tmux_session.is_some() {
            match create.tool {
                None => create.tool = Some(PtyTool::Generic),
                Some(PtyTool::Generic) => {}
                Some(_) => {
                    return Err(CliError::Usage(
                        "session create --tmux must use --tool generic".into(),
                    ));
                }
            }
        }
        if create.profile_id.is_none() && create.tool.is_none() {
            return Err(CliError::Usage(
                "session create requires --tool TOOL, or --profile PROFILE --target TARGET".into(),
            ));
        }

        Ok(create)
    }
}

fn parse_tool(value: &str) -> Result<PtyTool, String> {
    match value {
        "generic" => Ok(PtyTool::Generic),
        "claude" => Ok(PtyTool::Claude),
        "codex" => Ok(PtyTool::Codex),
        "pi" => Ok(PtyTool::Pi),
        "gemini" => Ok(PtyTool::Gemini),
        "opencode" | "open-code" => Ok(PtyTool::OpenCode),
        "cursor" => Ok(PtyTool::Cursor),
        "kiro" => Ok(PtyTool::Kiro),
        "qwen-code" | "qwen" => Ok(PtyTool::QwenCode),
        _ => Err(format!("unknown PTY tool: {value}")),
    }
}

fn parse_positive_u16(value: &str) -> Result<u16, String> {
    let value = value
        .parse::<u16>()
        .map_err(|_| "must be a positive integer".to_string())?;
    if value == 0 {
        return Err("must be a positive integer".into());
    }
    Ok(value)
}
