use std::io::Write;

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
                    ChatTerminalEvent::PermissionRequired => {
                        let _ = ws_tx.close().await;
                        return Err(CliError::Chat(
                            "permission request received; interactive chat send is not supported yet"
                                .into(),
                        ));
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
    PermissionRequired,
    Error(String),
}

fn terminal_event(event: &ChatEvent) -> ChatTerminalEvent {
    match event {
        ChatEvent::PromptDone { .. } => ChatTerminalEvent::Done,
        ChatEvent::PermissionRequest { .. } => ChatTerminalEvent::PermissionRequired,
        ChatEvent::Error { error } => ChatTerminalEvent::Error(error.clone()),
        _ => ChatTerminalEvent::Continue,
    }
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
}
