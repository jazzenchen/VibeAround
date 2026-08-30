use std::io::{IsTerminal, Read, Write};
use std::sync::{Mutex, OnceLock};

use futures_util::{SinkExt, StreamExt};
use serde_json::Value;
use tokio::sync::mpsc;
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::Message;
use va_client::events::{
    chat_ws, decode_chat_event, encode_chat_client_message, ChatClientMessage, ChatEvent,
    ChatSessionAction,
};
use va_client::http::AuthRequirement;
use va_client::state::ChatState;

use crate::args::{ChatForgetArgs, ChatReplArgs, ChatSendArgs, Options};
use crate::chat_store::{
    clear_sessions, forget_session_for, list_sessions, save_session_for, saved_session_for,
    scope_for_args, scope_for_forget_args, ChatSessionScope, StoredChatSession,
};
use crate::config::endpoint_for;
use crate::error::CliError;

pub(super) fn sessions(options: &Options) -> Result<(), CliError> {
    let sessions = list_sessions(options)?;
    if options.json {
        crate::print_json(serde_json::json!({
            "sessions": sessions.iter().map(session_json).collect::<Vec<_>>()
        }))?;
        return Ok(());
    }

    if sessions.is_empty() {
        println!("no saved chat sessions");
        return Ok(());
    }

    for session in sessions {
        println!(
            "{}\t{}\t{}\t{}\t{}",
            session.session_id,
            session.updated_at_ms,
            session.workspace,
            label(session.agent.as_deref()),
            label(session.profile_id.as_deref())
        );
    }
    Ok(())
}

pub(super) fn forget(options: &Options, args: &ChatForgetArgs) -> Result<(), CliError> {
    if args.all {
        let removed = clear_sessions(options)?;
        if options.json {
            crate::print_json(serde_json::json!({
                "removed": removed,
                "all": true
            }))?;
        } else {
            println!("forgot {removed} saved chat sessions");
        }
        return Ok(());
    }

    let scope = scope_for_forget_args(args)?;
    let removed = forget_session_for(options, &scope)?;
    if options.json {
        crate::print_json(serde_json::json!({
            "removed": removed,
            "scope": {
                "workspace": scope.workspace,
                "agent": scope.agent,
                "profile_id": scope.profile_id
            }
        }))?;
    } else if removed {
        println!("forgot saved chat session for {}", scope.display());
    } else {
        println!("no saved chat session for {}", scope.display());
    }
    Ok(())
}

pub(super) async fn repl(options: &Options, args: &ChatReplArgs) -> Result<(), CliError> {
    if options.json {
        return Err(CliError::Usage("chat repl does not support --json".into()));
    }

    let interactive = std::io::stdin().is_terminal();
    if interactive {
        eprintln!("va chat repl; type /exit or /quit to leave");
    }

    let mut first_turn = true;
    loop {
        if interactive {
            eprint!("va> ");
            std::io::stderr().flush().map_err(|source| CliError::Io {
                action: "flushing chat repl prompt",
                source,
            })?;
        }

        let mut input = String::new();
        let read = std::io::stdin()
            .read_line(&mut input)
            .map_err(|source| CliError::Io {
                action: "reading chat repl input",
                source,
            })?;
        if read == 0 {
            break;
        }

        let text = input.trim();
        if text.is_empty() {
            continue;
        }
        if is_repl_exit(text) {
            break;
        }

        let turn = repl_turn_args(args, text.to_string(), first_turn);
        send(options, &turn).await?;
        first_turn = false;
    }

    Ok(())
}

/// Global Ctrl+C dispatch. Installing tokio's ctrl_c listener permanently
/// replaces the default kill-on-SIGINT disposition, so one task owns the
/// signal for the whole CLI: presses during a turn are forwarded to that
/// turn's channel, presses outside one restore the conventional exit(130).
struct TurnInterrupts {
    turn_tx: Mutex<Option<mpsc::UnboundedSender<()>>>,
}

static TURN_INTERRUPTS: OnceLock<TurnInterrupts> = OnceLock::new();

fn turn_interrupts() -> &'static TurnInterrupts {
    TURN_INTERRUPTS.get_or_init(|| {
        tokio::spawn(async {
            loop {
                if tokio::signal::ctrl_c().await.is_err() {
                    return;
                }
                let forwarded = TURN_INTERRUPTS
                    .get()
                    .and_then(|state| {
                        let turn = state.turn_tx.lock().expect("interrupt registry poisoned");
                        turn.as_ref().map(|tx| tx.send(()).is_ok())
                    })
                    .unwrap_or(false);
                if !forwarded {
                    std::process::exit(130);
                }
            }
        });
        TurnInterrupts {
            turn_tx: Mutex::new(None),
        }
    })
}

/// Registers the running turn as the Ctrl+C recipient; dropping it (turn
/// over) hands the signal back to the exit(130) path.
struct TurnInterruptGuard {
    rx: mpsc::UnboundedReceiver<()>,
}

impl TurnInterruptGuard {
    fn begin() -> Self {
        let (tx, rx) = mpsc::unbounded_channel();
        *turn_interrupts()
            .turn_tx
            .lock()
            .expect("interrupt registry poisoned") = Some(tx);
        Self { rx }
    }

    async fn pressed(&mut self) {
        if self.rx.recv().await.is_none() {
            std::future::pending::<()>().await;
        }
    }
}

impl Drop for TurnInterruptGuard {
    fn drop(&mut self) {
        if let Ok(mut turn) = turn_interrupts().turn_tx.lock() {
            *turn = None;
        }
    }
}

pub(super) async fn send(options: &Options, args: &ChatSendArgs) -> Result<(), CliError> {
    let session = resolve_session(options, args)?;
    let endpoint = endpoint_for(options, AuthRequirement::BearerToken)?;
    let text = prompt_text(args)?;
    let mut args = args.clone();
    args.text = text;
    let socket = chat_ws();
    let url = endpoint.websocket_url(&socket);
    let (ws, _) = connect_async(&url)
        .await
        .map_err(|source| ws_error(&url, source))?;
    let (mut ws_tx, mut ws_rx) = ws.split();

    let body = encode_chat_client_message(&message_from_args(&args, &session))?;
    ws_tx
        .send(Message::Text(body.into()))
        .await
        .map_err(|source| ws_error(&url, source))?;

    let mut state = ChatState::new();
    let mut wrote_text_chunk = false;
    let mut interrupts = TurnInterruptGuard::begin();
    let mut cancel_requested = false;
    loop {
        let frame = tokio::select! {
            biased;
            _ = interrupts.pressed() => {
                if cancel_requested {
                    let _ = ws_tx.close().await;
                    return Err(CliError::Chat(
                        "interrupted; the turn keeps running on the daemon".into(),
                    ));
                }
                cancel_requested = true;
                finish_text_line(wrote_text_chunk)?;
                wrote_text_chunk = false;
                eprintln!("^C cancelling turn (press again to exit)");
                let body = encode_chat_client_message(&ChatClientMessage::cancel())?;
                ws_tx
                    .send(Message::Text(body.into()))
                    .await
                    .map_err(|source| ws_error(&url, source))?;
                continue;
            }
            frame = ws_rx.next() => match frame {
                Some(frame) => frame,
                None => break,
            },
        };
        let frame = frame.map_err(|source| ws_error(&url, source))?;
        match frame {
            Message::Text(text) => {
                let raw = serde_json::from_str::<Value>(&text)?;
                let event = decode_chat_event(raw.clone())?;
                if let ChatEvent::PermissionRequest {
                    request_id,
                    request,
                } = &event
                {
                    if options.json || !std::io::stdin().is_terminal() {
                        render_event(options, &event, &raw, &mut wrote_text_chunk)?;
                        state.apply_event(event);
                        let _ = ws_tx.close().await;
                        return Err(CliError::Chat(
                            "permission request received; rerun without --json from a terminal to respond interactively"
                                .into(),
                        ));
                    }
                    finish_text_line(wrote_text_chunk)?;
                    wrote_text_chunk = false;
                    let response = prompt_permission_response(request_id, request)?;
                    state.apply_event(event);
                    let body = encode_chat_client_message(&response)?;
                    ws_tx
                        .send(Message::Text(body.into()))
                        .await
                        .map_err(|source| ws_error(&url, source))?;
                    continue;
                }

                render_event(options, &event, &raw, &mut wrote_text_chunk)?;
                let terminal = terminal_event(&event);
                state.apply_event(event);
                match terminal {
                    ChatTerminalEvent::Continue => {}
                    ChatTerminalEvent::Done => {
                        finish_text_line(wrote_text_chunk)?;
                        let _ = ws_tx.close().await;
                        if let (Some(scope), Some(session_id)) =
                            (&session.store_scope, state.session_id.as_deref())
                        {
                            save_session_for(options, scope, session_id)?;
                        }
                        return Ok(());
                    }
                    ChatTerminalEvent::Error(error) => {
                        finish_text_line(wrote_text_chunk)?;
                        let _ = ws_tx.close().await;
                        return Err(CliError::Chat(error));
                    }
                }
            }
            Message::Close(_) => break,
            _ => {}
        }
    }

    finish_text_line(wrote_text_chunk)?;
    Err(CliError::Chat(
        "chat websocket closed before turn completion".into(),
    ))
}

fn session_json(session: &StoredChatSession) -> serde_json::Value {
    serde_json::json!({
        "workspace": session.workspace,
        "agent": session.agent,
        "profile_id": session.profile_id,
        "session_id": session.session_id,
        "updated_at_ms": session.updated_at_ms
    })
}

fn label(value: Option<&str>) -> &str {
    value.unwrap_or("-")
}

fn prompt_text(args: &ChatSendArgs) -> Result<String, CliError> {
    if !args.read_stdin {
        return Ok(args.text.clone());
    }

    let mut text = String::new();
    std::io::stdin()
        .read_to_string(&mut text)
        .map_err(|source| CliError::Io {
            action: "reading chat prompt from stdin",
            source,
        })?;
    if text.trim().is_empty() {
        return Err(CliError::Usage(
            "chat send --stdin received empty stdin".into(),
        ));
    }
    Ok(text)
}

fn repl_turn_args(args: &ChatReplArgs, text: String, first_turn: bool) -> ChatSendArgs {
    let explicit_first_session =
        args.new_session || args.resume_session_id.is_some() || args.continue_session;
    ChatSendArgs {
        text,
        read_stdin: false,
        agent: args.agent.clone(),
        profile_id: args.profile_id.clone(),
        resume_session_id: if first_turn {
            args.resume_session_id.clone()
        } else {
            None
        },
        new_session: first_turn && (args.new_session || !explicit_first_session),
        continue_session: if first_turn {
            args.continue_session
        } else {
            true
        },
        workspace_path: args.workspace_path.clone(),
        permission_mode: args.permission_mode.clone(),
    }
}

fn is_repl_exit(input: &str) -> bool {
    matches!(input, "/exit" | "/quit" | ":q" | "exit" | "quit")
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ResolvedChatSession {
    resume_session_id: Option<String>,
    session_action: Option<ChatSessionAction>,
    session_workspace: Option<String>,
    store_scope: Option<ChatSessionScope>,
}

fn resolve_session(
    options: &Options,
    args: &ChatSendArgs,
) -> Result<ResolvedChatSession, CliError> {
    if !args.new_session && args.resume_session_id.is_none() && !args.continue_session {
        return Ok(ResolvedChatSession {
            resume_session_id: None,
            session_action: None,
            session_workspace: args.workspace_path.clone(),
            store_scope: None,
        });
    }

    let scope = scope_for_args(args)?;
    if args.continue_session {
        let session_id = saved_session_for(options, &scope)?.ok_or_else(|| {
            CliError::Chat(format!(
                "no saved chat session for {}; run `va chat send --new-session ...` or pass --resume SESSION first",
                scope.display()
            ))
        })?;
        return Ok(ResolvedChatSession {
            resume_session_id: Some(session_id),
            session_action: Some(ChatSessionAction::Resume),
            session_workspace: Some(scope.workspace.clone()),
            store_scope: Some(scope),
        });
    }

    Ok(ResolvedChatSession {
        resume_session_id: args.resume_session_id.clone(),
        session_action: if args.new_session {
            Some(ChatSessionAction::New)
        } else {
            Some(ChatSessionAction::Resume)
        },
        session_workspace: args
            .workspace_path
            .clone()
            .or_else(|| Some(scope.workspace.clone())),
        store_scope: Some(scope),
    })
}

fn message_from_args(args: &ChatSendArgs, session: &ResolvedChatSession) -> ChatClientMessage {
    ChatClientMessage::Message {
        text: args.text.clone(),
        message_id: None,
        agent: args.agent.clone(),
        profile_id: args.profile_id.clone(),
        session_action: session.session_action,
        session_id: session.resume_session_id.clone(),
        session_workspace: session.session_workspace.clone(),
        permission_mode: args.permission_mode.clone(),
        attachments: Vec::new(),
    }
}

enum ChatTerminalEvent {
    Continue,
    Done,
    Error(String),
}

fn terminal_event(event: &ChatEvent) -> ChatTerminalEvent {
    match event {
        ChatEvent::TurnStatus { active: false } => ChatTerminalEvent::Done,
        ChatEvent::Error { error } => ChatTerminalEvent::Error(error.clone()),
        _ => ChatTerminalEvent::Continue,
    }
}

fn prompt_permission_response(
    request_id: &str,
    request: &Value,
) -> Result<ChatClientMessage, CliError> {
    let options = permission_options(request);
    let title = permission_title(request);

    eprintln!("permission required: {title}");
    if options.is_empty() {
        eprintln!("no selectable permission options were provided; cancelling request");
        return Ok(ChatClientMessage::permission_cancelled(request_id));
    }

    for (index, option) in options.iter().enumerate() {
        let kind = option
            .kind
            .as_deref()
            .map(|kind| format!("; {kind}"))
            .unwrap_or_default();
        eprintln!(
            "  {}. {} ({}){}",
            index + 1,
            option.name,
            option.option_id,
            kind
        );
    }
    loop {
        eprint!(
            "select permission option [1-{}] or c to cancel: ",
            options.len()
        );
        std::io::stderr().flush().map_err(|source| CliError::Io {
            action: "flushing permission prompt",
            source,
        })?;

        let mut input = String::new();
        std::io::stdin()
            .read_line(&mut input)
            .map_err(|source| CliError::Io {
                action: "reading permission response",
                source,
            })?;
        let input = input.trim();
        if matches!(input, "c" | "C" | "cancel" | "Cancel") {
            return Ok(ChatClientMessage::permission_cancelled(request_id));
        }
        if let Ok(index) = input.parse::<usize>() {
            if let Some(option) = index.checked_sub(1).and_then(|index| options.get(index)) {
                return Ok(ChatClientMessage::permission_selected(
                    request_id,
                    option.option_id.clone(),
                ));
            }
        }
        if let Some(option) = options.iter().find(|option| option.option_id == input) {
            return Ok(ChatClientMessage::permission_selected(
                request_id,
                option.option_id.clone(),
            ));
        }
        eprintln!("invalid selection");
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PermissionOption {
    option_id: String,
    name: String,
    kind: Option<String>,
}

fn permission_title(request: &Value) -> String {
    request
        .get("toolCall")
        .and_then(|tool_call| {
            string_field(tool_call, "title").or_else(|| string_field(tool_call, "kind"))
        })
        .unwrap_or_else(|| "Permission requested".into())
}

fn permission_options(request: &Value) -> Vec<PermissionOption> {
    request
        .get("options")
        .and_then(Value::as_array)
        .map(|options| {
            options
                .iter()
                .filter_map(|option| {
                    let option_id = string_field(option, "optionId")?;
                    Some(PermissionOption {
                        name: string_field(option, "name").unwrap_or_else(|| option_id.clone()),
                        kind: string_field(option, "kind"),
                        option_id,
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

fn string_field(value: &Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

fn render_event(
    options: &Options,
    event: &ChatEvent,
    raw: &Value,
    wrote_text_chunk: &mut bool,
) -> Result<(), CliError> {
    if options.json {
        println!("{}", serde_json::to_string(raw)?);
        return Ok(());
    }

    match event {
        ChatEvent::AgentReady { agent, version } => {
            eprintln!("agent: {agent} {version}");
        }
        ChatEvent::SessionReady { session_id } => {
            eprintln!("session: {session_id}");
        }
        ChatEvent::SystemText { text } => {
            finish_text_line(*wrote_text_chunk)?;
            *wrote_text_chunk = false;
            println!("{text}");
        }
        ChatEvent::PermissionRequest {
            request_id,
            request,
        } => {
            finish_text_line(*wrote_text_chunk)?;
            *wrote_text_chunk = false;
            eprintln!("permission required: {request_id}");
            eprintln!("{}", serde_json::to_string_pretty(request)?);
        }
        ChatEvent::AcpNotification { payload } => {
            if let Some(text) = agent_message_text(payload) {
                print!("{text}");
                std::io::stdout().flush().map_err(|source| CliError::Io {
                    action: "flushing chat output",
                    source,
                })?;
                *wrote_text_chunk = true;
            }
        }
        ChatEvent::Error { error } => {
            finish_text_line(*wrote_text_chunk)?;
            *wrote_text_chunk = false;
            eprintln!("error: {error}");
        }
        ChatEvent::Config { .. }
        | ChatEvent::SessionInfo { .. }
        | ChatEvent::PreviewRefresh
        | ChatEvent::SessionMode { .. }
        | ChatEvent::CommandMenu { .. }
        | ChatEvent::MultiAgentTurn { .. }
        | ChatEvent::SubagentStatus { .. }
        | ChatEvent::SubagentAcpNotification { .. }
        | ChatEvent::TurnStatus { .. }
        | ChatEvent::ReplayStart { .. }
        | ChatEvent::ReplayDone { .. } => {}
    }
    Ok(())
}

fn agent_message_text(payload: &Value) -> Option<&str> {
    let update = payload.get("update")?;
    if update.get("sessionUpdate")?.as_str()? != "agent_message_chunk" {
        return None;
    }
    update.get("content")?.get("text")?.as_str()
}

fn finish_text_line(wrote_text_chunk: bool) -> Result<(), CliError> {
    if wrote_text_chunk {
        println!();
    }
    Ok(())
}

fn ws_error(url: &str, source: tokio_tungstenite::tungstenite::Error) -> CliError {
    CliError::WebSocket {
        url: crate::transport::redact_token_query(url),
        source,
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use serde_json::json;

    use super::*;

    #[test]
    fn cli_chat_socket_does_not_pin_a_chat_route() {
        assert_eq!(chat_ws().path, "/ws/chat");
        assert!(!chat_ws().path.contains("chat_id"));
    }

    #[test]
    fn inactive_turn_status_completes_one_shot_chat() {
        assert!(matches!(
            terminal_event(&ChatEvent::TurnStatus { active: false }),
            ChatTerminalEvent::Done
        ));
        assert!(matches!(
            terminal_event(&ChatEvent::TurnStatus { active: true }),
            ChatTerminalEvent::Continue
        ));
    }

    #[test]
    fn chat_send_args_build_resume_message() {
        let message = message_from_args(
            &ChatSendArgs {
                text: "hello".into(),
                read_stdin: false,
                agent: Some("codex".into()),
                profile_id: Some("deepseek".into()),
                resume_session_id: Some("sid-1".into()),
                new_session: false,
                continue_session: false,
                workspace_path: Some("/tmp/project".into()),
                permission_mode: Some("acceptEdits".into()),
            },
            &ResolvedChatSession {
                resume_session_id: Some("sid-1".into()),
                session_action: Some(ChatSessionAction::Resume),
                session_workspace: Some("/tmp/project".into()),
                store_scope: None,
            },
        );

        let value = serde_json::to_value(message).expect("json");
        assert_eq!(
            value,
            json!({
                "type": "message",
                "text": "hello",
                "agent": "codex",
                "profileId": "deepseek",
                "sessionAction": "resume",
                "sessionId": "sid-1",
                "sessionWorkspace": "/tmp/project",
                "permissionMode": "acceptEdits"
            })
        );
    }

    #[test]
    fn resolve_continue_session_from_store() {
        let root = std::env::temp_dir().join(format!(
            "va-cli-chat-command-test-{}-{}",
            std::process::id(),
            line!()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("dir");
        let options = Options {
            auth_file: Some(root.join("auth.json")),
            ..Default::default()
        };
        let args = ChatSendArgs {
            text: "hello".into(),
            read_stdin: false,
            agent: Some("codex".into()),
            profile_id: Some("default".into()),
            resume_session_id: None,
            new_session: false,
            continue_session: true,
            workspace_path: Some("/tmp/project".into()),
            permission_mode: None,
        };
        let scope = scope_for_args(&args).expect("scope");
        save_session_for(&options, &scope, "session-1").expect("save");

        let session = resolve_session(&options, &args).expect("session");

        assert_eq!(session.resume_session_id.as_deref(), Some("session-1"));
        assert_eq!(session.session_action, Some(ChatSessionAction::Resume));
        assert_eq!(session.session_workspace.as_deref(), Some("/tmp/project"));
        assert_eq!(session.store_scope, Some(scope));

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn repl_default_first_turn_starts_new_then_continues() {
        let args = ChatReplArgs {
            agent: Some("codex".into()),
            profile_id: Some("default".into()),
            resume_session_id: None,
            new_session: false,
            continue_session: false,
            workspace_path: Some("/tmp/project".into()),
            permission_mode: Some("acceptEdits".into()),
        };

        let first = repl_turn_args(&args, "hello".into(), true);
        assert_eq!(first.text, "hello");
        assert!(first.new_session);
        assert!(!first.continue_session);
        assert_eq!(first.resume_session_id, None);
        assert_eq!(first.workspace_path.as_deref(), Some("/tmp/project"));
        assert_eq!(first.permission_mode.as_deref(), Some("acceptEdits"));

        let next = repl_turn_args(&args, "again".into(), false);
        assert_eq!(next.text, "again");
        assert!(!next.new_session);
        assert!(next.continue_session);
        assert_eq!(next.resume_session_id, None);
    }

    #[test]
    fn repl_preserves_explicit_first_resume() {
        let args = ChatReplArgs {
            agent: None,
            profile_id: None,
            resume_session_id: Some("session-1".into()),
            new_session: false,
            continue_session: false,
            workspace_path: None,
            permission_mode: None,
        };

        let first = repl_turn_args(&args, "hello".into(), true);
        assert!(!first.new_session);
        assert!(!first.continue_session);
        assert_eq!(first.resume_session_id.as_deref(), Some("session-1"));
    }

    #[test]
    fn repl_exit_commands_are_recognized() {
        assert!(is_repl_exit("/exit"));
        assert!(is_repl_exit(":q"));
        assert!(is_repl_exit("quit"));
        assert!(!is_repl_exit("hello"));
    }

    #[test]
    fn extracts_agent_message_text_from_acp_payload() {
        assert_eq!(
            agent_message_text(&json!({
                "update": {
                    "sessionUpdate": "agent_message_chunk",
                    "content": {
                        "type": "text",
                        "text": "hello"
                    }
                }
            })),
            Some("hello")
        );
        assert_eq!(
            agent_message_text(&json!({
                "update": {
                    "sessionUpdate": "tool_call"
                }
            })),
            None
        );
    }

    #[test]
    fn extracts_permission_prompt_options() {
        let request = json!({
            "toolCall": {
                "title": "Read file"
            },
            "options": [
                {
                    "optionId": "allow_once",
                    "name": "Allow",
                    "kind": "accept"
                },
                {
                    "optionId": "reject"
                },
                {
                    "name": "Missing id"
                }
            ]
        });

        assert_eq!(permission_title(&request), "Read file");
        assert_eq!(
            permission_options(&request),
            vec![
                PermissionOption {
                    option_id: "allow_once".into(),
                    name: "Allow".into(),
                    kind: Some("accept".into()),
                },
                PermissionOption {
                    option_id: "reject".into(),
                    name: "reject".into(),
                    kind: None,
                },
            ]
        );
    }
}
