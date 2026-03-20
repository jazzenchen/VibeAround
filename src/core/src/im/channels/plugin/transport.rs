//! Plugin transport: implements ImTransport by forwarding calls over stdio JSON-RPC
//! to an external plugin process (e.g. Node.js Feishu plugin).

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use async_trait::async_trait;
use dashmap::DashMap;
use tokio::io::AsyncWriteExt;
use tokio::sync::{oneshot, Mutex};

use crate::im::transport::{
    ImChannelCapabilities, ImTransport, InteractiveOption, SendError, SendResult,
};

/// Max message length for plugin channels (Feishu card limit).
const PLUGIN_MAX_MESSAGE_LEN: usize = 4000;

/// Shared writer to plugin stdin.
pub(crate) type StdinWriter = Arc<Mutex<tokio::process::ChildStdin>>;

/// Pending JSON-RPC request map: id → oneshot sender for the response.
pub(crate) type PendingRequests = Arc<DashMap<u64, oneshot::Sender<Result<serde_json::Value, String>>>>;

/// Transport that delegates all IM operations to a plugin process via stdin JSON-RPC.
pub struct PluginTransport {
    stdin: StdinWriter,
    pending: PendingRequests,
    next_id: AtomicU64,
}

impl PluginTransport {
    pub fn new(stdin: StdinWriter, pending: PendingRequests) -> Self {
        Self {
            stdin,
            pending,
            next_id: AtomicU64::new(100), // start at 100 to avoid collision with init id=1
        }
    }

    /// Send a JSON-RPC request and wait for the response.
    async fn rpc_call(
        &self,
        method: &str,
        params: serde_json::Value,
    ) -> Result<serde_json::Value, SendError> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let req = serde_json::json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params,
        });

        let (tx, rx) = oneshot::channel();
        self.pending.insert(id, tx);

        // Write to stdin
        let line = format!("{}\n", req);
        {
            let mut writer = self.stdin.lock().await;
            if let Err(e) = writer.write_all(line.as_bytes()).await {
                self.pending.remove(&id);
                return Err(SendError::Other(format!("stdin write failed: {}", e)));
            }
            if let Err(e) = writer.flush().await {
                self.pending.remove(&id);
                return Err(SendError::Other(format!("stdin flush failed: {}", e)));
            }
        }

        // Wait for response (with timeout)
        match tokio::time::timeout(std::time::Duration::from_secs(30), rx).await {
            Ok(Ok(Ok(result))) => Ok(result),
            Ok(Ok(Err(err_msg))) => Err(SendError::Other(err_msg)),
            Ok(Err(_)) => Err(SendError::Other("response channel dropped".into())),
            Err(_) => {
                self.pending.remove(&id);
                Err(SendError::Other("plugin RPC timeout (30s)".into()))
            }
        }
    }
}

#[async_trait]
impl ImTransport for PluginTransport {
    fn capabilities(&self) -> ImChannelCapabilities {
        ImChannelCapabilities {
            supports_stream_edit: true,
            max_message_len: PLUGIN_MAX_MESSAGE_LEN,
            channel_id_prefix: "feishu",
            processing_reaction: "OnIt",
            min_edit_interval: std::time::Duration::from_millis(800),
        }
    }

    async fn send(&self, channel_id: &str, text: &str) -> Result<SendResult, SendError> {
        let result = self
            .rpc_call(
                "send_text",
                serde_json::json!({ "channelId": channel_id, "text": text }),
            )
            .await?;
        Ok(result
            .get("messageId")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()))
    }

    async fn edit_message(
        &self,
        channel_id: &str,
        message_id: &str,
        text: &str,
    ) -> Result<(), SendError> {
        self.rpc_call(
            "edit_message",
            serde_json::json!({ "channelId": channel_id, "messageId": message_id, "text": text }),
        )
        .await?;
        Ok(())
    }

    async fn reply(
        &self,
        channel_id: &str,
        reply_to_message_id: &str,
        text: &str,
    ) -> Result<SendResult, SendError> {
        let result = self
            .rpc_call(
                "send_text",
                serde_json::json!({ "channelId": channel_id, "text": text, "replyTo": reply_to_message_id }),
            )
            .await?;
        Ok(result
            .get("messageId")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()))
    }

    async fn add_reaction(
        &self,
        _channel_id: &str,
        message_id: &str,
        emoji: &str,
    ) -> Result<Option<String>, SendError> {
        let result = self
            .rpc_call(
                "add_reaction",
                serde_json::json!({ "channelId": _channel_id, "messageId": message_id, "emoji": emoji }),
            )
            .await?;
        Ok(result
            .get("reactionId")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()))
    }

    async fn remove_reaction(
        &self,
        _channel_id: &str,
        message_id: &str,
        reaction_id: &str,
    ) -> Result<(), SendError> {
        self.rpc_call(
            "remove_reaction",
            serde_json::json!({ "channelId": _channel_id, "messageId": message_id, "emoji": reaction_id }),
        )
        .await?;
        Ok(())
    }

    async fn send_interactive(
        &self,
        channel_id: &str,
        text: &str,
        options: &[InteractiveOption],
        reply_to: Option<&str>,
    ) -> Result<SendResult, SendError> {
        // Build a simple card with action buttons
        let actions: Vec<serde_json::Value> = options
            .iter()
            .map(|o| {
                serde_json::json!({
                    "tag": "button",
                    "text": { "tag": "plain_text", "content": o.label },
                    "type": "default",
                    "value": { "action": o.value },
                })
            })
            .collect();

        let card = serde_json::json!({
            "config": { "wide_screen_mode": true },
            "elements": [
                { "tag": "div", "text": { "tag": "lark_md", "content": text } },
                { "tag": "hr" },
                { "tag": "action", "actions": actions },
            ],
        });

        let mut params = serde_json::json!({ "channelId": channel_id, "card": card });
        if let Some(reply_id) = reply_to {
            params["replyTo"] = serde_json::Value::String(reply_id.to_string());
        }

        let result = self.rpc_call("send_interactive", params).await?;
        Ok(result
            .get("messageId")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()))
    }

    async fn update_interactive(
        &self,
        _channel_id: &str,
        message_id: &str,
        text: &str,
        options: &[InteractiveOption],
    ) -> Result<(), SendError> {
        let actions: Vec<serde_json::Value> = options
            .iter()
            .map(|o| {
                serde_json::json!({
                    "tag": "button",
                    "text": { "tag": "plain_text", "content": o.label },
                    "type": "default",
                    "value": { "action": o.value },
                })
            })
            .collect();

        let card = serde_json::json!({
            "config": { "wide_screen_mode": true },
            "elements": [
                { "tag": "div", "text": { "tag": "lark_md", "content": text } },
                { "tag": "hr" },
                { "tag": "action", "actions": actions },
            ],
        });

        self.rpc_call(
            "update_interactive",
            serde_json::json!({ "channelId": _channel_id, "messageId": message_id, "card": card }),
        )
        .await?;
        Ok(())
    }
}
