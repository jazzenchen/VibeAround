use clap::{Args, Subcommand};

use super::Command;
use crate::error::CliError;

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct LaunchSessionsArgs {
    pub(crate) agent_ids: Vec<String>,
    pub(crate) workspace_paths: Vec<String>,
    pub(crate) include_archived: bool,
    pub(crate) limit: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LaunchSessionMutationArgs {
    pub(crate) agent_id: String,
    pub(crate) session_id: String,
    pub(crate) workspace_path: Option<String>,
}

#[derive(Debug, Subcommand)]
#[command(rename_all = "kebab-case")]
pub(super) enum LaunchCommand {
    Sessions(LaunchSessionsCli),
    Archive(LaunchSessionMutationCli),
    Unarchive(LaunchSessionMutationCli),
}

#[derive(Debug, Args)]
pub(super) struct LaunchSessionsCli {
    #[arg(long = "agent", short = 'a')]
    agent_ids: Vec<String>,
    #[arg(value_name = "AGENT")]
    positional_agent_ids: Vec<String>,
    #[arg(long = "workspace", aliases = ["workspace-path", "cwd"])]
    workspace_paths: Vec<String>,
    #[arg(
        long = "archived",
        alias = "include-archived",
        default_value_t = false,
        action = clap::ArgAction::Set,
        num_args = 0..=1,
        require_equals = true,
        default_missing_value = "true",
        value_parser = clap::builder::BoolishValueParser::new()
    )]
    include_archived: bool,
    #[arg(long, value_parser = parse_positive_usize)]
    limit: Option<usize>,
}

#[derive(Debug, Args)]
pub(super) struct LaunchSessionMutationCli {
    #[arg(long = "agent", short = 'a')]
    agent_id: Option<String>,
    #[arg(long = "workspace", aliases = ["workspace-path", "cwd"])]
    workspace_path: Option<String>,
    #[arg(value_name = "ARG")]
    positionals: Vec<String>,
}

pub(super) fn command_into_command(command: LaunchCommand) -> Result<Command, CliError> {
    Ok(match command {
        LaunchCommand::Sessions(args) => Command::LaunchSessions(args.into_args()),
        LaunchCommand::Archive(args) => Command::LaunchSessionArchive(args.into_args("archive")?),
        LaunchCommand::Unarchive(args) => {
            Command::LaunchSessionUnarchive(args.into_args("unarchive")?)
        }
    })
}

impl LaunchSessionsCli {
    fn into_args(mut self) -> LaunchSessionsArgs {
        self.agent_ids.append(&mut self.positional_agent_ids);
        LaunchSessionsArgs {
            agent_ids: self.agent_ids,
            workspace_paths: self.workspace_paths,
            include_archived: self.include_archived,
            limit: self.limit,
        }
    }
}

impl LaunchSessionMutationCli {
    fn into_args(mut self, action: &str) -> Result<LaunchSessionMutationArgs, CliError> {
        let agent_id = match self.agent_id.take() {
            Some(agent_id) => agent_id,
            None => {
                if self.positionals.is_empty() {
                    return Err(CliError::Usage(format!(
                        "usage: va launch {action} --agent AGENT SESSION_ID"
                    )));
                }
                self.positionals.remove(0)
            }
        };
        if self.positionals.len() != 1 {
            return Err(CliError::Usage(format!(
                "usage: va launch {action} --agent AGENT SESSION_ID"
            )));
        }
        Ok(LaunchSessionMutationArgs {
            agent_id,
            session_id: self.positionals.remove(0),
            workspace_path: self.workspace_path,
        })
    }
}

fn parse_positive_usize(value: &str) -> Result<usize, String> {
    let value = value
        .parse::<usize>()
        .map_err(|_| "must be a positive integer".to_string())?;
    if value == 0 {
        return Err("must be a positive integer".into());
    }
    Ok(value)
}
