use std::path::PathBuf;

use clap::{Parser, Subcommand};

use super::{chat, launch, pair, serve, session, Command, Options};
use crate::error::CliError;

#[derive(Debug, Parser)]
#[command(
    name = "va",
    disable_help_flag = true,
    disable_help_subcommand = true,
    disable_version_flag = true
)]
struct CliArgs {
    #[arg(long = "help", short = 'h', global = true)]
    help: bool,
    #[arg(long, global = true)]
    auth_file: Option<PathBuf>,
    #[arg(long, global = true)]
    base_url: Option<String>,
    #[arg(long, global = true)]
    token: Option<String>,
    #[arg(long, global = true)]
    json: bool,
    #[command(subcommand)]
    command: Option<TopCommand>,
}

#[derive(Debug, Subcommand)]
#[command(rename_all = "kebab-case")]
enum TopCommand {
    Help,
    Health,
    Info,
    Status,
    Doctor,
    Serve(serve::ServeArgs),
    Channels,
    Tunnels,
    Agents,
    Sessions,
    Workspaces,
    Previews,
    Profiles,
    Chat {
        #[command(subcommand)]
        command: chat::ChatCommand,
    },
    Pair {
        #[command(subcommand)]
        command: pair::PairCommand,
    },
    Auth {
        #[command(subcommand)]
        command: AuthCommand,
    },
    Launch {
        #[command(subcommand)]
        command: launch::LaunchCommand,
    },
    Tmux {
        #[command(subcommand)]
        command: TmuxCommand,
    },
    Settings {
        #[command(subcommand)]
        command: SettingsCommand,
    },
    Channel {
        #[command(subcommand)]
        command: ChannelCommand,
    },
    Tunnel {
        #[command(subcommand)]
        command: TunnelCommand,
    },
    Agent {
        #[command(subcommand)]
        command: AgentCommand,
    },
    Session {
        #[command(subcommand)]
        command: session::SessionCommand,
    },
    Pty {
        #[command(subcommand)]
        command: PtyCommand,
    },
    Preview {
        #[command(subcommand)]
        command: PreviewCommand,
    },
    Workspace {
        #[command(subcommand)]
        command: WorkspaceCommand,
    },
}

#[derive(Debug, Subcommand)]
#[command(rename_all = "kebab-case")]
enum AuthCommand {
    Status,
    Clear,
}

#[derive(Debug, Subcommand)]
#[command(rename_all = "kebab-case")]
enum TmuxCommand {
    Sessions,
}

#[derive(Debug, Subcommand)]
#[command(rename_all = "kebab-case")]
enum SettingsCommand {
    Reload,
}

#[derive(Debug, Subcommand)]
#[command(rename_all = "kebab-case")]
enum ChannelCommand {
    Sync,
    Start { kind: String },
    Stop { kind: String },
    Restart { kind: String },
}

#[derive(Debug, Subcommand)]
#[command(rename_all = "kebab-case")]
enum TunnelCommand {
    Kill { provider: String },
}

#[derive(Debug, Subcommand)]
#[command(rename_all = "kebab-case")]
enum AgentCommand {
    Kill { route_key: String },
}

#[derive(Debug, Subcommand)]
#[command(rename_all = "kebab-case")]
enum PtyCommand {
    Kill { session_id: String },
}

#[derive(Debug, Subcommand)]
#[command(rename_all = "kebab-case")]
enum PreviewCommand {
    Delete { slug: String },
}

#[derive(Debug, Subcommand)]
#[command(rename_all = "kebab-case")]
enum WorkspaceCommand {
    Add { path: String },
    Remove { path: String },
    Default { path: String },
    Create { name: String },
}

pub(crate) fn parse_args<I>(args: I) -> Result<Options, CliError>
where
    I: IntoIterator<Item = String>,
{
    let cli = CliArgs::try_parse_from(std::iter::once("va".to_string()).chain(args))
        .map_err(|error| CliError::Usage(error.to_string()))?;
    let command = if cli.help {
        Some(Command::Help)
    } else {
        cli.command.map(top_command_into_command).transpose()?
    };
    Ok(Options {
        command,
        auth_file: cli.auth_file,
        base_url: cli.base_url,
        token: cli.token,
        json: cli.json,
    })
}

fn top_command_into_command(command: TopCommand) -> Result<Command, CliError> {
    Ok(match command {
        TopCommand::Help => Command::Help,
        TopCommand::Health => Command::Health,
        TopCommand::Info => Command::Info,
        TopCommand::Status => Command::Status,
        TopCommand::Doctor => Command::Doctor,
        TopCommand::Serve(args) => Command::Serve(args),
        TopCommand::Channels => Command::Channels,
        TopCommand::Tunnels => Command::Tunnels,
        TopCommand::Agents => Command::Agents,
        TopCommand::Sessions => Command::Sessions,
        TopCommand::Workspaces => Command::Workspaces,
        TopCommand::Previews => Command::Previews,
        TopCommand::Profiles => Command::Profiles,
        TopCommand::Chat { command } => chat::command_into_command(command)?,
        TopCommand::Pair { command } => command.into_command(),
        TopCommand::Auth { command } => match command {
            AuthCommand::Status => Command::AuthStatus,
            AuthCommand::Clear => Command::AuthClear,
        },
        TopCommand::Launch { command } => launch::command_into_command(command)?,
        TopCommand::Tmux { command } => match command {
            TmuxCommand::Sessions => Command::TmuxSessions,
        },
        TopCommand::Settings { command } => match command {
            SettingsCommand::Reload => Command::SettingsReload,
        },
        TopCommand::Channel { command } => match command {
            ChannelCommand::Sync => Command::ChannelSync,
            ChannelCommand::Start { kind } => Command::ChannelStart { kind },
            ChannelCommand::Stop { kind } => Command::ChannelStop { kind },
            ChannelCommand::Restart { kind } => Command::ChannelRestart { kind },
        },
        TopCommand::Tunnel { command } => match command {
            TunnelCommand::Kill { provider } => Command::TunnelKill { provider },
        },
        TopCommand::Agent { command } => match command {
            AgentCommand::Kill { route_key } => Command::AgentKill { route_key },
        },
        TopCommand::Session { command } => session::command_into_command(command)?,
        TopCommand::Pty { command } => match command {
            PtyCommand::Kill { session_id } => Command::PtyKill { session_id },
        },
        TopCommand::Preview { command } => match command {
            PreviewCommand::Delete { slug } => Command::PreviewDelete { slug },
        },
        TopCommand::Workspace { command } => match command {
            WorkspaceCommand::Add { path } => Command::WorkspaceAdd { path },
            WorkspaceCommand::Remove { path } => Command::WorkspaceRemove { path },
            WorkspaceCommand::Default { path } => Command::WorkspaceDefault { path },
            WorkspaceCommand::Create { name } => Command::WorkspaceCreate { name },
        },
    })
}
