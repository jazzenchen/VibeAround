use std::path::PathBuf;

use clap::{Args, Parser, Subcommand};
use va_client::sessions::PtyTool;

use crate::error::CliError;

mod pair;

pub(crate) use pair::{PairStartArgs, PairWaitArgs};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Command {
    Help,
    Health,
    Info,
    Status,
    Doctor,
    Channels,
    Tunnels,
    Agents,
    Sessions,
    Workspaces,
    Previews,
    Profiles,
    PairStart(PairStartArgs),
    PairStatus { sid: String, save: bool },
    PairWait(PairWaitArgs),
    AuthStatus,
    AuthClear,
    SettingsReload,
    TmuxSessions,
    LaunchSessions(LaunchSessionsArgs),
    LaunchSessionArchive(LaunchSessionMutationArgs),
    LaunchSessionUnarchive(LaunchSessionMutationArgs),
    SessionCreate(SessionCreateArgs),
    SessionAttach { session_id: String },
    ChannelSync,
    ChannelStart { kind: String },
    ChannelStop { kind: String },
    ChannelRestart { kind: String },
    TunnelKill { provider: String },
    AgentKill { route_key: String },
    SessionKill { session_id: String },
    PtyKill { session_id: String },
    PreviewDelete { slug: String },
    WorkspaceAdd { path: String },
    WorkspaceRemove { path: String },
    WorkspaceDefault { path: String },
    WorkspaceCreate { name: String },
}

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

#[derive(Debug, Default)]
pub(crate) struct Options {
    pub(crate) command: Option<Command>,
    pub(crate) auth_file: Option<PathBuf>,
    pub(crate) base_url: Option<String>,
    pub(crate) token: Option<String>,
    pub(crate) json: bool,
}

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
    Channels,
    Tunnels,
    Agents,
    Sessions,
    Workspaces,
    Previews,
    Profiles,
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
        command: LaunchCommand,
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
        command: SessionCommand,
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
enum LaunchCommand {
    Sessions(LaunchSessionsCli),
    Archive(LaunchSessionMutationCli),
    Unarchive(LaunchSessionMutationCli),
}

#[derive(Debug, Args)]
struct LaunchSessionsCli {
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
struct LaunchSessionMutationCli {
    #[arg(long = "agent", short = 'a')]
    agent_id: Option<String>,
    #[arg(long = "workspace", aliases = ["workspace-path", "cwd"])]
    workspace_path: Option<String>,
    #[arg(value_name = "ARG")]
    positionals: Vec<String>,
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
enum SessionCommand {
    Create(SessionCreateCli),
    Attach { session_id: String },
    Kill { session_id: String },
}

#[derive(Debug, Args)]
struct SessionCreateCli {
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
        TopCommand::Channels => Command::Channels,
        TopCommand::Tunnels => Command::Tunnels,
        TopCommand::Agents => Command::Agents,
        TopCommand::Sessions => Command::Sessions,
        TopCommand::Workspaces => Command::Workspaces,
        TopCommand::Previews => Command::Previews,
        TopCommand::Profiles => Command::Profiles,
        TopCommand::Pair { command } => command.into_command(),
        TopCommand::Auth { command } => match command {
            AuthCommand::Status => Command::AuthStatus,
            AuthCommand::Clear => Command::AuthClear,
        },
        TopCommand::Launch { command } => launch_command_into_command(command)?,
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
        TopCommand::Session { command } => session_command_into_command(command)?,
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

fn launch_command_into_command(command: LaunchCommand) -> Result<Command, CliError> {
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

fn session_command_into_command(command: SessionCommand) -> Result<Command, CliError> {
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

fn parse_positive_usize(value: &str) -> Result<usize, String> {
    let value = value
        .parse::<usize>()
        .map_err(|_| "must be a positive integer".to_string())?;
    if value == 0 {
        return Err("must be a positive integer".into());
    }
    Ok(value)
}

pub(super) fn parse_positive_u64(value: &str) -> Result<u64, String> {
    let value = value
        .parse::<u64>()
        .map_err(|_| "must be a positive integer".to_string())?;
    if value == 0 {
        return Err("must be a positive integer".into());
    }
    Ok(value)
}

pub(crate) fn usage() -> &'static str {
    "Usage: va [--auth-file PATH] [--base-url URL] [--token TOKEN] [--json] <command>\n\nCommands:\n  help                         Show this help\n  health                       Check public server liveness\n  info                         Show server metadata\n  status                       Show a compact runtime summary\n  doctor                       Diagnose endpoint, auth, and server health\n  auth status                  Show resolved auth configuration\n  auth clear                   Remove the saved auth file\n  pair start                   Start browser/IM pairing\n  pair start --wait --save     Start pairing, wait for verification, then save auth\n  pair status SID [--save]     Poll pairing; save verified local auth with --save\n  pair wait SID [--save]       Wait for pairing verification\n  channels                     List channel plugin runtimes\n  channel sync                 Reconcile channel plugins with settings\n  channel start KIND           Start a stopped channel plugin\n  channel stop KIND            Stop a channel plugin\n  channel restart KIND         Restart a channel plugin\n  tunnels                      List tunnel runtimes\n  tunnel kill PROVIDER         Stop a tunnel runtime\n  agents                       List enabled agents\n  agent kill ROUTE_KEY         Kill an attached agent runtime\n  launch sessions              List resumable agent launch sessions\n  launch archive --agent A ID  Archive a launch session\n  launch unarchive --agent A ID Unarchive a launch session\n  sessions                     List PTY sessions\n  session create --tool TOOL   Create/resume a PTY session; add --attach to enter it\n  session attach SESSION_ID    Attach to a PTY session\n  session kill SESSION_ID      Kill and remove a PTY session\n  pty kill SESSION_ID          Kill a PTY process by session id\n  tmux sessions                List attachable tmux sessions\n  workspaces                   List registered workspaces\n  workspace add PATH           Register a workspace path\n  workspace remove PATH        Remove a workspace path\n  workspace default PATH       Set the default workspace\n  workspace create NAME        Create a workspace under the default root\n  previews                     List live previews\n  preview delete SLUG          Close a live preview\n  profiles                     List model profiles\n  settings reload              Reload server settings"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_command_and_global_options() {
        let options = parse_args([
            "--base-url".to_string(),
            "http://localhost:12358/va".to_string(),
            "--token=abc".to_string(),
            "status".to_string(),
        ])
        .expect("options");

        assert_eq!(options.command, Some(Command::Status));
        assert_eq!(
            options.base_url.as_deref(),
            Some("http://localhost:12358/va")
        );
        assert_eq!(options.token.as_deref(), Some("abc"));
    }

    #[test]
    fn rejects_unknown_command() {
        let error = parse_args(["bogus".to_string()]).expect_err("error");
        assert!(matches!(error, CliError::Usage(_)));
    }

    #[test]
    fn parses_help_as_command() {
        let options = parse_args(["--help".to_string()]).expect("options");
        assert_eq!(options.command, Some(Command::Help));

        let help = parse_args(["help".to_string()]).expect("help");
        assert_eq!(help.command, Some(Command::Help));
    }

    #[test]
    fn parses_channel_action_command() {
        let options = parse_args([
            "channel".to_string(),
            "restart".to_string(),
            "feishu".to_string(),
        ])
        .expect("options");

        assert_eq!(
            options.command,
            Some(Command::ChannelRestart {
                kind: "feishu".into()
            })
        );
    }

    #[test]
    fn parses_workspace_action_command() {
        let options = parse_args([
            "workspace".to_string(),
            "add".to_string(),
            "/tmp/project".to_string(),
        ])
        .expect("options");

        assert_eq!(
            options.command,
            Some(Command::WorkspaceAdd {
                path: "/tmp/project".into()
            })
        );
    }

    #[test]
    fn parses_pair_commands() {
        let start = parse_args(["pair".to_string(), "start".to_string()]).expect("start");
        assert_eq!(
            start.command,
            Some(Command::PairStart(PairStartArgs::default()))
        );

        let status = parse_args([
            "pair".to_string(),
            "status".to_string(),
            "sid-1".to_string(),
        ])
        .expect("status");
        assert_eq!(
            status.command,
            Some(Command::PairStatus {
                sid: "sid-1".into(),
                save: false
            })
        );
    }

    #[test]
    fn parses_pair_start_wait_save() {
        let options = parse_args([
            "pair".to_string(),
            "start".to_string(),
            "--save".to_string(),
            "--timeout".to_string(),
            "45".to_string(),
            "--interval-ms=500".to_string(),
        ])
        .expect("options");

        assert_eq!(
            options.command,
            Some(Command::PairStart(PairStartArgs {
                wait: true,
                save: true,
                timeout_secs: 45,
                interval_ms: 500,
            }))
        );
    }

    #[test]
    fn parses_pair_start_explicit_false_flags() {
        let options = parse_args([
            "pair".to_string(),
            "start".to_string(),
            "--wait=false".to_string(),
            "--save=false".to_string(),
        ])
        .expect("options");

        assert_eq!(
            options.command,
            Some(Command::PairStart(PairStartArgs {
                wait: false,
                save: false,
                ..Default::default()
            }))
        );
    }

    #[test]
    fn parses_pair_status_save() {
        let options = parse_args([
            "pair".to_string(),
            "status".to_string(),
            "--save".to_string(),
            "sid-1".to_string(),
        ])
        .expect("options");

        assert_eq!(
            options.command,
            Some(Command::PairStatus {
                sid: "sid-1".into(),
                save: true
            })
        );
    }

    #[test]
    fn parses_pair_wait_save() {
        let options = parse_args([
            "pair".to_string(),
            "wait".to_string(),
            "sid-1".to_string(),
            "--save".to_string(),
            "--timeout-secs=30".to_string(),
        ])
        .expect("options");

        assert_eq!(
            options.command,
            Some(Command::PairWait(PairWaitArgs {
                sid: "sid-1".into(),
                save: true,
                timeout_secs: 30,
                interval_ms: 2_000,
            }))
        );
    }

    #[test]
    fn parses_auth_commands() {
        let status = parse_args(["auth".to_string(), "status".to_string()]).expect("status");
        assert_eq!(status.command, Some(Command::AuthStatus));

        let clear = parse_args(["auth".to_string(), "clear".to_string()]).expect("clear");
        assert_eq!(clear.command, Some(Command::AuthClear));
    }

    #[test]
    fn parses_launch_sessions_command() {
        let options = parse_args([
            "launch".to_string(),
            "sessions".to_string(),
            "--agent".to_string(),
            "codex".to_string(),
            "--workspace=/tmp/project".to_string(),
            "--archived".to_string(),
            "--limit".to_string(),
            "10".to_string(),
        ])
        .expect("options");

        assert_eq!(
            options.command,
            Some(Command::LaunchSessions(LaunchSessionsArgs {
                agent_ids: vec!["codex".into()],
                workspace_paths: vec!["/tmp/project".into()],
                include_archived: true,
                limit: Some(10),
            }))
        );
    }

    #[test]
    fn parses_launch_sessions_explicit_archived_false() {
        let options = parse_args([
            "launch".to_string(),
            "sessions".to_string(),
            "--archived=false".to_string(),
        ])
        .expect("options");

        assert_eq!(
            options.command,
            Some(Command::LaunchSessions(LaunchSessionsArgs {
                include_archived: false,
                ..Default::default()
            }))
        );
    }

    #[test]
    fn parses_launch_archive_command_with_agent_flag() {
        let options = parse_args([
            "launch".to_string(),
            "archive".to_string(),
            "--agent=codex".to_string(),
            "session-1".to_string(),
            "--workspace".to_string(),
            "/tmp/project".to_string(),
        ])
        .expect("options");

        assert_eq!(
            options.command,
            Some(Command::LaunchSessionArchive(LaunchSessionMutationArgs {
                agent_id: "codex".into(),
                session_id: "session-1".into(),
                workspace_path: Some("/tmp/project".into()),
            }))
        );
    }

    #[test]
    fn parses_launch_unarchive_command_with_positional_agent() {
        let options = parse_args([
            "launch".to_string(),
            "unarchive".to_string(),
            "codex".to_string(),
            "session-1".to_string(),
        ])
        .expect("options");

        assert_eq!(
            options.command,
            Some(Command::LaunchSessionUnarchive(LaunchSessionMutationArgs {
                agent_id: "codex".into(),
                session_id: "session-1".into(),
                workspace_path: None,
            }))
        );
    }

    #[test]
    fn parses_session_create_with_tool_and_project() {
        let options = parse_args([
            "session".to_string(),
            "create".to_string(),
            "--tool".to_string(),
            "codex".to_string(),
            "--project=/tmp/project".to_string(),
            "--cols".to_string(),
            "120".to_string(),
            "--rows=40".to_string(),
        ])
        .expect("options");

        assert_eq!(
            options.command,
            Some(Command::SessionCreate(SessionCreateArgs {
                tool: Some(PtyTool::Codex),
                project_path: Some("/tmp/project".into()),
                cols: Some(120),
                rows: Some(40),
                ..Default::default()
            }))
        );
    }

    #[test]
    fn parses_session_create_with_attach() {
        let options = parse_args([
            "session".to_string(),
            "create".to_string(),
            "--tool".to_string(),
            "codex".to_string(),
            "--attach".to_string(),
        ])
        .expect("options");

        assert_eq!(
            options.command,
            Some(Command::SessionCreate(SessionCreateArgs {
                tool: Some(PtyTool::Codex),
                attach: true,
                ..Default::default()
            }))
        );
    }

    #[test]
    fn parses_session_create_with_explicit_attach_false() {
        let options = parse_args([
            "session".to_string(),
            "create".to_string(),
            "--tool".to_string(),
            "codex".to_string(),
            "--attach=false".to_string(),
        ])
        .expect("options");

        assert_eq!(
            options.command,
            Some(Command::SessionCreate(SessionCreateArgs {
                tool: Some(PtyTool::Codex),
                attach: false,
                ..Default::default()
            }))
        );
    }

    #[test]
    fn parses_session_create_with_resume() {
        let options = parse_args([
            "session".to_string(),
            "create".to_string(),
            "--tool=codex".to_string(),
            "--resume".to_string(),
            "resume-1".to_string(),
            "--attach".to_string(),
        ])
        .expect("options");

        assert_eq!(
            options.command,
            Some(Command::SessionCreate(SessionCreateArgs {
                tool: Some(PtyTool::Codex),
                resume_session_id: Some("resume-1".into()),
                attach: true,
                ..Default::default()
            }))
        );
    }

    #[test]
    fn rejects_session_create_resume_with_tmux() {
        let error = parse_args([
            "session".to_string(),
            "create".to_string(),
            "--resume=resume-1".to_string(),
            "--tmux=work".to_string(),
        ])
        .expect_err("error");
        assert!(matches!(error, CliError::Usage(_)));
    }

    #[test]
    fn parses_session_attach_command() {
        let options = parse_args([
            "session".to_string(),
            "attach".to_string(),
            "sid-1".to_string(),
        ])
        .expect("options");

        assert_eq!(
            options.command,
            Some(Command::SessionAttach {
                session_id: "sid-1".into()
            })
        );
    }

    #[test]
    fn parses_session_create_with_profile_target() {
        let options = parse_args([
            "session".to_string(),
            "create".to_string(),
            "--profile=p1".to_string(),
            "--target".to_string(),
            "claude".to_string(),
        ])
        .expect("options");

        assert_eq!(
            options.command,
            Some(Command::SessionCreate(SessionCreateArgs {
                profile_id: Some("p1".into()),
                launch_target: Some("claude".into()),
                ..Default::default()
            }))
        );
    }

    #[test]
    fn parses_session_create_with_tmux_defaulting_generic_tool() {
        let options = parse_args([
            "session".to_string(),
            "create".to_string(),
            "--tmux".to_string(),
            "server".to_string(),
        ])
        .expect("options");

        assert_eq!(
            options.command,
            Some(Command::SessionCreate(SessionCreateArgs {
                tool: Some(PtyTool::Generic),
                tmux_session: Some("server".into()),
                ..Default::default()
            }))
        );
    }

    #[test]
    fn rejects_incomplete_profile_session_create() {
        let error = parse_args([
            "session".to_string(),
            "create".to_string(),
            "--profile=p1".to_string(),
        ])
        .expect_err("error");
        assert!(matches!(error, CliError::Usage(_)));
    }

    #[test]
    fn parses_tmux_sessions_command() {
        let options = parse_args(["tmux".to_string(), "sessions".to_string()]).expect("options");
        assert_eq!(options.command, Some(Command::TmuxSessions));
    }

    #[test]
    fn parses_json_flag() {
        let options = parse_args(["--json".to_string(), "channels".to_string()]).expect("options");
        assert!(options.json);
        assert_eq!(options.command, Some(Command::Channels));
    }

    #[test]
    fn parses_global_json_after_simple_command() {
        let options = parse_args(["status".to_string(), "--json".to_string()]).expect("options");
        assert!(options.json);
        assert_eq!(options.command, Some(Command::Status));
    }

    #[test]
    fn parses_global_json_after_command_with_local_options() {
        let options = parse_args([
            "session".to_string(),
            "create".to_string(),
            "--tool".to_string(),
            "codex".to_string(),
            "--json".to_string(),
        ])
        .expect("options");
        assert!(options.json);
        assert_eq!(
            options.command,
            Some(Command::SessionCreate(SessionCreateArgs {
                tool: Some(PtyTool::Codex),
                ..Default::default()
            }))
        );
    }

    #[test]
    fn parses_doctor_command() {
        let options = parse_args(["doctor".to_string()]).expect("options");
        assert_eq!(options.command, Some(Command::Doctor));
    }

    #[test]
    fn rejects_unexpected_subcommand_args() {
        let error = parse_args(["status".to_string(), "extra".to_string()]).expect_err("error");
        assert!(matches!(error, CliError::Usage(_)));
    }
}
