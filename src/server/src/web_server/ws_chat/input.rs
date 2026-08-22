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
    Cancel(ChannelInput),
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
                "cancel" => Some(WebChatInput::Cancel(ChannelInput::Cancel {
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

#[cfg(test)]
mod tests {
    use agent_client_protocol::schema::v1 as acp;
    use common::channels::{ChannelEnvelope, ChannelInput};
    use common::routing::RouteKey;

    use super::*;

    fn parse_web_chat_input(chat_id: &str, text: &str) -> Option<WebChatInput> {
        let route = RouteKey::new("web", chat_id);
        super::parse_web_chat_input(&route, "web-user", text)
    }

    #[test]
    fn parses_tui_message_with_tui_route_identity() {
        let input = super::parse_web_chat_input(
            &RouteKey::new("tui", "chat-1"),
            "tui-user",
            r#"{"type":"message","text":"hello"}"#,
        )
        .expect("message input");

        let WebChatInput::Message {
            input:
                ChannelInput::Message {
                    envelope:
                        ChannelEnvelope {
                            route, sender_id, ..
                        },
                },
            ..
        } = input
        else {
            panic!("expected tui message");
        };

        assert_eq!(route, RouteKey::new("tui", "chat-1"));
        assert_eq!(sender_id, "tui-user");
    }

    #[test]
    fn parses_selected_permission_response() {
        let input = parse_web_chat_input(
            "chat-1",
            r#"{"type":"permission_response","requestId":"req-1","optionId":"allow-once"}"#,
        )
        .expect("permission response");

        let WebChatInput::PermissionResponse {
            request_id,
            response,
        } = input
        else {
            panic!("expected permission response");
        };

        assert_eq!(request_id, "req-1");
        match response.outcome {
            acp::RequestPermissionOutcome::Selected(selected) => {
                assert_eq!(selected.option_id.to_string(), "allow-once");
            }
            acp::RequestPermissionOutcome::Cancelled => panic!("expected selected outcome"),
            _ => panic!("expected selected outcome"),
        }
    }

    #[test]
    fn parses_cancelled_permission_response() {
        let input = parse_web_chat_input(
            "chat-1",
            r#"{"type":"permission_response","requestId":"req-2","outcome":"cancelled"}"#,
        )
        .expect("permission response");

        let WebChatInput::PermissionResponse {
            request_id,
            response,
        } = input
        else {
            panic!("expected permission response");
        };

        assert_eq!(request_id, "req-2");
        assert!(matches!(
            response.outcome,
            acp::RequestPermissionOutcome::Cancelled
        ));
    }

    #[test]
    fn parses_cancel_message() {
        let input = parse_web_chat_input("chat-1", r#"{"type":"cancel"}"#).expect("cancel input");

        let WebChatInput::Cancel(ChannelInput::Cancel { route }) = input else {
            panic!("expected cancel input");
        };

        assert_eq!(route, RouteKey::new("web", "chat-1"));
    }

    #[test]
    fn parses_resume_session_intent() {
        let input = parse_web_chat_input(
            "chat-1",
            r#"{"type":"message","text":"continue","agent":"codex","sessionAction":"resume","sessionId":"sid-1","sessionWorkspace":"/tmp/project"}"#,
        )
        .expect("message input");

        let WebChatInput::Message {
            input:
                ChannelInput::Message {
                    envelope:
                        ChannelEnvelope {
                            cli_kind: Some(agent),
                            ..
                        },
                },
            profile: None,
            session_intent:
                Some(WebChatSessionIntent::Resume {
                    agent: Some(intent_agent),
                    session_id,
                    cwd: Some(cwd),
                }),
            session_mode: None,
        } = input
        else {
            panic!("expected resume message");
        };

        assert_eq!(agent, "codex");
        assert_eq!(intent_agent, "codex");
        assert_eq!(session_id, "sid-1");
        assert_eq!(cwd, "/tmp/project");
    }

    #[test]
    fn parses_direct_resume_session() {
        let input = parse_web_chat_input(
            "chat-1",
            r#"{"type":"resume_session","agent":"codex","profileId":"deepseek","sessionId":"sid-1","sessionWorkspace":"/tmp/project"}"#,
        )
        .expect("resume session input");

        let WebChatInput::ResumeSession {
            agent: Some(agent),
            profile: Some(profile),
            session_id,
            cwd: Some(cwd),
        } = input
        else {
            panic!("expected direct resume input");
        };

        assert_eq!(agent, "codex");
        assert_eq!(profile, "deepseek");
        assert_eq!(session_id, "sid-1");
        assert_eq!(cwd, "/tmp/project");
    }

    #[test]
    fn parses_new_session_intent() {
        let input = parse_web_chat_input(
            "chat-1",
            r#"{"type":"message","text":"start over","sessionAction":"new"}"#,
        )
        .expect("message input");

        let WebChatInput::Message {
            session_intent: Some(WebChatSessionIntent::New { cwd: None }),
            session_mode: None,
            ..
        } = input
        else {
            panic!("expected new-session message");
        };
    }

    #[test]
    fn parses_new_session_workspace() {
        let input = parse_web_chat_input(
            "chat-1",
            r#"{"type":"message","text":"start here","sessionAction":"new","sessionWorkspace":"/tmp/new-project"}"#,
        )
        .expect("message input");

        let WebChatInput::Message {
            session_intent: Some(WebChatSessionIntent::New { cwd: Some(cwd) }),
            session_mode: None,
            ..
        } = input
        else {
            panic!("expected new-session message with workspace");
        };

        assert_eq!(cwd, "/tmp/new-project");
    }

    #[test]
    fn parses_profile_selection() {
        let input = parse_web_chat_input(
            "chat-1",
            r#"{"type":"message","text":"hello","agent":"claude","profileId":"deepseek"}"#,
        )
        .expect("message input");

        let WebChatInput::Message {
            profile: Some(profile),
            session_mode: None,
            ..
        } = input
        else {
            panic!("expected profile message");
        };

        assert_eq!(profile, "deepseek");
    }

    #[test]
    fn parses_message_permission_mode() {
        let input = parse_web_chat_input(
            "chat-1",
            r#"{"type":"message","text":"hello","permissionMode":"acceptEdits"}"#,
        )
        .expect("message input");

        let WebChatInput::Message {
            session_mode: Some(mode_id),
            ..
        } = input
        else {
            panic!("expected message mode");
        };

        assert_eq!(mode_id, "acceptEdits");
    }

    #[test]
    fn parses_set_mode_message() {
        let input = parse_web_chat_input(
            "chat-1",
            r#"{"type":"set_mode","modeId":"bypassPermissions"}"#,
        )
        .expect("set mode input");

        let WebChatInput::SetMode { mode_id } = input else {
            panic!("expected set mode");
        };

        assert_eq!(mode_id, "bypassPermissions");
    }

    #[test]
    fn parses_set_config_option_message() {
        let input = parse_web_chat_input(
            "chat-1",
            r#"{"type":"set_config_option","configId":"permissions","value":"fullAccess"}"#,
        )
        .expect("set config option input");

        let WebChatInput::SetConfigOption { config_id, value } = input else {
            panic!("expected set config option");
        };

        assert_eq!(config_id, "permissions");
        assert_eq!(value, "fullAccess");
    }

    #[test]
    fn parses_message_attachments() {
        let input = parse_web_chat_input(
            "chat-1",
            r#"{"type":"message","messageId":"msg-1","agent":"codex","attachments":[{"uri":"file:///tmp/report.md","name":"report.md","mimeType":"text/markdown","size":42}]}"#,
        )
        .expect("message input");

        let WebChatInput::Message {
            input:
                ChannelInput::Message {
                    envelope:
                        ChannelEnvelope {
                            message_id,
                            attachments,
                            ..
                        },
                },
            session_mode: None,
            ..
        } = input
        else {
            panic!("expected attachment message");
        };

        assert_eq!(message_id, "msg-1");
        assert_eq!(attachments.len(), 1);
        assert_eq!(attachments[0].message_id, "msg-1");
        assert_eq!(attachments[0].file_key, "file:///tmp/report.md");
        assert_eq!(attachments[0].file_name, "report.md");
        assert_eq!(attachments[0].resource_type, "text/markdown");
        assert_eq!(attachments[0].size, Some(42));
    }

    #[test]
    fn dedupes_message_attachments() {
        let input = parse_web_chat_input(
            "chat-1",
            r#"{"type":"message","messageId":"msg-1","attachments":[{"uri":"file:///tmp/logo.png","name":"Logo.png","mimeType":"image/png","size":42},{"uri":"file:///tmp/logo.png","name":"Logo.png","mimeType":"image/png","size":42}]}"#,
        )
        .expect("message input");

        let WebChatInput::Message {
            input:
                ChannelInput::Message {
                    envelope: ChannelEnvelope { attachments, .. },
                },
            ..
        } = input
        else {
            panic!("expected attachment message");
        };

        assert_eq!(attachments.len(), 1);
        assert_eq!(attachments[0].file_key, "file:///tmp/logo.png");
    }

    #[test]
    fn rejects_unsafe_relative_attachment_keys() {
        let input = parse_web_chat_input(
            "chat-1",
            r#"{"type":"message","messageId":"msg-1","text":"see file","attachments":[{"fileKey":"../secret","name":"secret.txt"}]}"#,
        )
        .expect("message input");

        let WebChatInput::Message {
            input:
                ChannelInput::Message {
                    envelope: ChannelEnvelope { attachments, .. },
                },
            ..
        } = input
        else {
            panic!("expected attachment message");
        };

        assert!(attachments.is_empty());
    }
}
