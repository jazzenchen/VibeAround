use std::path::PathBuf;

use crate::error::CliError;
use va_client::sessions::PtyTool;

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
    PairStart,
    PairStatus { sid: String },
    SettingsReload,
    TmuxSessions,
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
pub(crate) struct SessionCreateArgs {
    pub(crate) tool: Option<PtyTool>,
    pub(crate) profile_id: Option<String>,
    pub(crate) launch_target: Option<String>,
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

pub(crate) fn parse_args<I>(args: I) -> Result<Options, CliError>
where
    I: IntoIterator<Item = String>,
{
    let mut options = Options::default();
    let mut args = args.into_iter().peekable();
    let mut positionals = Vec::new();
    let mut command_started = false;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--help" | "-h" => options.command = Some(Command::Help),
            "--auth-file" => {
                options.auth_file = Some(PathBuf::from(next_value(&mut args, "--auth-file")?));
            }
            "--base-url" => {
                options.base_url = Some(next_value(&mut args, "--base-url")?);
            }
            "--token" => {
                options.token = Some(next_value(&mut args, "--token")?);
            }
            "--json" => {
                options.json = true;
            }
            value if value.starts_with("--auth-file=") => {
                options.auth_file = Some(PathBuf::from(value.trim_start_matches("--auth-file=")));
            }
            value if value.starts_with("--base-url=") => {
                options.base_url = Some(value.trim_start_matches("--base-url=").to_string());
            }
            value if value.starts_with("--token=") => {
                options.token = Some(value.trim_start_matches("--token=").to_string());
            }
            value if value.starts_with('-') && !command_started => {
                return Err(CliError::Usage(format!("unknown option: {value}")));
            }
            value => {
                command_started = true;
                positionals.push(value.to_string());
            }
        }
    }
    if !positionals.is_empty() {
        if options.command.is_some() {
            return Err(CliError::Usage(format!(
                "unexpected argument: {}",
                positionals[0]
            )));
        }
        options.command = Some(parse_command(&positionals)?);
    }
    Ok(options)
}

fn next_value<I>(args: &mut std::iter::Peekable<I>, flag: &str) -> Result<String, CliError>
where
    I: Iterator<Item = String>,
{
    args.next()
        .filter(|value| !value.is_empty())
        .ok_or_else(|| CliError::Usage(format!("missing value for {flag}")))
}

fn parse_command(args: &[String]) -> Result<Command, CliError> {
    let Some(command) = args.first().map(String::as_str) else {
        return Err(CliError::Usage("missing command".into()));
    };
    let rest = &args[1..];
    match command {
        "help" => no_args(rest, "help").map(|()| Command::Help),
        "health" => no_args(rest, "health").map(|()| Command::Health),
        "info" => no_args(rest, "info").map(|()| Command::Info),
        "status" => no_args(rest, "status").map(|()| Command::Status),
        "doctor" => no_args(rest, "doctor").map(|()| Command::Doctor),
        "channels" => no_args(rest, "channels").map(|()| Command::Channels),
        "tunnels" => no_args(rest, "tunnels").map(|()| Command::Tunnels),
        "agents" => no_args(rest, "agents").map(|()| Command::Agents),
        "sessions" => no_args(rest, "sessions").map(|()| Command::Sessions),
        "workspaces" => no_args(rest, "workspaces").map(|()| Command::Workspaces),
        "previews" => no_args(rest, "previews").map(|()| Command::Previews),
        "profiles" => no_args(rest, "profiles").map(|()| Command::Profiles),
        "pair" => parse_pair_command(rest),
        "tmux" => parse_tmux_command(rest),
        "settings" => match rest {
            [action] if action == "reload" => Ok(Command::SettingsReload),
            _ => Err(CliError::Usage("usage: va settings reload".to_string())),
        },
        "channel" => parse_channel_command(rest),
        "tunnel" => match rest {
            [action, provider] if action == "kill" => Ok(Command::TunnelKill {
                provider: provider.to_string(),
            }),
            _ => Err(CliError::Usage("usage: va tunnel kill PROVIDER".into())),
        },
        "agent" => match rest {
            [action, route_key] if action == "kill" => Ok(Command::AgentKill {
                route_key: route_key.to_string(),
            }),
            _ => Err(CliError::Usage("usage: va agent kill ROUTE_KEY".into())),
        },
        "session" => parse_session_command(rest),
        "pty" => match rest {
            [action, session_id] if action == "kill" => Ok(Command::PtyKill {
                session_id: session_id.to_string(),
            }),
            _ => Err(CliError::Usage("usage: va pty kill SESSION_ID".into())),
        },
        "preview" => match rest {
            [action, slug] if action == "delete" => Ok(Command::PreviewDelete {
                slug: slug.to_string(),
            }),
            _ => Err(CliError::Usage("usage: va preview delete SLUG".into())),
        },
        "workspace" => parse_workspace_command(rest),
        other => Err(CliError::Usage(format!("unknown command: {other}"))),
    }
}

fn parse_session_command(args: &[String]) -> Result<Command, CliError> {
    match args {
        [action, session_id] if action == "kill" => Ok(Command::SessionKill {
            session_id: session_id.to_string(),
        }),
        [action, session_id] if action == "attach" => Ok(Command::SessionAttach {
            session_id: session_id.to_string(),
        }),
        [action, rest @ ..] if action == "create" => {
            parse_session_create_args(rest).map(Command::SessionCreate)
        }
        _ => Err(CliError::Usage(
            "usage: va session create --tool TOOL [--project PATH]; va session attach SESSION_ID; va session kill SESSION_ID".into(),
        )),
    }
}

fn parse_tmux_command(args: &[String]) -> Result<Command, CliError> {
    match args {
        [action] if action == "sessions" => Ok(Command::TmuxSessions),
        _ => Err(CliError::Usage("usage: va tmux sessions".into())),
    }
}

fn parse_session_create_args(args: &[String]) -> Result<SessionCreateArgs, CliError> {
    let mut create = SessionCreateArgs::default();
    let mut args = args.iter().peekable();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--tool" => create.tool = Some(parse_tool(&next_ref(&mut args, "--tool")?)?),
            "--profile" | "--profile-id" => {
                create.profile_id = Some(next_ref(&mut args, arg)?.to_string());
            }
            "--target" | "--launch-target" => {
                create.launch_target = Some(next_ref(&mut args, arg)?.to_string());
            }
            "--project" | "--project-path" | "--cwd" => {
                create.project_path = Some(next_ref(&mut args, arg)?.to_string());
            }
            "--tmux" | "--tmux-session" => {
                create.tmux_session = Some(next_ref(&mut args, arg)?.to_string());
            }
            "--attach" => create.attach = true,
            "--theme" => create.theme = Some(next_ref(&mut args, "--theme")?.to_string()),
            "--cols" => create.cols = Some(parse_u16(&next_ref(&mut args, "--cols")?, "--cols")?),
            "--rows" => create.rows = Some(parse_u16(&next_ref(&mut args, "--rows")?, "--rows")?),
            value if value.starts_with("--tool=") => {
                create.tool = Some(parse_tool(value.trim_start_matches("--tool="))?);
            }
            value if value.starts_with("--profile=") => {
                create.profile_id = Some(value.trim_start_matches("--profile=").to_string());
            }
            value if value.starts_with("--profile-id=") => {
                create.profile_id = Some(value.trim_start_matches("--profile-id=").to_string());
            }
            value if value.starts_with("--target=") => {
                create.launch_target = Some(value.trim_start_matches("--target=").to_string());
            }
            value if value.starts_with("--launch-target=") => {
                create.launch_target =
                    Some(value.trim_start_matches("--launch-target=").to_string());
            }
            value if value.starts_with("--project=") => {
                create.project_path = Some(value.trim_start_matches("--project=").to_string());
            }
            value if value.starts_with("--project-path=") => {
                create.project_path = Some(value.trim_start_matches("--project-path=").to_string());
            }
            value if value.starts_with("--cwd=") => {
                create.project_path = Some(value.trim_start_matches("--cwd=").to_string());
            }
            value if value.starts_with("--tmux=") => {
                create.tmux_session = Some(value.trim_start_matches("--tmux=").to_string());
            }
            value if value.starts_with("--tmux-session=") => {
                create.tmux_session = Some(value.trim_start_matches("--tmux-session=").to_string());
            }
            value if value.starts_with("--attach=") => {
                create.attach = parse_bool(value.trim_start_matches("--attach="), "--attach")?;
            }
            value if value.starts_with("--theme=") => {
                create.theme = Some(value.trim_start_matches("--theme=").to_string());
            }
            value if value.starts_with("--cols=") => {
                create.cols = Some(parse_u16(value.trim_start_matches("--cols="), "--cols")?);
            }
            value if value.starts_with("--rows=") => {
                create.rows = Some(parse_u16(value.trim_start_matches("--rows="), "--rows")?);
            }
            value if value.starts_with('-') => {
                return Err(CliError::Usage(format!(
                    "unknown session create option: {value}"
                )));
            }
            value => {
                if create.tool.is_some() {
                    return Err(CliError::Usage(format!("unexpected argument: {value}")));
                }
                create.tool = Some(parse_tool(value)?);
            }
        }
    }

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

fn next_ref<'a, I>(args: &mut std::iter::Peekable<I>, flag: &str) -> Result<&'a str, CliError>
where
    I: Iterator<Item = &'a String>,
{
    args.next()
        .map(String::as_str)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| CliError::Usage(format!("missing value for {flag}")))
}

fn parse_u16(value: &str, flag: &str) -> Result<u16, CliError> {
    let value = value
        .parse::<u16>()
        .map_err(|_| CliError::Usage(format!("{flag} must be a positive integer")))?;
    if value == 0 {
        return Err(CliError::Usage(format!(
            "{flag} must be a positive integer"
        )));
    }
    Ok(value)
}

fn parse_bool(value: &str, flag: &str) -> Result<bool, CliError> {
    match value {
        "true" | "1" | "yes" | "on" => Ok(true),
        "false" | "0" | "no" | "off" => Ok(false),
        _ => Err(CliError::Usage(format!("{flag} must be true or false"))),
    }
}

fn parse_tool(value: &str) -> Result<PtyTool, CliError> {
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
        _ => Err(CliError::Usage(format!("unknown PTY tool: {value}"))),
    }
}

fn parse_pair_command(args: &[String]) -> Result<Command, CliError> {
    match args {
        [action] if action == "start" => Ok(Command::PairStart),
        [action, sid] if action == "status" => Ok(Command::PairStatus {
            sid: sid.to_string(),
        }),
        _ => Err(CliError::Usage(
            "usage: va pair start; va pair status SID".into(),
        )),
    }
}

fn parse_channel_command(args: &[String]) -> Result<Command, CliError> {
    match args {
        [action] if action == "sync" => Ok(Command::ChannelSync),
        [action, kind] if action == "start" => Ok(Command::ChannelStart {
            kind: kind.to_string(),
        }),
        [action, kind] if action == "stop" => Ok(Command::ChannelStop {
            kind: kind.to_string(),
        }),
        [action, kind] if action == "restart" => Ok(Command::ChannelRestart {
            kind: kind.to_string(),
        }),
        _ => Err(CliError::Usage(
            "usage: va channel sync|start|stop|restart [KIND]".into(),
        )),
    }
}

fn parse_workspace_command(args: &[String]) -> Result<Command, CliError> {
    match args {
        [action, path] if action == "add" => Ok(Command::WorkspaceAdd {
            path: path.to_string(),
        }),
        [action, path] if action == "remove" => Ok(Command::WorkspaceRemove {
            path: path.to_string(),
        }),
        [action, path] if action == "default" => Ok(Command::WorkspaceDefault {
            path: path.to_string(),
        }),
        [action, name] if action == "create" => Ok(Command::WorkspaceCreate {
            name: name.to_string(),
        }),
        _ => Err(CliError::Usage(
            "usage: va workspace add|remove|default PATH; va workspace create NAME".into(),
        )),
    }
}

fn no_args(args: &[String], command: &str) -> Result<(), CliError> {
    if args.is_empty() {
        Ok(())
    } else {
        Err(CliError::Usage(format!("usage: va {command}")))
    }
}

pub(crate) fn usage() -> &'static str {
    "Usage: va [--auth-file PATH] [--base-url URL] [--token TOKEN] [--json] <command>\n\nCommands:\n  help                         Show this help\n  health                       Check public server liveness\n  info                         Show server metadata\n  status                       Show a compact runtime summary\n  doctor                       Diagnose endpoint, auth, and server health\n  pair start                   Start browser/IM pairing\n  pair status SID              Poll a pairing session\n  channels                     List channel plugin runtimes\n  channel sync                 Reconcile channel plugins with settings\n  channel start KIND           Start a stopped channel plugin\n  channel stop KIND            Stop a channel plugin\n  channel restart KIND         Restart a channel plugin\n  tunnels                      List tunnel runtimes\n  tunnel kill PROVIDER         Stop a tunnel runtime\n  agents                       List enabled agents\n  agent kill ROUTE_KEY         Kill an attached agent runtime\n  sessions                     List PTY sessions\n  session create --tool TOOL   Create a PTY session; add --attach to enter it\n  session attach SESSION_ID    Attach to a PTY session\n  session kill SESSION_ID      Kill and remove a PTY session\n  pty kill SESSION_ID          Kill a PTY process by session id\n  tmux sessions                List attachable tmux sessions\n  workspaces                   List registered workspaces\n  workspace add PATH           Register a workspace path\n  workspace remove PATH        Remove a workspace path\n  workspace default PATH       Set the default workspace\n  workspace create NAME        Create a workspace under the default root\n  previews                     List live previews\n  preview delete SLUG          Close a live preview\n  profiles                     List model profiles\n  settings reload              Reload server settings"
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
        assert_eq!(start.command, Some(Command::PairStart));

        let status = parse_args([
            "pair".to_string(),
            "status".to_string(),
            "sid-1".to_string(),
        ])
        .expect("status");
        assert_eq!(
            status.command,
            Some(Command::PairStatus {
                sid: "sid-1".into()
            })
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
