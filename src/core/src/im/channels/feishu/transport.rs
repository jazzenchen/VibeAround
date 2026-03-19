//! Feishu transport: HTTP API with tenant_access_token.
//! Uses interactive card messages with PATCH updates for streaming-like edits.
//! Each content block (thinking, text, tool_use, etc.) is a separate card message.

use std::sync::Arc;
use std::time::{Duration, Instant};

use async_trait::async_trait;

use crate::im::log::prefix_channel;
use crate::im::transport::{ImChannelCapabilities, ImTransport, InteractiveOption, SendError, SendResult};

pub const FEISHU_MAX_MESSAGE_LEN: usize = 4000;

pub(crate) const FEISHU_API_BASE: &str = "https://open.feishu.cn/open-apis";
const TOKEN_URL: &str = "https://open.feishu.cn/open-apis/auth/v3/tenant_access_token/internal";
const TOKEN_REFRESH_MARGIN_SECS: u64 = 300;

struct TokenCache {
    token: String,
    expires_at: Instant,
}

/// Feishu transport: card messages with PATCH edit for streaming-like updates.
pub struct FeishuTransport {
    app_id: String,
    app_secret: String,
    pub(crate) client: reqwest::Client,
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

    pub async fn get_token(&self) -> Result<String, SendError> {
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
        let res = self.client.post(TOKEN_URL).json(&body).send().await
            .map_err(|e| SendError::Other(e.to_string()))?;
        let text = res.text().await.map_err(|e| SendError::Other(e.to_string()))?;
        let json: serde_json::Value = serde_json::from_str(&text).unwrap_or(serde_json::Value::Null);
        let code = json.get("code").and_then(|c| c.as_i64()).unwrap_or(-1);
        if code != 0 {
            return Err(SendError::Other(format!("token API code={} body={}", code, text)));
        }
        let token = json.get("tenant_access_token").and_then(|t| t.as_str())
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

    pub(crate) fn parse_chat_id(channel_id: &str) -> Result<&str, SendError> {
        channel_id.strip_prefix("feishu:")
            .ok_or_else(|| SendError::Other("invalid channel_id (expected feishu:CHAT_ID)".into()))
    }

    /// Send a card message to a chat. Returns the message_id.
    async fn send_card_message(&self, chat_id: &str, text: &str) -> Result<Option<String>, SendError> {
        let token = self.get_token().await?;
        let card = build_markdown_card(text);
        let body = serde_json::json!({
            "receive_id": chat_id,
            "msg_type": "interactive",
            "content": card,
        });
        let url = format!("{}/im/v1/messages?receive_id_type=chat_id", FEISHU_API_BASE);
        let res = self.client.post(&url)
            .header("Authorization", format!("Bearer {}", token))
            .json(&body)
            .send().await
            .map_err(|e| SendError::Other(format!("send_card request: {}", e)))?;
        if res.status() == 429 {
            let ra = res.headers().get("Retry-After")
                .and_then(|v| v.to_str().ok()).and_then(|s| s.parse::<f64>().ok()).unwrap_or(60.0);
            return Err(SendError::RateLimited { retry_after_secs: ra });
        }
        let text = res.text().await.map_err(|e| SendError::Other(e.to_string()))?;
        let json: serde_json::Value = serde_json::from_str(&text).unwrap_or(serde_json::Value::Null);
        let code = json.get("code").and_then(|c| c.as_i64()).unwrap_or(-1);
        if code != 0 {
            return Err(SendError::Other(format!("send_card code={} body={}", code, text)));
        }
        let message_id = json.pointer("/data/message_id")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        Ok(message_id)
    }

    /// PATCH update an existing card message's content.
    async fn patch_card(&self, message_id: &str, text: &str) -> Result<(), SendError> {
        let token = self.get_token().await?;
        let card = build_markdown_card(text);
        let body = serde_json::json!({
            "content": card,
        });
        let url = format!("{}/im/v1/messages/{}", FEISHU_API_BASE, message_id);
        let res = self.client.patch(&url)
            .header("Authorization", format!("Bearer {}", token))
            .json(&body)
            .send().await
            .map_err(|e| SendError::Other(format!("patch_card request: {}", e)))?;
        let text = res.text().await.map_err(|e| SendError::Other(e.to_string()))?;
        let json: serde_json::Value = serde_json::from_str(&text).unwrap_or(serde_json::Value::Null);
        let code = json.get("code").and_then(|c| c.as_i64()).unwrap_or(-1);
        if code != 0 {
            eprintln!("{} patch_card id={} error=code={} body={}",
                prefix_channel("feishu"), message_id, code, text);
        }
        Ok(())
    }

    /// Get a single message by id.
    pub async fn get_message(
        &self, chat_id: &str, message_id: &str,
    ) -> Result<(String, serde_json::Value), SendError> {
        let token = self.get_token().await?;
        let url = format!("{}/im/v1/messages/{}?receive_id_type=chat_id&receive_id={}",
            FEISHU_API_BASE, message_id, urlencoding::encode(chat_id));
        let res = self.client.get(&url)
            .header("Authorization", format!("Bearer {}", token))
            .send().await
            .map_err(|e| SendError::Other(format!("get_message request: {}", e)))?;
        let status = res.status();
        if !status.is_success() {
            let body = res.text().await.unwrap_or_default();
            return Err(SendError::Other(format!("get_message status={} body={}", status, body)));
        }
        let text = res.text().await.map_err(|e| SendError::Other(e.to_string()))?;
        let json: serde_json::Value = serde_json::from_str(&text)
            .map_err(|e| SendError::Other(format!("get_message parse: {}", e)))?;
        let data = json.get("data").ok_or_else(|| SendError::Other("get_message: no data".into()))?;
        let (msg_type, content) = if let Some(items) = data.get("items").and_then(|i| i.as_array()) {
            let msg = items.first().ok_or_else(|| SendError::Other("get_message: empty items".into()))?;
            let t = msg.get("message_type").or_else(|| msg.get("msg_type"))
                .and_then(|t| t.as_str()).unwrap_or("text").to_string();
            let body = msg.get("body").ok_or_else(|| SendError::Other("get_message: no body".into()))?;
            let content_str = body.get("content").and_then(|c| c.as_str()).unwrap_or("{}");
            (t, serde_json::from_str(content_str).unwrap_or(serde_json::Value::Null))
        } else if data.get("body").is_some() {
            let t = data.get("message_type").or_else(|| data.get("msg_type"))
                .and_then(|t| t.as_str()).unwrap_or("text").to_string();
            let content_str = data.get("body").and_then(|b| b.get("content"))
                .and_then(|c| c.as_str()).unwrap_or("{}");
            (t, serde_json::from_str(content_str).unwrap_or(serde_json::Value::Null))
        } else {
            return Err(SendError::Other("get_message: data has no items nor body".into()));
        };
        Ok((msg_type, content))
    }

    /// Download a message resource (file/image) from Feishu API.
    pub async fn download_resource(
        &self, message_id: &str, file_key: &str, resource_type: &str,
    ) -> Result<Vec<u8>, SendError> {
        let token = self.get_token().await?;
        let url = format!("{}/im/v1/messages/{}/resources/{}?type={}",
            FEISHU_API_BASE, message_id, file_key, resource_type);
        let res = self.client.get(&url)
            .header("Authorization", format!("Bearer {}", token))
            .send().await
            .map_err(|e| SendError::Other(format!("download_resource request: {}", e)))?;
        let status = res.status();
        if !status.is_success() {
            let body = res.text().await.unwrap_or_default();
            return Err(SendError::Other(format!("download_resource status={} body={}", status, body)));
        }
        res.bytes().await.map(|b| b.to_vec())
            .map_err(|e| SendError::Other(format!("download_resource read bytes: {}", e)))
    }
}

/// Build a simple interactive card with a single markdown element.
/// The content string is used as the card body.
fn build_markdown_card(content: &str) -> String {
    serde_json::json!({
        "config": { "wide_screen_mode": true },
        "elements": [
            {
                "tag": "div",
                "text": {
                    "tag": "lark_md",
                    "content": content
                }
            }
        ]
    }).to_string()
}

#[async_trait]
impl ImTransport for FeishuTransport {
    fn capabilities(&self) -> ImChannelCapabilities {
        ImChannelCapabilities {
            supports_stream_edit: true,
            max_message_len: FEISHU_MAX_MESSAGE_LEN,
            channel_id_prefix: "feishu",
            processing_reaction: "OnIt",
            min_edit_interval: Duration::from_millis(500),
        }
    }

    async fn send(&self, channel_id: &str, text: &str) -> Result<SendResult, SendError> {
        let chat_id = Self::parse_chat_id(channel_id)?;
        self.send_card_message(chat_id, text).await
    }

    async fn edit_message(&self, _channel_id: &str, message_id: &str, text: &str) -> Result<(), SendError> {
        self.patch_card(message_id, text).await
    }

    async fn finalize_stream(&self, _channel_id: &str, _message_id: &str, _final_text: &str) -> Result<(), SendError> {
        // Plain text messages don't need finalization
        Ok(())
    }

    async fn reply(&self, channel_id: &str, reply_to_message_id: &str, text: &str) -> Result<SendResult, SendError> {
        let chat_id = Self::parse_chat_id(channel_id)?;
        let token = self.get_token().await?;
        let content = serde_json::json!({ "text": text });
        let body = serde_json::json!({
            "receive_id": chat_id,
            "msg_type": "text",
            "content": content.to_string(),
        });
        let url = format!("{}/im/v1/messages/{}/reply", FEISHU_API_BASE, reply_to_message_id);
        let res = self.client.post(&url)
            .header("Authorization", format!("Bearer {}", token))
            .header("Content-Type", "application/json; charset=utf-8")
            .json(&body).send().await
            .map_err(|e| SendError::Other(e.to_string()))?;
        let text = res.text().await.map_err(|e| SendError::Other(e.to_string()))?;
        let json: serde_json::Value = serde_json::from_str(&text).unwrap_or(serde_json::Value::Null);
        let code = json.get("code").and_then(|c| c.as_i64()).unwrap_or(-1);
        if code != 0 {
            return Err(SendError::Other(format!("reply API code={} body={}", code, text)));
        }
        Ok(json.pointer("/data/message_id").and_then(|v| v.as_str()).map(String::from))
    }

    async fn add_reaction(&self, channel_id: &str, message_id: &str, emoji: &str) -> Result<Option<String>, SendError> {
        let _ = Self::parse_chat_id(channel_id)?;
        let token = self.get_token().await?;
        let body = serde_json::json!({ "reaction_type": { "emoji_type": emoji } });
        let url = format!("{}/im/v1/messages/{}/reactions", FEISHU_API_BASE, message_id);
        let res = self.client.post(&url)
            .header("Authorization", format!("Bearer {}", token))
            .header("Content-Type", "application/json; charset=utf-8")
            .json(&body).send().await
            .map_err(|e| SendError::Other(e.to_string()))?;
        let text = res.text().await.map_err(|e| SendError::Other(e.to_string()))?;
        let json: serde_json::Value = serde_json::from_str(&text).unwrap_or(serde_json::Value::Null);
        let code = json.get("code").and_then(|c| c.as_i64()).unwrap_or(-1);
        if code != 0 {
            return Err(SendError::Other(format!("add_reaction code={} body={}", code, text)));
        }
        let reaction_id = json.pointer("/data/reaction_id").and_then(|v| v.as_str()).map(String::from);
        Ok(reaction_id)
    }

    async fn remove_reaction(&self, _channel_id: &str, message_id: &str, reaction_id: &str) -> Result<(), SendError> {
        let token = self.get_token().await?;
        let url = format!("{}/im/v1/messages/{}/reactions/{}", FEISHU_API_BASE, message_id, reaction_id);
        let res = self.client.delete(&url)
            .header("Authorization", format!("Bearer {}", token))
            .send().await
            .map_err(|e| SendError::Other(e.to_string()))?;
        let text = res.text().await.map_err(|e| SendError::Other(e.to_string()))?;
        let json: serde_json::Value = serde_json::from_str(&text).unwrap_or(serde_json::Value::Null);
        let code = json.get("code").and_then(|c| c.as_i64()).unwrap_or(-1);
        if code != 0 {
            eprintln!("{} direction=remove_reaction error=code={} body={}",
                prefix_channel("feishu"), code, text);
        }
        Ok(())
    }

    async fn send_interactive(
        &self,
        channel_id: &str,
        prompt: &str,
        options: &[InteractiveOption],
        reply_to: Option<&str>,
    ) -> Result<SendResult, SendError> {
        let chat_id = Self::parse_chat_id(channel_id)?;
        let token = self.get_token().await?;
        let card_json = super::interaction::build_card("VibeAround", prompt, options);
        // Send as interactive card directly (not CardKit streaming)
        let body = serde_json::json!({
            "receive_id": chat_id,
            "msg_type": "interactive",
            "content": card_json,
        });
        let url = if let Some(reply_id) = reply_to {
            format!("{}/im/v1/messages/{}/reply", FEISHU_API_BASE, reply_id)
        } else {
            format!("{}/im/v1/messages?receive_id_type=chat_id", FEISHU_API_BASE)
        };
        let res = self.client.post(&url)
            .header("Authorization", format!("Bearer {}", token))
            .header("Content-Type", "application/json; charset=utf-8")
            .json(&body).send().await
            .map_err(|e| SendError::Other(e.to_string()))?;
        let text = res.text().await.map_err(|e| SendError::Other(e.to_string()))?;
        let json: serde_json::Value = serde_json::from_str(&text).unwrap_or(serde_json::Value::Null);
        let code = json.get("code").and_then(|c| c.as_i64()).unwrap_or(-1);
        if code != 0 {
            eprintln!("{} chat_id={} direction=send_interactive error=code={} body={}",
                prefix_channel("feishu"), chat_id, code, text);
            return Err(SendError::Other(format!("send_interactive code={} body={}", code, text)));
        }
        Ok(json.pointer("/data/message_id").and_then(|v| v.as_str()).map(String::from))
    }

    async fn update_interactive(
        &self,
        channel_id: &str,
        message_id: &str,
        prompt: &str,
        options: &[InteractiveOption],
    ) -> Result<(), SendError> {
        let _chat_id = Self::parse_chat_id(channel_id)?;
        let token = self.get_token().await?;
        let card_json = super::interaction::build_card("VibeAround", prompt, options);
        let body = serde_json::json!({
            "msg_type": "interactive",
            "content": card_json,
        });
        let url = format!("{}/im/v1/messages/{}", FEISHU_API_BASE, message_id);
        let res = self.client.patch(&url)
            .header("Authorization", format!("Bearer {}", token))
            .header("Content-Type", "application/json; charset=utf-8")
            .json(&body).send().await
            .map_err(|e| SendError::Other(e.to_string()))?;
        let text = res.text().await.map_err(|e| SendError::Other(e.to_string()))?;
        let json: serde_json::Value = serde_json::from_str(&text).unwrap_or(serde_json::Value::Null);
        let code = json.get("code").and_then(|c| c.as_i64()).unwrap_or(-1);
        if code != 0 {
            eprintln!("{} chat_id={} direction=update_interactive message_id={} error=code={} body={}",
                prefix_channel("feishu"), _chat_id, message_id, code, text);
            return Err(SendError::Other(format!("update_interactive code={} body={}", code, text)));
        }
        Ok(())
    }
}
