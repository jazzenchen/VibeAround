use std::collections::HashSet;

use agent_client_protocol::schema::v1 as acp;
use common::channels::{ChannelEnvelope, ChannelInput};
use common::routing::{is_safe_attachment_file_key, Attachment, RouteKey};
use uuid::Uuid;

pub(super) enum WebChatInput {
    Message {
        input: ChannelInput,
        profile: Option<String>,
        session_intent: Option<WebChatSessionIntent>,
        session_mode: Option<String>,
    },
    SetMode {
        mode_id: String,
    },
    SetConfigOption {
        config_id: String,
        value: String,
    },
    Stop(ChannelInput),
    PermissionResponse {
        request_id: String,
        response: acp::RequestPermissionResponse,
    },
    ResumeSession {
        agent: Option<String>,
        profile: Option<String>,
        session_id: String,
        cwd: Option<String>,
    },
}

pub(super) enum WebChatSessionIntent {
    Resume {
        agent: Option<String>,
        session_id: String,
        cwd: Option<String>,
    },
    New {
        cwd: Option<String>,
    },
}

pub(super) fn parse_web_chat_input(
    route: &RouteKey,
    sender_id: &str,
    text: &str,
) -> Option<WebChatInput> {
    let parsed = serde_json::from_str::<serde_json::Value>(text);

    match parsed {
        Ok(v) => {
            let ty = v.get("type").and_then(|x| x.as_str()).unwrap_or("");
            match ty {
                "message" => {
                    let text = v.get("text").and_then(|x| x.as_str()).unwrap_or("").trim();
                    let message_id = v
                        .get("messageId")
                        .and_then(|x| x.as_str())
                        .map(ToOwned::to_owned)
                        .unwrap_or_else(|| Uuid::new_v4().to_string());
                    let attachments = parse_web_attachments(&v, &message_id);
                    if text.is_empty() && attachments.is_empty() {
                        return None;
                    }
                    let agent = parse_web_agent(&v);
                    let session_intent = parse_web_session_intent(&v, agent.clone());
                    let profile = parse_web_profile(&v);
                    let session_mode = parse_web_session_mode(&v);
                    Some(WebChatInput::Message {
                        input: ChannelInput::Message {
                            envelope: ChannelEnvelope {
                                route: route.clone(),
                                message_id,
                                turn_id: None,
                                text: text.to_string(),
                                sender_id: sender_id.to_string(),
                                attachments,
                                parent_id: None,
                                cli_kind: agent,
                            },
                        },
                        profile,
                        session_intent,
                        session_mode,
                    })
                }
                "set_mode" => {
                    let mode_id = string_field(&v, &["modeId", "mode_id", "permissionMode"])?;
                    Some(WebChatInput::SetMode { mode_id })
                }
                "set_config_option" => {
                    let config_id = string_field(&v, &["configId", "config_id"])?;
                    let value = string_field(&v, &["value"])?;
                    Some(WebChatInput::SetConfigOption { config_id, value })
                }
                "resume_session" => {
                    let agent = parse_web_agent(&v);
                    let profile = parse_web_profile(&v);
                    let session_id = v
                        .get("sessionId")
                        .and_then(|x| x.as_str())
                        .map(str::trim)
                        .filter(|x| !x.is_empty())?
                        .to_string();
                    let cwd = v
                        .get("sessionWorkspace")
                        .and_then(|x| x.as_str())
                        .map(str::trim)
                        .filter(|x| !x.is_empty())
                        .map(ToOwned::to_owned);

                    Some(WebChatInput::ResumeSession {
                        agent,
                        profile,
                        session_id,
                        cwd,
                    })
                }
                "stop" => Some(WebChatInput::Stop(ChannelInput::Stop {
                    route: route.clone(),
                })),
                "permission_response" => {
                    let request_id = v.get("requestId").and_then(|x| x.as_str())?.to_string();
                    let outcome = match v.get("outcome").and_then(|x| x.as_str()) {
                        Some("cancelled") => acp::RequestPermissionOutcome::Cancelled,
                        _ => {
                            let option_id = v.get("optionId").and_then(|x| x.as_str())?;
                            acp::RequestPermissionOutcome::Selected(
                                acp::SelectedPermissionOutcome::new(option_id.to_string()),
                            )
                        }
                    };
                    Some(WebChatInput::PermissionResponse {
                        request_id,
                        response: acp::RequestPermissionResponse::new(outcome),
                    })
                }
                _ => None,
            }
        }
        Err(_) => {
            let trimmed = text.trim();
            if trimmed.is_empty() {
                None
            } else {
                Some(WebChatInput::Message {
                    input: ChannelInput::Message {
                        envelope: ChannelEnvelope {
                            route: route.clone(),
                            message_id: Uuid::new_v4().to_string(),
                            turn_id: None,
                            text: trimmed.to_string(),
                            sender_id: sender_id.to_string(),
                            attachments: vec![],
                            parent_id: None,
                            cli_kind: None,
                        },
                    },
                    profile: None,
                    session_intent: None,
                    session_mode: None,
                })
            }
        }
    }
}

fn parse_web_attachments(value: &serde_json::Value, message_id: &str) -> Vec<Attachment> {
    value
        .get("attachments")
        .and_then(|items| items.as_array())
        .map(|items| {
            let mut seen = HashSet::new();
            items
                .iter()
                .filter_map(|item| parse_web_attachment(item, message_id))
                .filter(|attachment| {
                    let key = format!(
                        "{}\u{0}{}\u{0}{}\u{0}{}",
                        attachment.file_key,
                        attachment.file_name,
                        attachment.resource_type,
                        attachment.size.unwrap_or_default()
                    );
                    seen.insert(key)
                })
                .collect()
        })
        .unwrap_or_default()
}

fn parse_web_attachment(value: &serde_json::Value, message_id: &str) -> Option<Attachment> {
    let file_key = string_field(value, &["fileKey", "file_key", "uri", "url"])?;
    if !is_safe_attachment_file_key(&file_key) {
        tracing::warn!(file_key = %file_key, "dropping web attachment with unsafe file key");
        return None;
    }
    let file_name = string_field(value, &["fileName", "file_name", "name"]).unwrap_or_else(|| {
        file_key
            .rsplit('/')
            .next()
            .unwrap_or("attachment")
            .to_string()
    });
    let resource_type = string_field(
        value,
        &["resourceType", "resource_type", "mimeType", "mime_type"],
    )
    .unwrap_or_else(|| "application/octet-stream".to_string());
    let size = value
        .get("size")
        .and_then(|size| {
            size.as_i64()
                .or_else(|| size.as_u64().map(|size| size as i64))
        })
        .filter(|size| *size >= 0);

    Some(Attachment {
        message_id: message_id.to_string(),
        file_key,
        file_name,
        resource_type,
        size,
    })
}

fn string_field(value: &serde_json::Value, keys: &[&str]) -> Option<String> {
    keys.iter()
        .find_map(|key| value.get(*key).and_then(|item| item.as_str()))
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

fn parse_web_agent(value: &serde_json::Value) -> Option<String> {
    value
        .get("agent")
        .and_then(|x| x.as_str())
        .map(str::trim)
        .filter(|x| !x.is_empty())
        .map(ToOwned::to_owned)
}

fn parse_web_profile(value: &serde_json::Value) -> Option<String> {
    value
        .get("profileId")
        .and_then(|x| x.as_str())
        .map(str::trim)
        .filter(|x| !x.is_empty())
        .map(ToOwned::to_owned)
}

fn parse_web_session_mode(value: &serde_json::Value) -> Option<String> {
    string_field(value, &["permissionMode", "modeId", "mode_id"])
}

fn parse_web_session_intent(
    value: &serde_json::Value,
    agent: Option<String>,
) -> Option<WebChatSessionIntent> {
    match value.get("sessionAction").and_then(|x| x.as_str()) {
        Some("new") => {
            return Some(WebChatSessionIntent::New {
                cwd: parse_web_session_workspace(value),
            });
        }
        Some("resume") | None => {}
        Some(_) => return None,
    }

    let session_id = value
        .get("sessionId")
        .and_then(|x| x.as_str())
        .map(str::trim)
        .filter(|x| !x.is_empty())?;
    let cwd = parse_web_session_workspace(value);

    Some(WebChatSessionIntent::Resume {
        agent,
        session_id: session_id.to_string(),
        cwd,
    })
}

fn parse_web_session_workspace(value: &serde_json::Value) -> Option<String> {
    value
        .get("sessionWorkspace")
        .and_then(|x| x.as_str())
        .map(str::trim)
        .filter(|x| !x.is_empty())
        .map(ToOwned::to_owned)
}
