use super::*;
use crate::error::CliError;

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
fn parses_serve_command() {
    let options = parse_args([
        "serve".to_string(),
        "--port=12358".to_string(),
        "--data-dir".to_string(),
        "/tmp/va-data".to_string(),
        "--web-dist=/tmp/web".to_string(),
        "--auth-mode".to_string(),
        "token".to_string(),
        "--server-bin".to_string(),
        "/tmp/vibearound-server".to_string(),
    ])
    .expect("options");

    assert_eq!(
        options.command,
        Some(Command::Serve(ServeArgs {
            port: Some(12358),
            data_dir: Some("/tmp/va-data".into()),
            web_dist: Some("/tmp/web".into()),
            auth_mode: Some("token".into()),
            server_bin: Some("/tmp/vibearound-server".into()),
        }))
    );
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
fn parses_chat_send_command() {
    let options = parse_args([
        "chat".to_string(),
        "send".to_string(),
        "--agent=codex".to_string(),
        "--profile".to_string(),
        "deepseek".to_string(),
        "--resume=sid-1".to_string(),
        "--workspace=/tmp/project".to_string(),
        "--permission-mode".to_string(),
        "acceptEdits".to_string(),
        "hello".to_string(),
        "there".to_string(),
    ])
    .expect("options");

    assert_eq!(
        options.command,
        Some(Command::ChatSend(ChatSendArgs {
            text: "hello there".into(),
            read_stdin: false,
            agent: Some("codex".into()),
            profile_id: Some("deepseek".into()),
            resume_session_id: Some("sid-1".into()),
            new_session: false,
            continue_session: false,
            workspace_path: Some("/tmp/project".into()),
            permission_mode: Some("acceptEdits".into()),
        }))
    );
}

#[test]
fn parses_chat_send_continue_command() {
    let options = parse_args([
        "chat".to_string(),
        "send".to_string(),
        "--continue".to_string(),
        "hello".to_string(),
    ])
    .expect("options");

    assert_eq!(
        options.command,
        Some(Command::ChatSend(ChatSendArgs {
            text: "hello".into(),
            read_stdin: false,
            agent: None,
            profile_id: None,
            resume_session_id: None,
            new_session: false,
            continue_session: true,
            workspace_path: None,
            permission_mode: None,
        }))
    );
}

#[test]
fn parses_chat_send_stdin_command() {
    let options = parse_args([
        "chat".to_string(),
        "send".to_string(),
        "--stdin".to_string(),
        "--new-session".to_string(),
    ])
    .expect("options");

    assert_eq!(
        options.command,
        Some(Command::ChatSend(ChatSendArgs {
            text: String::new(),
            read_stdin: true,
            agent: None,
            profile_id: None,
            resume_session_id: None,
            new_session: true,
            continue_session: false,
            workspace_path: None,
            permission_mode: None,
        }))
    );
}

#[test]
fn rejects_chat_send_stdin_with_text() {
    let error = parse_args([
        "chat".to_string(),
        "send".to_string(),
        "--stdin".to_string(),
        "hello".to_string(),
    ])
    .expect_err("error");
    assert!(matches!(error, CliError::Usage(_)));
}

#[test]
fn parses_chat_repl_command() {
    let options = parse_args([
        "chat".to_string(),
        "repl".to_string(),
        "--agent=codex".to_string(),
        "--profile".to_string(),
        "default".to_string(),
        "--continue".to_string(),
        "--workspace=/tmp/project".to_string(),
        "--permission-mode=acceptEdits".to_string(),
    ])
    .expect("options");

    assert_eq!(
        options.command,
        Some(Command::ChatRepl(ChatReplArgs {
            agent: Some("codex".into()),
            profile_id: Some("default".into()),
            resume_session_id: None,
            new_session: false,
            continue_session: true,
            workspace_path: Some("/tmp/project".into()),
            permission_mode: Some("acceptEdits".into()),
        }))
    );
}

#[test]
fn parses_chat_session_management_commands() {
    let sessions = parse_args(["chat".to_string(), "sessions".to_string()]).expect("sessions");
    assert_eq!(sessions.command, Some(Command::ChatSessions));

    let forget = parse_args([
        "chat".to_string(),
        "forget".to_string(),
        "--agent".to_string(),
        "codex".to_string(),
        "--profile=default".to_string(),
        "--workspace=/tmp/project".to_string(),
    ])
    .expect("forget");
    assert_eq!(
        forget.command,
        Some(Command::ChatForget(ChatForgetArgs {
            agent: Some("codex".into()),
            profile_id: Some("default".into()),
            workspace_path: Some("/tmp/project".into()),
            all: false,
        }))
    );

    let forget_all = parse_args([
        "chat".to_string(),
        "forget".to_string(),
        "--all".to_string(),
    ])
    .expect("forget all");
    assert_eq!(
        forget_all.command,
        Some(Command::ChatForget(ChatForgetArgs {
            agent: None,
            profile_id: None,
            workspace_path: None,
            all: true,
        }))
    );
}

#[test]
fn rejects_chat_forget_all_with_scope_filters() {
    let error = parse_args([
        "chat".to_string(),
        "forget".to_string(),
        "--all".to_string(),
        "--agent=codex".to_string(),
    ])
    .expect_err("error");
    assert!(matches!(error, CliError::Usage(_)));
}

#[test]
fn rejects_chat_repl_continue_conflicts() {
    let error = parse_args([
        "chat".to_string(),
        "repl".to_string(),
        "--resume=sid-1".to_string(),
        "--continue".to_string(),
    ])
    .expect_err("error");
    assert!(matches!(error, CliError::Usage(_)));
}

#[test]
fn rejects_chat_send_conflicting_session_intent() {
    let error = parse_args([
        "chat".to_string(),
        "send".to_string(),
        "--new-session".to_string(),
        "--resume=sid-1".to_string(),
        "hello".to_string(),
    ])
    .expect_err("error");
    assert!(matches!(error, CliError::Usage(_)));
}

#[test]
fn rejects_chat_send_continue_conflicts() {
    let new_error = parse_args([
        "chat".to_string(),
        "send".to_string(),
        "--new-session".to_string(),
        "--continue".to_string(),
        "hello".to_string(),
    ])
    .expect_err("error");
    assert!(matches!(new_error, CliError::Usage(_)));

    let resume_error = parse_args([
        "chat".to_string(),
        "send".to_string(),
        "--resume=sid-1".to_string(),
        "--continue".to_string(),
        "hello".to_string(),
    ])
    .expect_err("error");
    assert!(matches!(resume_error, CliError::Usage(_)));
}

#[test]
fn rejects_chat_send_workspace_without_session_intent() {
    let error = parse_args([
        "chat".to_string(),
        "send".to_string(),
        "--workspace=/tmp/project".to_string(),
        "hello".to_string(),
    ])
    .expect_err("error");
    assert!(matches!(error, CliError::Usage(_)));
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
fn parses_launch_profile_command() {
    let options = parse_args([
        "launch".to_string(),
        "--profile".to_string(),
        "codex-work".to_string(),
        "--dry-run".to_string(),
    ])
    .expect("options");

    assert_eq!(
        options.command,
        Some(Command::LaunchRun(LaunchRunArgs {
            profile: Some("codex-work".into()),
            profile_path: None,
            dry_run: true,
        }))
    );
}

#[test]
fn parses_launch_profile_path_command() {
    let options = parse_args([
        "launch".to_string(),
        "--profile-path".to_string(),
        "/tmp/launch.json".to_string(),
    ])
    .expect("options");

    assert_eq!(
        options.command,
        Some(Command::LaunchRun(LaunchRunArgs {
            profile: None,
            profile_path: Some(std::path::PathBuf::from("/tmp/launch.json")),
            dry_run: false,
        }))
    );
}

#[test]
fn rejects_launch_profile_flags_with_subcommands() {
    let error = parse_args([
        "launch".to_string(),
        "--profile".to_string(),
        "codex-work".to_string(),
        "sessions".to_string(),
    ])
    .expect_err("error");

    assert!(matches!(error, CliError::Usage(_)));
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
        "pair".to_string(),
        "status".to_string(),
        "sid-1".to_string(),
        "--save".to_string(),
        "--json".to_string(),
    ])
    .expect("options");
    assert!(options.json);
    assert_eq!(
        options.command,
        Some(Command::PairStatus {
            sid: "sid-1".into(),
            save: true
        })
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
