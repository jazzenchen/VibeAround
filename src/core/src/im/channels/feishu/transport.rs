//! Feishu transport: HTTP API with tenant_access_token.
//! Send, reply, reactions, interactive cards.

use std::sync::Arc;
use std::time::{Duration, Instant};

use async_trait::async_trait;

use crate::im::log::prefix_channel;
use crate::im::transport::{ImChannelCapabilities, ImTransport, InteractiveOption, SendError};

pub const FEISHU_MAX_MESSAGE_LEN: usize = 4000;

pub(crate) const FEISHU_API_BASE: &str = "https://open.feishu.cn/open-apis";
const TOKEN_URL: &str = "https://open.feishu.cn/open-apis/auth/v3/tenant_access_token/internal";
const TOKEN_REFRESH_MARGIN_SECS: u64 = 300;

struct TokenCache {
    token: String,
    expires_at: Instant,
}

/// Feishu transport: app_id + app_secret, tenant_access_token, send via HTTP.
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

    /// Get a single message by id (e.g. to fetch quoted message content/attachments).
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

#[async_trait]
impl ImTransport for FeishuTransport {
    fn capabilities(&self) -> ImChannelCapabilities {
        ImChannelCapabilities {
            supports_stream_edit: false,
            buffer_stream: true,
            max_message_len: FEISHU_MAX_MESSAGE_LEN,
            channel_id_prefix: "feishu",
            processing_reaction: "OneSecond",
            done_reaction: "CheckMark",
        }
    }

    async fn send(&self, channel_id: &str, text: &str) -> Result<Option<String>, SendError> {
        let chat_id = Self::parse_chat_id(channel_id)?;
        let text = if text.len() > FEISHU_MAX_MESSAGE_LEN {
            text[..FEISHU_MAX_MESSAGE_LEN].to_string()
        } else { text.to_string() };
        let token = self.get_token().await?;
        let content_json = serde_json::json!({ "text": text });
        let body = serde_json::json!({
            "receive_id": chat_id,
            "msg_type": "text",
            "content": content_json.to_string(),
        });
        let url = format!("{}/im/v1/messages?receive_id_type=chat_id", FEISHU_API_BASE);
        let res = self.client.post(&url)
            .header("Authorization", format!("Bearer {}", token))
            .header("Content-Type", "application/json; charset=utf-8")
            .json(&body).send().await
            .map_err(|e| SendError::Other(e.to_string()))?;
        if res.status() == 429 {
            let retry_after = res.headers().get("Retry-After")
                .and_then(|v| v.to_str().ok()).and_then(|s| s.parse::<f64>().ok()).unwrap_or(60.0);
            return Err(SendError::RateLimited { retry_after_secs: retry_after });
        }
        let text_res = res.text().await.map_err(|e| SendError::Other(e.to_string()))?;
        let json: serde_json::Value = serde_json::from_str(&text_res).unwrap_or(serde_json::Value::Null);
        let code = json.get("code").and_then(|c| c.as_i64()).unwrap_or(-1);
        if code != 0 {
            eprintln!("{} chat_id={} direction=send error=code={} body={}",
                prefix_channel("feishu"), chat_id, code, text_res);
            return Err(SendError::Other(format!("send message API code={} body={}", code, text_res)));
        }
        let message_id = json.pointer("/data/message_id").and_then(|v| v.as_str()).map(String::from);
        Ok(message_id)
    }

    async fn edit_message(&self, _channel_id: &str, _message_id: &str, _text: &str) -> Result<(), SendError> {
        Ok(())
    }

    async fn reply(&self, channel_id: &str, reply_to_message_id: &str, text: &str) -> Result<Option<String>, SendError> {
        let _chat_id = Self::parse_chat_id(channel_id)?;
        let text = if text.len() > FEISHU_MAX_MESSAGE_LEN {
            text[..FEISHU_MAX_MESSAGE_LEN].to_string()
        } else { text.to_string() };
        let token = self.get_token().await?;
        let content_json = serde_json::json!({ "text": text });
        let body = serde_json::json!({
            "msg_type": "text",
            "content": content_json.to_string(),
        });
        let url = format!("{}/im/v1/messages/{}/reply", FEISHU_API_BASE, reply_to_message_id);
        let res = self.client.post(&url)
            .header("Authorization", format!("Bearer {}", token))
            .header("Content-Type", "application/json; charset=utf-8")
            .json(&body).send().await
            .map_err(|e| SendError::Other(e.to_string()))?;
        if res.status() == 429 {
            let retry_after = res.headers().get("Retry-After")
                .and_then(|v| v.to_str().ok()).and_then(|s| s.parse::<f64>().ok()).unwrap_or(60.0);
            return Err(SendError::RateLimited { retry_after_secs: retry_after });
        }
        let text_res = res.text().await.map_err(|e| SendError::Other(e.to_string()))?;
        let json: serde_json::Value = serde_json::from_str(&text_res).unwrap_or(serde_json::Value::Null);
        let code = json.get("code").and_then(|c| c.as_i64()).unwrap_or(-1);
        if code != 0 {
            return Err(SendError::Other(format!("reply API code={} body={}", code, text_res)));
        }
        let message_id = json.pointer("/data/message_id").and_then(|v| v.as_str()).map(String::from);
        Ok(message_id)
    }

    async fn add_reaction(&self, channel_id: &str, message_id: &str, emoji: &str) -> Result<Option<String>, SendError> {
        let _chat_id = Self::parse_chat_id(channel_id)?;
        let token = self.get_token().await?;
        // Feishu uses its own emoji_type identifiers — pass through directly (e.g. "OneSecond", "CheckMark").
        let emoji_type = emoji;
        let body = serde_json::json!({ "reaction_type": { "emoji_type": emoji_type } });
        let url = format!("{}/im/v1/messages/{}/reactions", FEISHU_API_BASE, message_id);
        eprintln!("{} direction=add_reaction message_id={} emoji_type={} url={}",
            prefix_channel("feishu"), message_id, emoji_type, url);
        let res = self.client.post(&url)
            .header("Authorization", format!("Bearer {}", token))
            .header("Content-Type", "application/json; charset=utf-8")
            .json(&body).send().await
            .map_err(|e| SendError::Other(e.to_string()))?;
        let status = res.status();
        let text_res = res.text().await.map_err(|e| SendError::Other(e.to_string()))?;
        let json: serde_json::Value = serde_json::from_str(&text_res).unwrap_or(serde_json::Value::Null);
        let code = json.get("code").and_then(|c| c.as_i64()).unwrap_or(-1);
        if code != 0 {
            eprintln!("{} direction=add_reaction error: status={} code={} body={}",
                prefix_channel("feishu"), status, code, text_res);
            return Err(SendError::Other(format!("add_reaction API code={} body={}", code, text_res)));
        }
        let reaction_id = json.pointer("/data/reaction_id").and_then(|v| v.as_str()).map(String::from);
        eprintln!("{} direction=add_reaction success reaction_id={:?}",
            prefix_channel("feishu"), reaction_id);
        Ok(reaction_id)
    }

    async fn remove_reaction(&self, channel_id: &str, message_id: &str, reaction_id: &str) -> Result<(), SendError> {
        let _chat_id = Self::parse_chat_id(channel_id)?;
        let token = self.get_token().await?;
        let url = format!("{}/im/v1/messages/{}/reactions/{}", FEISHU_API_BASE, message_id, reaction_id);
        let _ = self.client.delete(&url)
            .header("Authorization", format!("Bearer {}", token))
            .send().await
            .map_err(|e| SendError::Other(e.to_string()))?;
        Ok(())
    }

    async fn send_interactive(
        &self, channel_id: &str, prompt: &str, options: &[InteractiveOption], reply_to: Option<&str>,
    ) -> Result<Option<String>, SendError> {
        let chat_id = Self::parse_chat_id(channel_id)?;
        let token = self.get_token().await?;

        let content = super::interaction::build_card("VibeAround", prompt, options);
        eprintln!("{} direction=send_interactive card_content={}", prefix_channel("feishu"), &content);

        let (url, body) = if let Some(reply_mid) = reply_to {
            let url = format!("{}/im/v1/messages/{}/reply", FEISHU_API_BASE, reply_mid);
            let b = serde_json::json!({ "msg_type": "interactive", "content": content });
            (url, b)
        } else {
            let url = format!("{}/im/v1/messages?receive_id_type=chat_id", FEISHU_API_BASE);
            let b = serde_json::json!({ "receive_id": chat_id, "msg_type": "interactive", "content": content });
            (url, b)
        };

        let res = self.client.post(&url)
            .header("Authorization", format!("Bearer {}", token))
            .header("Content-Type", "application/json; charset=utf-8")
            .json(&body).send().await
            .map_err(|e| SendError::Other(e.to_string()))?;
        let text_res = res.text().await.map_err(|e| SendError::Other(e.to_string()))?;
        eprintln!("{} direction=send_interactive response={}", prefix_channel("feishu"), text_res);
        let json: serde_json::Value = serde_json::from_str(&text_res).unwrap_or(serde_json::Value::Null);
        let code = json.get("code").and_then(|c| c.as_i64()).unwrap_or(0);
        if code != 0 {
            eprintln!("{} direction=send_interactive error=code={} body={}",
                prefix_channel("feishu"), code, text_res);
        }
        let message_id = json.pointer("/data/message_id").and_then(|v| v.as_str()).map(String::from);
        Ok(message_id)
    }
}
