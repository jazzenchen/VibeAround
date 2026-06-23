use std::path::PathBuf;

use crate::error::CliError;

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
            value if value.starts_with('-') => {
                return Err(CliError::Usage(format!("unknown option: {value}")));
            }
            value => positionals.push(value.to_string()),
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
        "session" => match rest {
            [action, session_id] if action == "kill" => Ok(Command::SessionKill {
                session_id: session_id.to_string(),
            }),
            _ => Err(CliError::Usage("usage: va session kill SESSION_ID".into())),
        },
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
    "Usage: va [--auth-file PATH] [--base-url URL] [--token TOKEN] [--json] <command>\n\nCommands:\n  help                         Show this help\n  health                       Check public server liveness\n  info                         Show server metadata\n  status                       Show a compact runtime summary\n  doctor                       Diagnose endpoint, auth, and server health\n  pair start                   Start browser/IM pairing\n  pair status SID              Poll a pairing session\n  channels                     List channel plugin runtimes\n  channel sync                 Reconcile channel plugins with settings\n  channel start KIND           Start a stopped channel plugin\n  channel stop KIND            Stop a channel plugin\n  channel restart KIND         Restart a channel plugin\n  tunnels                      List tunnel runtimes\n  tunnel kill PROVIDER         Stop a tunnel runtime\n  agents                       List enabled agents\n  agent kill ROUTE_KEY         Kill an attached agent runtime\n  sessions                     List PTY sessions\n  session kill SESSION_ID      Kill and remove a PTY session\n  pty kill SESSION_ID          Kill a PTY process by session id\n  workspaces                   List registered workspaces\n  workspace add PATH           Register a workspace path\n  workspace remove PATH        Remove a workspace path\n  workspace default PATH       Set the default workspace\n  workspace create NAME        Create a workspace under the default root\n  previews                     List live previews\n  preview delete SLUG          Close a live preview\n  profiles                     List model profiles\n  settings reload              Reload server settings"
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
    fn parses_json_flag() {
        let options = parse_args(["--json".to_string(), "channels".to_string()]).expect("options");
        assert!(options.json);
        assert_eq!(options.command, Some(Command::Channels));
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
