//! Feishu (Lark) IM: HTTP API with tenant_access_token, webhook for events.
//! Send message via open-apis/im/v1/messages; receive via event subscription (url_verification + im.message.receive_v1).

use std::sync::Arc;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use dashmap::DashMap;
use tokio::sync::mpsc;

use crate::config;
use crate::im::daemon::{OutboundHub, OutboundMsg};
use crate::im::log::{prefix_channel, truncate_content_default};
use crate::im::transport::{ImChannelCapabilities, ImTransport, SendError};

/// Feishu text message max length (conservative; API may allow more).
pub const FEISHU_MAX_MESSAGE_LEN: usize = 4000;

const FEISHU_API_BASE: &str = "https://open.feishu.cn/open-apis";
const TOKEN_URL: &str = "https://open.feishu.cn/open-apis/auth/v3/tenant_access_token/internal";
/// Refresh token when less than this many seconds remain.
const TOKEN_REFRESH_MARGIN_SECS: u64 = 300;

/// Cached tenant_access_token with expiry.
struct TokenCache {
    token: String,
    expires_at: Instant,
}

/// Feishu transport: app_id + app_secret, tenant_access_token, send via HTTP.
/// Capabilities: no stream-edit (edit message not used); daemon buffers and sends once on StreamEnd.
pub struct FeishuTransport {
    app_id: String,
    app_secret: String,
    client: reqwest::Client,
    token_cache: Arc<tokio::sync::RwLock<Option<TokenCache>>>,
}

impl FeishuTransport {
    pub fn new(app_id: String, app_secret: String) -> Self {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(15))
            .build()
            .expect("reqwest client");
        Self {
            app_id,
            app_secret,
            client,
            token_cache: Arc::new(tokio::sync::RwLock::new(None)),
        }
    }

    async fn get_token(&self) -> Result<String, SendError> {
        {
            let guard = self.token_cache.read().await;
            if let Some(c) = guard.as_ref() {
                if c.expires_at.saturating_duration_since(Instant::now()) > Duration::from_secs(TOKEN_REFRESH_MARGIN_SECS) {
                    return Ok(c.token.clone());
                }
            }
        }
        let body = serde_json::json!({
            "app_id": self.app_id,
            "app_secret": self.app_secret,
        });
        let res = self
            .client
            .post(TOKEN_URL)
            .json(&body)
            .send()
            .await
            .map_err(|e| SendError::Other(e.to_string()))?;
        let _ = res.status();
        let text = res.text().await.map_err(|e| SendError::Other(e.to_string()))?;
        let json: serde_json::Value = serde_json::from_str(&text).unwrap_or(serde_json::Value::Null);
        let code = json.get("code").and_then(|c| c.as_i64()).unwrap_or(-1);
        if code != 0 {
            return Err(SendError::Other(format!("token API code={} body={}", code, text)));
        }
        let token = json
            .get("tenant_access_token")
            .and_then(|t| t.as_str())
            .ok_or_else(|| SendError::Other("token response missing tenant_access_token".into()))?
            .to_string();
        let expire = json.get("expire").and_then(|e| e.as_u64()).unwrap_or(7200);
        let expires_at = Instant::now() + Duration::from_secs(expire);
        {
            let mut guard = self.token_cache.write().await;
            *guard = Some(TokenCache { token: token.clone(), expires_at });
        }
        Ok(token)
    }

    fn parse_chat_id(channel_id: &str) -> Result<&str, SendError> {
        channel_id
            .strip_prefix("feishu:")
            .ok_or_else(|| SendError::Other("invalid channel_id (expected feishu:CHAT_ID)".into()))
    }

    /// Download a message resource (file/image) from Feishu API.
    /// GET /open-apis/im/v1/messages/{message_id}/resources/{file_key}?type={type}
    /// `resource_type` is "file" or "image".
    /// Returns the raw bytes on success.
    pub async fn download_resource(
        &self,
        message_id: &str,
        file_key: &str,
        resource_type: &str,
    ) -> Result<Vec<u8>, SendError> {
        let token = self.get_token().await?;
        let url = format!(
            "{}/im/v1/messages/{}/resources/{}?type={}",
            FEISHU_API_BASE, message_id, file_key, resource_type
        );
        let res = self
            .client
            .get(&url)
            .header("Authorization", format!("Bearer {}", token))
            .send()
            .await
            .map_err(|e| SendError::Other(format!("download_resource request: {}", e)))?;
        let status = res.status();
        if !status.is_success() {
            let body = res.text().await.unwrap_or_default();
            return Err(SendError::Other(format!(
                "download_resource status={} body={}",
                status, body
            )));
        }
        res.bytes()
            .await
            .map(|b| b.to_vec())
            .map_err(|e| SendError::Other(format!("download_resource read bytes: {}", e)))
    }
}

#[async_trait]
impl ImTransport for FeishuTransport {
    fn capabilities(&self) -> ImChannelCapabilities {
        ImChannelCapabilities {
            supports_stream_edit: false,
            max_message_len: FEISHU_MAX_MESSAGE_LEN,
            channel_id_prefix: "feishu",
        }
    }

    async fn send(&self, channel_id: &str, text: &str) -> Result<Option<String>, SendError> {
        let chat_id = Self::parse_chat_id(channel_id)?;
        let text = if text.len() > FEISHU_MAX_MESSAGE_LEN {
            text[..FEISHU_MAX_MESSAGE_LEN].to_string()
        } else {
            text.to_string()
        };
        let token = self.get_token().await?;
        let content_json = serde_json::json!({ "text": text });
        let body = serde_json::json!({
            "receive_id": chat_id,
            "msg_type": "text",
            "content": content_json.to_string(),
        });
        let url = format!("{}/im/v1/messages?receive_id_type=chat_id", FEISHU_API_BASE);
        let res = self
            .client
            .post(&url)
            .header("Authorization", format!("Bearer {}", token))
            .header("Content-Type", "application/json; charset=utf-8")
            .json(&body)
            .send()
            .await
            .map_err(|e| SendError::Other(e.to_string()))?;
        if res.status() == 429 {
            let retry_after = res
                .headers()
                .get("Retry-After")
                .and_then(|v| v.to_str().ok())
                .and_then(|s| s.parse::<f64>().ok())
                .unwrap_or(60.0);
            return Err(SendError::RateLimited { retry_after_secs: retry_after });
        }
        let text_res = res.text().await.map_err(|e| SendError::Other(e.to_string()))?;
        let json: serde_json::Value = serde_json::from_str(&text_res).unwrap_or(serde_json::Value::Null);
        let code = json.get("code").and_then(|c| c.as_i64()).unwrap_or(-1);
        if code != 0 {
            eprintln!("{} chat_id={} direction=send error=code={} body={}", prefix_channel("feishu"), chat_id, code, text_res);
            return Err(SendError::Other(format!("send message API code={} body={}", code, text_res)));
        }
        // Extract message_id from response: { "data": { "message_id": "om_xxx" } }
        let message_id = json.pointer("/data/message_id").and_then(|v| v.as_str()).map(String::from);
        Ok(message_id)
    }

    async fn edit_message(&self, _channel_id: &str, _message_id: &str, _text: &str) -> Result<(), SendError> {
        Ok(())
    }
}

/// State passed to the web server to handle Feishu webhook (url_verification + event_callback).
#[derive(Clone)]
pub struct FeishuWebhookState {
    pub inbound_tx: mpsc::Sender<crate::im::worker::InboundMessage>,
    pub outbound: Arc<OutboundHub<FeishuTransport>>,
    pub busy_set: Arc<DashMap<String, ()>>,
}

/// Handle Feishu webhook body: url_verification returns {"challenge": "<challenge>"}; event_callback parses im.message.receive_v1.
/// Returns (status_code, body_json_string).
/// Supports msg_type: text, file, image. For file/image, passes attachment metadata to the worker (download happens there).
/// Note: If Encrypt Key is enabled in Feishu console, request body is {"encrypt":"..."}; we do not decrypt yet, so events will not be processed.
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

    let ty = root
        .get("type")
        .or_else(|| root.get("Type"))
        .and_then(|t| t.as_str());
    if ty == Some("url_verification") {
        let challenge = root
            .get("challenge")
            .and_then(|c| c.as_str())
            .unwrap_or("");
        return (200, serde_json::json!({ "challenge": challenge }).to_string());
    }

    let event = match root.get("event") {
        Some(e) => e,
        None => return (200, "{}".to_string()),
    };
    let event_type = root
        .get("header")
        .and_then(|h| h.get("event_type"))
        .and_then(|t| t.as_str())
        .or_else(|| event.get("type").and_then(|t| t.as_str()))
        .or_else(|| event.get("event_type").and_then(|t| t.as_str()));
    if event_type != Some("im.message.receive_v1") {
        return (200, "{}".to_string());
    }

    let Some(st) = state else {
        return (200, "{}".to_string());
    };

    let message = match event.get("message") {
        Some(m) => m,
        None => return (200, "{}".to_string()),
    };
    let chat_id = match message.get("chat_id").and_then(|c| c.as_str()) {
        Some(c) => c,
        None => return (200, "{}".to_string()),
    };
    let message_id = message.get("message_id").and_then(|m| m.as_str()).unwrap_or("");
    let parent_id = message.get("parent_id").and_then(|p| p.as_str()).filter(|s| !s.is_empty()).map(String::from);
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

    use crate::im::worker::{FeishuAttachment, InboundMessage};

    // Build InboundMessage based on msg_type
    let inbound = match msg_type {
        "file" => {
            let file_key = content.get("file_key").and_then(|k| k.as_str()).unwrap_or("");
            let file_name = content.get("file_name").and_then(|n| n.as_str()).unwrap_or("unknown");
            if file_key.is_empty() {
                return (200, "{}".to_string());
            }
            eprintln!("{} chat_id={} message_id={} direction=incoming type=file file_name={}", P, chat_id, message_id, file_name);
            InboundMessage {
                channel_id: channel_id.clone(),
                text: format!("I uploaded a file: {}. Please read and analyze it.", file_name),
                attachments: vec![FeishuAttachment {
                    message_id: message_id.to_string(),
                    file_key: file_key.to_string(),
                    file_name: file_name.to_string(),
                    resource_type: "file".to_string(),
                }],
                parent_id: parent_id.clone(),
            }
        }
        "image" => {
            let image_key = content.get("image_key").and_then(|k| k.as_str()).unwrap_or("");
            if image_key.is_empty() {
                return (200, "{}".to_string());
            }
            let image_name = format!("{}.png", &image_key.chars().take(16).collect::<String>());
            eprintln!("{} chat_id={} message_id={} direction=incoming type=image image_key={}", P, chat_id, message_id, image_key);
            InboundMessage {
                channel_id: channel_id.clone(),
                text: "I uploaded an image. Please analyze it.".to_string(),
                attachments: vec![FeishuAttachment {
                    message_id: message_id.to_string(),
                    file_key: image_key.to_string(),
                    file_name: image_name,
                    resource_type: "image".to_string(),
                }],
                parent_id: parent_id.clone(),
            }
        }
        _ => {
            // text or other — original behavior
            let text = content
                .get("text")
                .and_then(|t| t.as_str())
                .unwrap_or("")
                .trim()
                .to_string();
            if text.is_empty() {
                return (200, "{}".to_string());
            }
            InboundMessage {
                channel_id: channel_id.clone(),
                text,
                attachments: vec![],
                parent_id: parent_id.clone(),
            }
        }
    };

    eprintln!("{} chat_id={} message_id={} direction=incoming content={} attachments={}", P, chat_id, message_id, truncate_content_default(&inbound.text), inbound.attachments.len());

    if st.busy_set.contains_key(&channel_id) {
        let _ = st
            .outbound
            .send(&channel_id, OutboundMsg::Send(channel_id.clone(), "Please wait for the current task to finish.".to_string()))
            .await;
        return (200, "{}".to_string());
    }

    if st.inbound_tx.try_send(inbound).is_err() {
        eprintln!("{} chat_id={} message_id={} event=inbound_queue_full dropped=1", P, chat_id, message_id);
    }
    (200, "{}".to_string())
}

/// Setup Feishu: create transport, outbound, worker; return state for webhook route.
/// Returns None if feishu app_id/app_secret not configured.
pub async fn run_feishu_bot() -> Option<FeishuWebhookState> {
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
    let transport = Arc::new(FeishuTransport::new(app_id, app_secret));
    let outbound = OutboundHub::new(transport.clone());

    tokio::spawn(crate::im::worker::run_worker(
        inbound_rx,
        outbound.clone(),
        busy_set.clone(),
        Some(transport),
    ));

    eprintln!("{} event=bot_ready webhook=/api/im/feishu/event", prefix_channel("feishu"));
    Some(FeishuWebhookState {
        inbound_tx,
        outbound,
        busy_set,
    })
}
