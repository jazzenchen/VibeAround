//! Shared types for the hub architecture.

// ---------------------------------------------------------------------------
// Hub lifecycle events (subscribed by ServerDaemon)
// ---------------------------------------------------------------------------

/// Events emitted by hubs for external observers (e.g. ServerDaemon → Dashboard).
#[derive(Debug, Clone)]
pub enum HubEvent {
    // AgentHub
    OnAgentSpawned { key: String, kind: String },
    OnAgentKilled { key: String },

    // SessionHub
    OnSessionCreated { key: String },
    OnSessionDestroyed { key: String },

    // ChannelHub
    OnPluginStarted { channel: String },
    OnPluginStopped { channel: String },
}

// ---------------------------------------------------------------------------
// IDs
// ---------------------------------------------------------------------------

/// Channel kind identifier (e.g. "feishu", "telegram").
pub type ChannelKind = String;

/// Chat identifier within a channel (e.g. Feishu chat_id "oc_xxx").
pub type ChatId = String;

/// Platform message identifier (e.g. Feishu message_id "om_xxx").
pub type MessageId = String;

/// Internal session identifier: "{channel_kind}:{chat_id}:{seq}".
/// A new session is created on first message or /new command.
pub type SessionId = String;

/// Agent CLI session identifier (returned by the CLI after spawn).
pub type CliSessionId = String;

// ---------------------------------------------------------------------------
// Inbound message (from channel plugin)
// ---------------------------------------------------------------------------

/// A message received from a channel plugin.
#[derive(Debug, Clone)]
pub struct InboundMessage {
    /// Channel kind (e.g. "feishu").
    pub channel_kind: ChannelKind,
    /// Chat identifier within the channel.
    pub chat_id: ChatId,
    /// Platform message identifier.
    pub message_id: MessageId,
    /// Message text content.
    pub text: String,
    /// Sender identifier (platform-specific).
    pub sender_id: String,
    /// Attachments (platform-agnostic).
    pub attachments: Vec<Attachment>,
    /// Parent message id (for threaded replies).
    pub parent_id: Option<String>,
}

/// Attachment metadata (platform-agnostic).
#[derive(Debug, Clone)]
pub struct Attachment {
    pub message_id: String,
    pub file_key: String,
    pub file_name: String,
    pub resource_type: String,
}

// ---------------------------------------------------------------------------
// Message status (tracked by SessionHub)
// ---------------------------------------------------------------------------

/// Status of a queued message.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MessageStatus {
    /// Waiting in queue.
    Unreplied,
    /// Currently being processed by an agent.
    Processing,
    /// Agent has finished replying.
    Replied,
}

/// A message entry in the session queue.
#[derive(Debug, Clone)]
pub struct QueuedMessage {
    pub message: InboundMessage,
    pub status: MessageStatus,
}

// ---------------------------------------------------------------------------
// Agent events (from AgentHub back to SessionHub/ChannelHub)
// ---------------------------------------------------------------------------

/// An event from the agent, tagged with routing info.
#[derive(Debug, Clone)]
pub struct AgentReply {
    /// Which channel to route back to.
    pub channel_kind: ChannelKind,
    /// Which chat within the channel.
    pub chat_id: ChatId,
    /// The original user message this is replying to.
    pub message_id: MessageId,
    /// Session this reply belongs to.
    pub session_id: SessionId,
    /// The event payload.
    pub event: AgentReplyEvent,
}

/// Agent reply event variants.
#[derive(Debug, Clone)]
pub enum AgentReplyEvent {
    /// Agent started processing.
    Start,
    /// Streaming text token.
    Token { delta: String },
    /// Thinking/reasoning text.
    Thinking { text: String },
    /// Tool use started.
    ToolUse { tool: String, input: String },
    /// Tool result.
    ToolResult { tool: String, output: String },
    /// Agent finished this turn.
    Complete {
        cli_session_id: Option<CliSessionId>,
        cli_kind: String,
        profile: String,
    },
    /// Agent error.
    Error { error: String },
}

// ---------------------------------------------------------------------------
// Plugin notification (sent to channel plugin via stdin JSON-RPC)
// ---------------------------------------------------------------------------

/// Notification to send to a channel plugin.
#[derive(Debug, Clone)]
pub enum PluginNotification {
    AgentStart { channel_kind: ChannelKind, chat_id: ChatId, message_id: MessageId },
    AgentThinking { channel_kind: ChannelKind, chat_id: ChatId, text: String },
    AgentToken { channel_kind: ChannelKind, chat_id: ChatId, delta: String },
    AgentToolUse { channel_kind: ChannelKind, chat_id: ChatId, tool: String, input: String },
    AgentToolResult { channel_kind: ChannelKind, chat_id: ChatId, tool: String, output: String },
    AgentEnd { channel_kind: ChannelKind, chat_id: ChatId },
    AgentError { channel_kind: ChannelKind, chat_id: ChatId, error: String },
    /// Direct text message (command responses, system messages).
    SendText { channel_kind: ChannelKind, chat_id: ChatId, text: String, reply_to: Option<MessageId> },
}

impl PluginNotification {
    /// The channel_id expected by the plugin protocol: "{channel_kind}:{chat_id}".
    fn plugin_channel_id(channel_kind: &str, chat_id: &str) -> String {
        format!("{}:{}", channel_kind, chat_id)
    }

    /// Convert to JSON-RPC notification format (compatible with existing plugin protocol).
    pub fn to_jsonrpc(&self) -> serde_json::Value {
        match self {
            Self::AgentStart { channel_kind, chat_id, message_id } => serde_json::json!({
                "jsonrpc": "2.0", "method": "agent_start",
                "params": { "channelId": Self::plugin_channel_id(channel_kind, chat_id), "userMessageId": message_id }
            }),
            Self::AgentThinking { channel_kind, chat_id, text } => serde_json::json!({
                "jsonrpc": "2.0", "method": "agent_thinking",
                "params": { "channelId": Self::plugin_channel_id(channel_kind, chat_id), "text": text }
            }),
            Self::AgentToken { channel_kind, chat_id, delta } => serde_json::json!({
                "jsonrpc": "2.0", "method": "agent_token",
                "params": { "channelId": Self::plugin_channel_id(channel_kind, chat_id), "delta": delta }
            }),
            Self::AgentToolUse { channel_kind, chat_id, tool, input } => serde_json::json!({
                "jsonrpc": "2.0", "method": "agent_tool_use",
                "params": { "channelId": Self::plugin_channel_id(channel_kind, chat_id), "tool": tool, "input": input }
            }),
            Self::AgentToolResult { channel_kind, chat_id, tool, output } => serde_json::json!({
                "jsonrpc": "2.0", "method": "agent_tool_result",
                "params": { "channelId": Self::plugin_channel_id(channel_kind, chat_id), "tool": tool, "output": output }
            }),
            Self::AgentEnd { channel_kind, chat_id } => serde_json::json!({
                "jsonrpc": "2.0", "method": "agent_end",
                "params": { "channelId": Self::plugin_channel_id(channel_kind, chat_id) }
            }),
            Self::AgentError { channel_kind, chat_id, error } => serde_json::json!({
                "jsonrpc": "2.0", "method": "agent_error",
                "params": { "channelId": Self::plugin_channel_id(channel_kind, chat_id), "error": error }
            }),
            Self::SendText { channel_kind, chat_id, text, reply_to } => serde_json::json!({
                "jsonrpc": "2.0", "method": "send_text",
                "params": { "channelId": Self::plugin_channel_id(channel_kind, chat_id), "text": text, "replyTo": reply_to }
            }),
        }
    }
}
