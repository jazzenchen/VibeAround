//! Wire types for the legacy stdio plugin transport.
//!
//! These structs flow between the host and stdio plugins via JSON. They
//! pre-date the ACP-native path that `ws_chat` uses today, but they are still
//! the common currency for every plugin subprocess.

use serde::{Deserialize, Serialize};

use crate::routing::{Attachment, MessageId, RouteKey, TurnId};

pub const CHANNEL_CONTEXT_META_KEY: &str = "va.channel";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ConversationScope {
    Dm,
    Group,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AddressedBy {
    Dm,
    Mention,
    Command,
    Callback,
    Unaddressed,
}

/// Platform-neutral routing metadata attached to an inbound channel prompt.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChannelInboundContext {
    pub channel_instance_id: String,
    pub actor_id: String,
    pub chat_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub topic_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sender_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub platform_message_id: Option<String>,
    pub scope: ConversationScope,
    pub addressed_by: AddressedBy,
}

impl ChannelInboundContext {
    pub fn is_prompt_allowed(&self) -> bool {
        self.scope == ConversationScope::Dm
            || matches!(
                self.addressed_by,
                AddressedBy::Mention | AddressedBy::Callback
            )
    }
}

/// Legacy envelope kept for stdio plugin compatibility.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChannelEnvelope {
    pub route: RouteKey,
    #[serde(default)]
    pub message_id: MessageId,
    #[serde(default)]
    pub turn_id: Option<TurnId>,
    #[serde(default)]
    pub text: String,
    #[serde(default)]
    pub sender_id: String,
    #[serde(default)]
    pub attachments: Vec<Attachment>,
    #[serde(default)]
    pub parent_id: Option<String>,
    #[serde(default)]
    pub cli_kind: Option<String>,
}

impl ChannelEnvelope {
    pub fn reply_to(&self) -> Option<MessageId> {
        (!self.message_id.is_empty()).then(|| self.message_id.clone())
    }
}

/// Legacy stdio plugin input.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum ChannelInput {
    Message {
        #[serde(flatten)]
        envelope: ChannelEnvelope,
    },
    Callback {
        #[serde(flatten)]
        envelope: ChannelEnvelope,
        #[serde(default)]
        action_value: Option<String>,
    },
    Stop {
        route: RouteKey,
    },
    Close {
        route: RouteKey,
        #[serde(default)]
        reason: Option<String>,
    },
    SwitchAgent {
        route: RouteKey,
        agent_kind: String,
    },
    Log {
        #[serde(default)]
        level: Option<String>,
        message: String,
    },
}

impl ChannelInput {
    pub fn route_key(&self) -> Option<&RouteKey> {
        match self {
            Self::Message { envelope } | Self::Callback { envelope, .. } => Some(&envelope.route),
            Self::Stop { route } | Self::Close { route, .. } | Self::SwitchAgent { route, .. } => {
                Some(route)
            }
            Self::Log { .. } => None,
        }
    }
}

/// Channel plugin output.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum ChannelOutput {
    ThreadReply {
        route: RouteKey,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reply_to: Option<MessageId>,
        reply: ThreadReply,
    },
    RawAcp {
        route: RouteKey,
        payload: serde_json::Value,
    },
    SystemText {
        route: RouteKey,
        text: String,
        reply_to: Option<MessageId>,
    },
    AgentReady {
        route: RouteKey,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reply_to: Option<MessageId>,
        agent: String,
        version: String,
    },
    SessionReady {
        route: RouteKey,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reply_to: Option<MessageId>,
        session_id: String,
    },
    SessionInfo {
        route: RouteKey,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reply_to: Option<MessageId>,
        info: ChannelSessionInfo,
    },
    SessionMode {
        route: RouteKey,
        session_mode: serde_json::Value,
    },
    CommandMenu {
        route: RouteKey,
        system_commands: serde_json::Value,
        agent_commands: serde_json::Value,
    },
    PromptDone {
        route: RouteKey,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        message_id: Option<MessageId>,
    },
    TurnStatus {
        route: RouteKey,
        active: bool,
    },
    /// Forward a `requestPermission` ACP call from the upstream agent down to
    /// the plugin. The plugin answers via its `client.requestPermission`
    /// handler (standard ACP), and the forwarder task sends the response back
    /// via the oneshot registered in `PluginHost::pending_permissions`.
    ///
    /// `request_id` matches the entry in `pending_permissions`.
    /// `payload` is a JSON-serialized `acp::RequestPermissionRequest`.
    PermissionRequest {
        route: RouteKey,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reply_to: Option<MessageId>,
        request_id: String,
        payload: serde_json::Value,
    },
    MultiAgentTurn {
        route: RouteKey,
        turn: crate::workspace::threads::MultiAgentTurn,
        agents: Vec<crate::workspace::threads::ThreadAgent>,
    },
    SubagentStatus {
        route: RouteKey,
        agent: crate::workspace::threads::ThreadAgent,
    },
    SubagentAcp {
        route: RouteKey,
        agent: crate::workspace::threads::ThreadAgent,
        payload: serde_json::Value,
    },
}

impl ChannelOutput {
    pub fn route_key(&self) -> &RouteKey {
        match self {
            Self::ThreadReply { route, .. }
            | Self::RawAcp { route, .. }
            | Self::SystemText { route, .. }
            | Self::AgentReady { route, .. }
            | Self::SessionReady { route, .. }
            | Self::SessionInfo { route, .. }
            | Self::SessionMode { route, .. }
            | Self::CommandMenu { route, .. }
            | Self::PromptDone { route, .. }
            | Self::TurnStatus { route, .. }
            | Self::PermissionRequest { route, .. }
            | Self::MultiAgentTurn { route, .. }
            | Self::SubagentStatus { route, .. }
            | Self::SubagentAcp { route, .. } => route,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChannelSessionInfo {
    pub workspace_id: String,
    pub workspace_path: String,
    pub thread_id: String,
    pub agent: ChannelSessionAgent,
    pub session_id: String,
    pub start: ChannelSessionStart,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChannelSessionAgent {
    pub id: String,
    pub name: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub version: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub profile_id: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChannelSessionStart {
    New,
    Resumed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadReply {
    pub workspace_id: String,
    pub thread_id: String,
    pub agent: ThreadReplyAgent,
    pub payload: ThreadReplyPayload,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadReplyAgent {
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub profile: Option<String>,
    pub session_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ThreadReplyPayload {
    AcpSessionNotification { notification: serde_json::Value },
}
