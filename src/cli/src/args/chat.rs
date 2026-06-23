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
    pub(crate) continue_session: bool,
    pub(crate) workspace_path: Option<String>,
    pub(crate) permission_mode: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ChatReplArgs {
    pub(crate) agent: Option<String>,
    pub(crate) profile_id: Option<String>,
    pub(crate) resume_session_id: Option<String>,
    pub(crate) new_session: bool,
    pub(crate) continue_session: bool,
    pub(crate) workspace_path: Option<String>,
    pub(crate) permission_mode: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ChatForgetArgs {
    pub(crate) agent: Option<String>,
    pub(crate) profile_id: Option<String>,
    pub(crate) workspace_path: Option<String>,
    pub(crate) all: bool,
}

#[derive(Debug, Subcommand)]
#[command(rename_all = "kebab-case")]
pub(super) enum ChatCommand {
    Send(ChatSendCli),
    Repl(ChatReplCli),
    Sessions,
    Forget(ChatForgetCli),
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
    #[arg(long = "continue", aliases = ["last", "resume-last"])]
    continue_session: bool,
    #[arg(long = "workspace", aliases = ["workspace-path", "cwd"])]
    workspace_path: Option<String>,
    #[arg(long = "permission-mode", aliases = ["mode", "mode-id"])]
    permission_mode: Option<String>,
}

#[derive(Debug, Args)]
pub(super) struct ChatReplCli {
    #[arg(long = "agent", short = 'a')]
    agent: Option<String>,
    #[arg(long = "profile", alias = "profile-id")]
    profile_id: Option<String>,
    #[arg(long = "resume", aliases = ["session", "session-id"])]
    resume_session_id: Option<String>,
    #[arg(long = "new-session", alias = "new")]
    new_session: bool,
    #[arg(long = "continue", aliases = ["last", "resume-last"])]
    continue_session: bool,
    #[arg(long = "workspace", aliases = ["workspace-path", "cwd"])]
    workspace_path: Option<String>,
    #[arg(long = "permission-mode", aliases = ["mode", "mode-id"])]
    permission_mode: Option<String>,
}

#[derive(Debug, Args)]
pub(super) struct ChatForgetCli {
    #[arg(long = "agent", short = 'a')]
    agent: Option<String>,
    #[arg(long = "profile", alias = "profile-id")]
    profile_id: Option<String>,
    #[arg(long = "workspace", aliases = ["workspace-path", "cwd"])]
    workspace_path: Option<String>,
    #[arg(long)]
    all: bool,
}

pub(super) fn command_into_command(command: ChatCommand) -> Result<Command, CliError> {
    Ok(match command {
        ChatCommand::Send(args) => Command::ChatSend(args.into_args()?),
        ChatCommand::Repl(args) => Command::ChatRepl(args.into_args()?),
        ChatCommand::Sessions => Command::ChatSessions,
        ChatCommand::Forget(args) => Command::ChatForget(args.into_args()?),
    })
}

impl ChatSendCli {
    fn into_args(self) -> Result<ChatSendArgs, CliError> {
        validate_session_flags(
            "chat send",
            self.new_session,
            self.resume_session_id.as_ref(),
            self.continue_session,
        )?;
        if self.workspace_path.is_some()
            && !self.new_session
            && self.resume_session_id.is_none()
            && !self.continue_session
        {
            return Err(CliError::Usage(
                "chat send --workspace requires --new-session, --resume, or --continue".into(),
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
            continue_session: self.continue_session,
            workspace_path: self.workspace_path,
            permission_mode: self.permission_mode,
        })
    }
}

impl ChatReplCli {
    fn into_args(self) -> Result<ChatReplArgs, CliError> {
        validate_session_flags(
            "chat repl",
            self.new_session,
            self.resume_session_id.as_ref(),
            self.continue_session,
        )?;
        Ok(ChatReplArgs {
            agent: self.agent,
            profile_id: self.profile_id,
            resume_session_id: self.resume_session_id,
            new_session: self.new_session,
            continue_session: self.continue_session,
            workspace_path: self.workspace_path,
            permission_mode: self.permission_mode,
        })
    }
}

impl ChatForgetCli {
    fn into_args(self) -> Result<ChatForgetArgs, CliError> {
        if self.all
            && (self.agent.is_some() || self.profile_id.is_some() || self.workspace_path.is_some())
        {
            return Err(CliError::Usage(
                "chat forget --all cannot combine with --agent, --profile, or --workspace".into(),
            ));
        }
        Ok(ChatForgetArgs {
            agent: self.agent,
            profile_id: self.profile_id,
            workspace_path: self.workspace_path,
            all: self.all,
        })
    }
}

fn validate_session_flags(
    command: &str,
    new_session: bool,
    resume_session_id: Option<&String>,
    continue_session: bool,
) -> Result<(), CliError> {
    if new_session && resume_session_id.is_some() {
        return Err(CliError::Usage(format!(
            "{command} cannot combine --new-session with --resume"
        )));
    }
    if new_session && continue_session {
        return Err(CliError::Usage(format!(
            "{command} cannot combine --new-session with --continue"
        )));
    }
    if resume_session_id.is_some() && continue_session {
        return Err(CliError::Usage(format!(
            "{command} cannot combine --resume with --continue"
        )));
    }
    Ok(())
}
