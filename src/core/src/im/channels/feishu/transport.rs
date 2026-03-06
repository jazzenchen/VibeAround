//! Feishu transport: HTTP API with tenant_access_token.
//! Uses CardKit Streaming API for typewriter-effect updates.

use std::sync::Arc;
use std::time::{Duration, Instant};

use async_trait::async_trait;

use crate::im::log::prefix_channel;
use crate::im::transport::{ImChannelCapabilities, ImTransport, InteractiveOption, SendError, SendResult};

pub const FEISHU_MAX_MESSAGE_LEN: usize = 4000;

pub(crate) const FEISHU_API_BASE: &str = "https://open.feishu.cn/open-apis";
const TOKEN_URL: &str = "https://open.feishu.cn/open-apis/auth/v3/tenant_access_token/internal";
const TOKEN_REFRESH_MARGIN_SECS: u64 = 300;

/// The element_id used for the main markdown body in streaming cards.
const STREAM_ELEMENT_ID: &str = "stream_md";

struct TokenCache {
    token: String,
    expires_at: Instant,
}

/// Feishu transport: always uses CardKit Streaming API for card messages.
pub struct FeishuTransport {
    app_id: String,
    app_secret: String,
    pub(crate) client: reqwest::Client,
    token_cache: Arc<tokio::sync::RwLock<Option<TokenCache>>>,
    /// Monotonically increasing sequence number for CardKit streaming API calls.
    sequence: std::sync::atomic::AtomicU64,
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
            sequence: std::sync::atomic::AtomicU64::new(1),
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

    fn next_sequence(&self) -> u64 {
        self.sequence.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
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

    // ── CardKit Streaming API ──────────────────────────────────────────

    /// Create a card entity with streaming_mode enabled. Returns card_id.
    async fn create_card_entity(&self, card_json: &str) -> Result<String, SendError> {
        let token = self.get_token().await?;
        let body = serde_json::json!({
            "type": "card_json",
            "data": card_json,
        });
        let url = format!("{}/cardkit/v1/cards", FEISHU_API_BASE);
        let res = self.client.post(&url)
            .header("Authorization", format!("Bearer {}", token))
            .header("Content-Type", "application/json; charset=utf-8")
            .json(&body).send().await
            .map_err(|e| SendError::Other(format!("create_card: {}", e)))?;
        let text = res.text().await.map_err(|e| SendError::Other(e.to_string()))?;
        let json: serde_json::Value = serde_json::from_str(&text).unwrap_or(serde_json::Value::Null);
        let code = json.get("code").and_then(|c| c.as_i64()).unwrap_or(-1);
        if code != 0 {
            eprintln!("{} direction=create_card error=code={} body={}",
                prefix_channel("feishu"), code, text);
            return Err(SendError::Other(format!("create_card API code={} body={}", code, text)));
        }
        json.pointer("/data/card_id")
            .and_then(|v| v.as_str())
            .map(String::from)
            .ok_or_else(|| SendError::Other("create_card: missing card_id".into()))
    }

    /// Send a card entity as a message.
    async fn send_card_entity(&self, chat_id: &str, card_id: &str) -> Result<Option<String>, SendError> {
        let token = self.get_token().await?;
        let content = serde_json::json!({
            "type": "card",
            "data": { "card_id": card_id }
        });
        let body = serde_json::json!({
            "receive_id": chat_id,
            "msg_type": "interactive",
            "content": content.to_string(),
        });
        let url = format!("{}/im/v1/messages?receive_id_type=chat_id", FEISHU_API_BASE);
        let res = self.client.post(&url)
            .header("Authorization", format!("Bearer {}", token))
            .header("Content-Type", "application/json; charset=utf-8")
            .json(&body).send().await
            .map_err(|e| SendError::Other(e.to_string()))?;
        if res.status() == 429 {
            let ra = res.headers().get("Retry-After")
                .and_then(|v| v.to_str().ok()).and_then(|s| s.parse::<f64>().ok()).unwrap_or(60.0);
            return Err(SendError::RateLimited { retry_after_secs: ra });
        }
        let text = res.text().await.map_err(|e| SendError::Other(e.to_string()))?;
        let json: serde_json::Value = serde_json::from_str(&text).unwrap_or(serde_json::Value::Null);
        let code = json.get("code").and_then(|c| c.as_i64()).unwrap_or(-1);
        if code != 0 {
            eprintln!("{} chat_id={} direction=send_card error=code={} body={}",
                prefix_channel("feishu"), chat_id, code, text);
            return Err(SendError::Other(format!("send_card API code={} body={}", code, text)));
        }
        Ok(json.pointer("/data/message_id").and_then(|v| v.as_str()).map(String::from))
    }

    /// Push full text to a streaming card element (typewriter effect).
    async fn streaming_update_text(&self, card_id: &str, element_id: &str, content: &str) -> Result<(), SendError> {
        let token = self.get_token().await?;
        let seq = self.next_sequence();
        let body = serde_json::json!({ "content": content, "sequence": seq });
        let url = format!("{}/cardkit/v1/cards/{}/elements/{}/content",
            FEISHU_API_BASE, card_id, element_id);
        let res = self.client.put(&url)
            .header("Authorization", format!("Bearer {}", token))
            .header("Content-Type", "application/json; charset=utf-8")
            .json(&body).send().await
            .map_err(|e| SendError::Other(format!("streaming_update_text: {}", e)))?;
        let text = res.text().await.map_err(|e| SendError::Other(e.to_string()))?;
        let json: serde_json::Value = serde_json::from_str(&text).unwrap_or(serde_json::Value::Null);
        let code = json.get("code").and_then(|c| c.as_i64()).unwrap_or(-1);
        if code != 0 {
            eprintln!("{} card_id={} element_id={} direction=streaming_update error=code={} body={}",
                prefix_channel("feishu"), card_id, element_id, code, text);
            return Err(SendError::Other(format!("streaming_update_text code={} body={}", code, text)));
        }
        Ok(())
    }

    /// Update card settings (e.g. disable streaming_mode when done).
    async fn update_card_settings(&self, card_id: &str, streaming_mode: bool) -> Result<(), SendError> {
        let token = self.get_token().await?;
        let seq = self.next_sequence();
        let settings = serde_json::json!({ "config": { "streaming_mode": streaming_mode } });
        let body = serde_json::json!({ "settings": settings.to_string(), "sequence": seq });
        let url = format!("{}/cardkit/v1/cards/{}/settings", FEISHU_API_BASE, card_id);
        let res = self.client.put(&url)
            .header("Authorization", format!("Bearer {}", token))
            .header("Content-Type", "application/json; charset=utf-8")
            .json(&body).send().await
            .map_err(|e| SendError::Other(format!("update_card_settings: {}", e)))?;
        let text = res.text().await.map_err(|e| SendError::Other(e.to_string()))?;
        let json: serde_json::Value = serde_json::from_str(&text).unwrap_or(serde_json::Value::Null);
        let code = json.get("code").and_then(|c| c.as_i64()).unwrap_or(-1);
        if code != 0 {
            eprintln!("{} card_id={} direction=update_settings streaming={} error=code={} body={}",
                prefix_channel("feishu"), card_id, streaming_mode, code, text);
        }
        Ok(())
    }
}

/// Build a minimal streaming card JSON (no header, just a markdown body element).
fn build_streaming_card() -> String {
    let card = serde_json::json!({
        "schema": "2.0",
        "config": {
            "update_multi": true,
            "streaming_mode": true,
            "summary": { "content": "[Generating]" }
        },
        "streaming_config": {
            "print_strategy": "fast"
        },
        "body": {
            "elements": [
                {
                    "tag": "markdown",
                    "content": "...",
                    "element_id": STREAM_ELEMENT_ID
                }
            ]
        }
    });
    card.to_string()
}

#[async_trait]
impl ImTransport for FeishuTransport {
    fn capabilities(&self) -> ImChannelCapabilities {
        ImChannelCapabilities {
            supports_stream_edit: true,
            max_message_len: FEISHU_MAX_MESSAGE_LEN,
            channel_id_prefix: "feishu",
            processing_reaction: "OnIt",
            min_edit_interval: Duration::ZERO,
        }
    }

    async fn send(&self, channel_id: &str, text: &str) -> Result<SendResult, SendError> {
        let chat_id = Self::parse_chat_id(channel_id)?;
        // Create streaming card → send card entity → return card_id
        let card_json = build_streaming_card();
        let card_id = self.create_card_entity(&card_json).await?;
        self.send_card_entity(chat_id, &card_id).await?;
        // Push initial text
        let _ = self.streaming_update_text(&card_id, STREAM_ELEMENT_ID, text).await;
        Ok(Some(card_id))
    }

    async fn edit_message(&self, _channel_id: &str, card_id: &str, text: &str) -> Result<(), SendError> {
        // Push full text to the streaming card element
        self.streaming_update_text(card_id, STREAM_ELEMENT_ID, text).await
    }

    async fn finalize_stream(&self, _channel_id: &str, card_id: &str, _final_text: &str) -> Result<(), SendError> {
        // End streaming mode on the card (stops typewriter animation)
        self.update_card_settings(card_id, false).await
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
