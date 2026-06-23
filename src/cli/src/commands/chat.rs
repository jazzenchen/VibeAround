use std::io::{IsTerminal, Write};

use futures_util::{SinkExt, StreamExt};
use serde_json::Value;
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::Message;
use va_client::events::{
    chat_ws, decode_chat_event, encode_chat_client_message, ChatClientMessage, ChatEvent,
    ChatSessionAction,
};
use va_client::http::AuthRequirement;
use va_client::state::ChatState;

use crate::args::{ChatSendArgs, Options};
use crate::config::endpoint_for;
use crate::error::CliError;

pub(super) async fn send(options: &Options, args: &ChatSendArgs) -> Result<(), CliError> {
    let endpoint = endpoint_for(options, AuthRequirement::BearerToken)?;
    let socket = chat_ws();
    let url = endpoint.websocket_url(&socket);
    let (ws, _) = connect_async(&url)
        .await
        .map_err(|source| ws_error(&url, source))?;
    let (mut ws_tx, mut ws_rx) = ws.split();

    let body = encode_chat_client_message(&message_from_args(args))?;
    ws_tx
        .send(Message::Text(body.into()))
        .await
        .map_err(|source| ws_error(&url, source))?;

    let mut state = ChatState::new();
    let mut wrote_text_chunk = false;
    while let Some(frame) = ws_rx.next().await {
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
        "chat websocket closed before prompt_done".into(),
    ))
}

fn message_from_args(args: &ChatSendArgs) -> ChatClientMessage {
    ChatClientMessage::Message {
        text: args.text.clone(),
        message_id: None,
        agent: args.agent.clone(),
        profile_id: args.profile_id.clone(),
        session_action: if args.new_session {
            Some(ChatSessionAction::New)
        } else if args.resume_session_id.is_some() {
            Some(ChatSessionAction::Resume)
        } else {
            None
        },
        session_id: args.resume_session_id.clone(),
        session_workspace: args.workspace_path.clone(),
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
        ChatEvent::PromptDone { .. } => ChatTerminalEvent::Done,
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
        | ChatEvent::SessionMode { .. }
        | ChatEvent::CommandMenu { .. }
        | ChatEvent::MultiAgentTurn { .. }
        | ChatEvent::SubagentStatus { .. }
        | ChatEvent::SubagentAcpNotification { .. }
        | ChatEvent::PromptDone { .. }
        | ChatEvent::TurnStatus { .. } => {}
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
    use serde_json::json;

    use super::*;

    #[test]
    fn chat_send_args_build_resume_message() {
        let message = message_from_args(&ChatSendArgs {
            text: "hello".into(),
            agent: Some("codex".into()),
            profile_id: Some("deepseek".into()),
            resume_session_id: Some("sid-1".into()),
            new_session: false,
            workspace_path: Some("/tmp/project".into()),
            permission_mode: Some("acceptEdits".into()),
        });

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
