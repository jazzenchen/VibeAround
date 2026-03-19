//! Feishu webhook handler: url_verification + im.message.receive_v1 event processing.

use std::sync::Arc;

use dashmap::DashMap;
use tokio::sync::mpsc;

use super::transport::FeishuTransport;
use crate::config;
use crate::im::daemon::{OutboundHub, OutboundMsg};
use crate::im::log::{prefix_channel, truncate_content_default};
use crate::im::worker::{FeishuAttachment, InboundMessage};

/// State passed to the web server to handle Feishu webhook.
#[derive(Clone)]
pub struct FeishuWebhookState {
    pub inbound_tx: mpsc::Sender<InboundMessage>,
    pub outbound: Arc<OutboundHub<FeishuTransport>>,
    pub busy_set: Arc<DashMap<String, ()>>,
    pub transport: Arc<FeishuTransport>,
}

/// Handle Feishu webhook body. Returns (status_code, body_json_string).
pub async fn handle_webhook_body(
    body: &str,
    state: Option<&FeishuWebhookState>,
) -> (u16, String) {
    const P: &str = "[VibeAround][im][feishu]";

    let root: serde_json::Value = match serde_json::from_str(body) {
        Ok(v) => v,
        Err(_) => return (400, "{}".to_string()),
    };

    if root.get("encrypt").is_some() && root.get("type").is_none() {
        return (200, "{}".to_string());
    }

    let ty = root.get("type").or_else(|| root.get("Type")).and_then(|t| t.as_str());
    if ty == Some("url_verification") {
        let challenge = root.get("challenge").and_then(|c| c.as_str()).unwrap_or("");
        return (200, serde_json::json!({ "challenge": challenge }).to_string());
    }

    let event = match root.get("event") {
        Some(e) => e,
        None => return (200, "{}".to_string()),
    };
    let event_type = root.get("header").and_then(|h| h.get("event_type")).and_then(|t| t.as_str())
        .or_else(|| event.get("type").and_then(|t| t.as_str()))
        .or_else(|| event.get("event_type").and_then(|t| t.as_str()));
    if event_type != Some("im.message.receive_v1") {
        return (200, "{}".to_string());
    }

    let Some(st) = state else { return (200, "{}".to_string()); };

    let message = match event.get("message") {
        Some(m) => m,
        None => return (200, "{}".to_string()),
    };
    let chat_id = match message.get("chat_id").and_then(|c| c.as_str()) {
        Some(c) => c,
        None => return (200, "{}".to_string()),
    };
    let message_id = message.get("message_id").and_then(|m| m.as_str()).unwrap_or("");
    let parent_id = message.get("parent_id").and_then(|p| p.as_str())
        .filter(|s| !s.is_empty()).map(String::from);
    let msg_type = message.get("message_type").and_then(|t| t.as_str()).unwrap_or("text");
    let content_str = match message.get("content").and_then(|c| c.as_str()) {
        Some(c) => c,
        None => return (200, "{}".to_string()),
    };
    let content: serde_json::Value = match serde_json::from_str(content_str) {
        Ok(v) => v,
        Err(_) => return (200, "{}".to_string()),
    };

    let channel_id = format!("feishu:{}", chat_id);
    let user_message_id = Some(message_id.to_string());

    let mut inbound = match msg_type {
        "file" => {
            let file_key = content.get("file_key").and_then(|k| k.as_str()).unwrap_or("");
            let file_name = content.get("file_name").and_then(|n| n.as_str()).unwrap_or("unknown");
            if file_key.is_empty() { return (200, "{}".to_string()); }
            eprintln!("{} chat_id={} message_id={} direction=incoming type=file file_name={}",
                P, chat_id, message_id, file_name);
            InboundMessage {
                channel_id: channel_id.clone(),
                text: format!("I uploaded a file: {}. Please read and analyze it.", file_name),
                attachments: vec![FeishuAttachment {
                    message_id: message_id.to_string(), file_key: file_key.to_string(),
                    file_name: file_name.to_string(), resource_type: "file".to_string(),
                }],
                parent_id: parent_id.clone(),
                user_message_id: user_message_id.clone(),
            }
        }
        "image" => {
            let image_key = content.get("image_key").and_then(|k| k.as_str()).unwrap_or("");
            if image_key.is_empty() { return (200, "{}".to_string()); }
            let image_name = format!("{}.png", &image_key.chars().take(16).collect::<String>());
            eprintln!("{} chat_id={} message_id={} direction=incoming type=image image_key={}",
                P, chat_id, message_id, image_key);
            InboundMessage {
                channel_id: channel_id.clone(),
                text: "I uploaded an image. Please analyze it.".to_string(),
                attachments: vec![FeishuAttachment {
                    message_id: message_id.to_string(), file_key: image_key.to_string(),
                    file_name: image_name, resource_type: "image".to_string(),
                }],
                parent_id: parent_id.clone(),
                user_message_id: user_message_id.clone(),
            }
        }
        _ => {
            let text = content.get("text").and_then(|t| t.as_str()).unwrap_or("").trim().to_string();
            if text.is_empty() { return (200, "{}".to_string()); }
            InboundMessage {
                channel_id: channel_id.clone(), text, attachments: vec![],
                parent_id: parent_id.clone(), user_message_id: user_message_id.clone(),
            }
        }
    };

    // If this message quotes another, fetch the parent and merge file/image attachments
    if let Some(pid) = &parent_id {
        match st.transport.get_message(chat_id, pid).await {
            Ok((ref mt, ref content)) if mt == "file" => {
                if let (Some(fk), Some(fn_)) = (
                    content.get("file_key").and_then(|k| k.as_str()),
                    content.get("file_name").and_then(|n| n.as_str()),
                ) {
                    if !fk.is_empty() {
                        inbound.attachments.push(FeishuAttachment {
                            message_id: pid.clone(), file_key: fk.to_string(),
                            file_name: fn_.to_string(), resource_type: "file".to_string(),
                        });
                    }
                }
            }
            Ok((ref mt, ref content)) if mt == "image" => {
                if let Some(ik) = content.get("image_key").and_then(|k| k.as_str()) {
                    if !ik.is_empty() {
                        let name = format!("{}.png", &ik.chars().take(16).collect::<String>());
                        inbound.attachments.push(FeishuAttachment {
                            message_id: pid.clone(), file_key: ik.to_string(),
                            file_name: name, resource_type: "image".to_string(),
                        });
                    }
                }
            }
            Ok(_) => {}
            Err(e) => eprintln!("{} get_message parent_id={} err={:?}", P, pid, e),
        }
    }

    eprintln!("{} chat_id={} message_id={} direction=incoming content={} attachments={}",
        P, chat_id, message_id, truncate_content_default(&inbound.text), inbound.attachments.len());

    if st.busy_set.contains_key(&channel_id) {
        let _ = st.outbound.send(&channel_id, OutboundMsg::Send(
            channel_id.clone(), "Please wait for the current task to finish.".to_string())).await;
        return (200, "{}".to_string());
    }

    if st.inbound_tx.try_send(inbound).is_err() {
        eprintln!("{} chat_id={} message_id={} event=inbound_queue_full dropped=1", P, chat_id, message_id);
    }
    (200, "{}".to_string())
}

/// Setup Feishu: create transport, outbound, worker; return state for webhook route.
/// Returns None if feishu app_id/app_secret not configured.
pub async fn run_feishu_bot(services: Arc<crate::service::ServiceManager>) -> Option<FeishuWebhookState> {
    let config = config::ensure_loaded();
    let app_id = match config.feishu_app_id.as_deref() {
        Some(id) if !id.trim().is_empty() => id.to_string(),
        _ => {
            eprintln!("{} config=missing app_id Feishu disabled", prefix_channel("feishu"));
            return None;
        }
    };
    let app_secret = match config.feishu_app_secret.as_deref() {
        Some(s) if !s.trim().is_empty() => s.to_string(),
        _ => {
            eprintln!("{} config=missing app_secret Feishu disabled", prefix_channel("feishu"));
            return None;
        }
    };

    let (inbound_tx, inbound_rx) = mpsc::channel(64);
    let busy_set: Arc<DashMap<String, ()>> = Arc::new(DashMap::new());
    let transport = Arc::new(FeishuTransport::new(
        app_id,
        app_secret,
    ));
    let outbound = OutboundHub::new(transport.clone());

    tokio::spawn(crate::im::worker::run_worker(
        inbound_rx, outbound.clone(), busy_set.clone(), Some(transport.clone()),
        config.feishu_verbose.clone(),
        services,
    ));

    eprintln!("{} event=bot_ready webhook=/api/im/feishu/event", prefix_channel("feishu"));
    Some(FeishuWebhookState { inbound_tx, outbound, busy_set, transport })
}

/// Handle Feishu card callback (button clicks). Returns (status_code, body_json_string).
/// Feishu requires a response within 3 seconds.
/// The callback data contains action.value (our button value) and context.open_chat_id.
pub async fn handle_card_callback(
    body: &str,
    state: Option<&FeishuWebhookState>,
) -> (u16, String) {
    eprintln!("[VibeAround][im][feishu] card_callback received body={}", &body[..body.len().min(500)]);

    let root: serde_json::Value = match serde_json::from_str(body) {
        Ok(v) => v,
        Err(_) => return (200, "{}".to_string()),
    };

    // Handle challenge verification (sent when configuring the callback URL)
    let ty = root.get("type").or_else(|| root.get("Type")).and_then(|t| t.as_str());
    if ty == Some("url_verification") {
        let challenge = root.get("challenge").and_then(|c| c.as_str()).unwrap_or("");
        return (200, serde_json::json!({ "challenge": challenge }).to_string());
    }

    let Some(st) = state else { return (200, "{}".to_string()); };

    // v2 callback: action in /event/action/value, chat_id in /event/context/open_chat_id
    let action_value = root.pointer("/event/action/value");
    let chat_id = root.pointer("/event/context/open_chat_id").and_then(|v| v.as_str());

    let Some(chat_id) = chat_id else {
        eprintln!("[VibeAround][im][feishu] card_callback missing open_chat_id");
        return (200, "{}".to_string());
    };

    // The action value contains {"action": "/cli claude"} or similar
    let command = action_value
        .and_then(|v| v.get("action"))
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim()
        .to_string();

    if command.is_empty() {
        eprintln!("[VibeAround][im][feishu] card_callback empty command");
        return (200, "{}".to_string());
    }

    let channel_id = format!("feishu:{}", chat_id);
    eprintln!("[VibeAround][im][feishu] card_callback chat_id={} command={}", chat_id, command);

    if st.busy_set.contains_key(&channel_id) {
        let _ = st.outbound.send(&channel_id, OutboundMsg::Send(
            channel_id.clone(), "Please wait for the current task to finish.".to_string())).await;
        return (200, "{}".to_string());
    }

    let _ = st.inbound_tx.send(InboundMessage {
        channel_id,
        text: command,
        attachments: vec![],
        parent_id: None,
        user_message_id: None,
    }).await;

    (200, "{}".to_string())
}
