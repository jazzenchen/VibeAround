//! Shared types for SessionHub, AgentManager, and ChannelManager.

/// Events emitted by hubs for external observers (e.g. ServerDaemon → Dashboard).
#[derive(Debug, Clone)]
pub enum HubEvent {
    OnAgentSpawned { key: String, kind: String },
    OnAgentKilled { key: String },
    OnSessionCreated { key: String },
    OnSessionDestroyed { key: String },
    OnPluginStarted { channel: String },
    OnPluginStopped { channel: String },
}

/// Channel kind identifier (e.g. "feishu", "telegram").
pub type ChannelKind = String;

/// Chat identifier within a channel (e.g. Feishu chat_id "oc_xxx").
pub type ChatId = String;

/// Platform message identifier (e.g. Feishu message_id "om_xxx").
pub type MessageId = String;

/// Internal session identifier: "{channel_kind}:{chat_id}:{seq}".
pub type SessionId = String;

/// Agent CLI session identifier (returned by the CLI after spawn).
pub type CliSessionId = String;

/// A message received from a channel plugin.
#[derive(Debug, Clone)]
pub struct InboundMessage {
    pub channel_kind: ChannelKind,
    pub chat_id: ChatId,
    pub message_id: MessageId,
    pub text: String,
    pub sender_id: String,
    pub attachments: Vec<Attachment>,
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

/// Status of a queued message.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MessageStatus {
    Unreplied,
    Processing,
    Replied,
}

/// A message entry in the session queue.
#[derive(Debug, Clone)]
pub struct QueuedMessage {
    pub message: InboundMessage,
    pub status: MessageStatus,
}

/// Lifecycle signal emitted when an agent session becomes usable.
#[derive(Debug, Clone)]
pub struct AgentReady {
    pub channel_kind: ChannelKind,
    pub chat_id: ChatId,
    pub message_id: MessageId,
    pub session_id: SessionId,
    pub cli_kind: String,
    pub cli_session_id: CliSessionId,
    pub profile: String,
}

/// Lifecycle signal emitted when an agent session closes.
#[derive(Debug, Clone)]
pub struct AgentClosed {
    pub channel_kind: ChannelKind,
    pub chat_id: ChatId,
    pub session_id: SessionId,
    pub cli_kind: Option<String>,
    pub cli_session_id: Option<CliSessionId>,
    pub profile: Option<String>,
    pub reason: String,
}

/// An event from the agent, tagged with routing info.
#[derive(Debug, Clone)]
pub struct AgentReply {
    pub channel_kind: ChannelKind,
    pub chat_id: ChatId,
    pub message_id: MessageId,
    pub session_id: SessionId,
    pub event: AgentReplyEvent,
}

/// Agent reply event variants.
#[derive(Debug, Clone)]
pub enum AgentReplyEvent {
    Start,
    Token { delta: String },
    Thinking { text: String },
    ToolUse { tool: String, input: String },
    ToolResult { tool: String, output: String },
    Complete,
    Error { error: String },
}

/// SessionHub -> AgentManager event skeleton for the new architecture.
#[derive(Debug, Clone)]
pub enum AgentEvent {
    OnStartRuntime {
        channel_kind: ChannelKind,
        chat_id: ChatId,
        message_id: MessageId,
        cli_kind: Option<String>,
        profile: Option<String>,
    },
    OnReceiveMessage {
        channel_kind: ChannelKind,
        chat_id: ChatId,
        message: InboundMessage,
    },
    OnStopRuntime {
        channel_kind: ChannelKind,
        chat_id: ChatId,
    },
    OnCloseRuntime {
        channel_kind: ChannelKind,
        chat_id: ChatId,
        reason: Option<String>,
    },
}

/// SessionHub -> ChannelManager event skeleton for the new architecture.
#[derive(Debug, Clone)]
pub enum ChannelEvent {
    OnSessionStarted {
        channel_kind: ChannelKind,
        chat_id: ChatId,
    },
    OnAgentSessionReady {
        channel_kind: ChannelKind,
        chat_id: ChatId,
        message_id: MessageId,
        cli_kind: String,
        cli_session_id: CliSessionId,
        profile: String,
    },
    OnTurnStarted {
        channel_kind: ChannelKind,
        chat_id: ChatId,
        message_id: MessageId,
    },
    OnTurnCompleted {
        channel_kind: ChannelKind,
        chat_id: ChatId,
    },
    OnSessionClosed {
        channel_kind: ChannelKind,
        chat_id: ChatId,
        reason: Option<String>,
    },
    OnSessionError {
        channel_kind: ChannelKind,
        chat_id: ChatId,
        error: String,
    },
    OnSystemText {
        channel_kind: ChannelKind,
        chat_id: ChatId,
        text: String,
        reply_to: Option<MessageId>,
    },
    OnAcpEvent {
        channel_kind: ChannelKind,
        chat_id: ChatId,
        message_id: MessageId,
        payload: serde_json::Value,
    },
}

/// Notification to send to a channel transport.
#[derive(Debug, Clone)]
pub enum ChannelNotification {
    AgentStart { channel_kind: ChannelKind, chat_id: ChatId, message_id: MessageId },
    AgentThinking { channel_kind: ChannelKind, chat_id: ChatId, text: String },
    AgentToken { channel_kind: ChannelKind, chat_id: ChatId, delta: String },
    AgentToolUse { channel_kind: ChannelKind, chat_id: ChatId, tool: String, input: String },
    AgentToolResult { channel_kind: ChannelKind, chat_id: ChatId, tool: String, output: String },
    AgentEnd { channel_kind: ChannelKind, chat_id: ChatId },
    AgentError { channel_kind: ChannelKind, chat_id: ChatId, error: String },
    SendText { channel_kind: ChannelKind, chat_id: ChatId, text: String, reply_to: Option<MessageId> },
}

impl ChannelNotification {
    fn plugin_channel_id(channel_kind: &str, chat_id: &str) -> String {
        format!("{}:{}", channel_kind, chat_id)
    }

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
