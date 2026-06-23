use clap::{Args, Subcommand};

use super::Command;
use crate::error::CliError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ChatSendArgs {
    pub(crate) text: String,
    pub(crate) agent: Option<String>,
    pub(crate) profile_id: Option<String>,
    pub(crate) resume_session_id: Option<String>,
    pub(crate) new_session: bool,
    pub(crate) workspace_path: Option<String>,
    pub(crate) permission_mode: Option<String>,
}

#[derive(Debug, Subcommand)]
#[command(rename_all = "kebab-case")]
pub(super) enum ChatCommand {
    Send(ChatSendCli),
}

#[derive(Debug, Args)]
pub(super) struct ChatSendCli {
    #[arg(value_name = "TEXT", required = true, num_args = 1..)]
    text: Vec<String>,
    #[arg(long = "agent", short = 'a')]
    agent: Option<String>,
    #[arg(long = "profile", alias = "profile-id")]
    profile_id: Option<String>,
    #[arg(long = "resume", aliases = ["session", "session-id"])]
    resume_session_id: Option<String>,
    #[arg(long = "new-session", alias = "new")]
    new_session: bool,
    #[arg(long = "workspace", aliases = ["workspace-path", "cwd"])]
    workspace_path: Option<String>,
    #[arg(long = "permission-mode", aliases = ["mode", "mode-id"])]
    permission_mode: Option<String>,
}

pub(super) fn command_into_command(command: ChatCommand) -> Result<Command, CliError> {
    Ok(match command {
        ChatCommand::Send(args) => Command::ChatSend(args.into_args()?),
    })
}

impl ChatSendCli {
    fn into_args(self) -> Result<ChatSendArgs, CliError> {
        if self.new_session && self.resume_session_id.is_some() {
            return Err(CliError::Usage(
                "chat send cannot combine --new-session with --resume".into(),
            ));
        }
        if self.workspace_path.is_some() && !self.new_session && self.resume_session_id.is_none() {
            return Err(CliError::Usage(
                "chat send --workspace requires --new-session or --resume".into(),
            ));
        }
        let text = self.text.join(" ").trim().to_string();
        if text.is_empty() {
            return Err(CliError::Usage("chat send requires TEXT".into()));
        }
        Ok(ChatSendArgs {
            text,
            agent: self.agent,
            profile_id: self.profile_id,
            resume_session_id: self.resume_session_id,
            new_session: self.new_session,
            workspace_path: self.workspace_path,
            permission_mode: self.permission_mode,
        })
    }
}
