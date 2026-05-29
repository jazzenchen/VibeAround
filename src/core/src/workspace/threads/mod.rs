//! Workspace thread domain model.

pub mod attachment;
pub mod runtime;
pub mod store;

pub use attachment::{RouteAttachment, RouteAttachmentProjection};
pub use runtime::{ThreadRuntime, ThreadRuntimeState};
pub use store::{
    AgentSessionRef, HostBinding, MultiAgentTurn, MultiAgentTurnId, MultiAgentTurnMode,
    ThreadAgent, ThreadAgentId, ThreadAgentStatus, ThreadProjection, ThreadStatus, WorkspaceThread,
    WorkspaceThreadId,
};
