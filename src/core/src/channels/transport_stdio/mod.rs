//! Stdio plugin transport using ACP protocol.
//!
//! The host acts as an ACP Agent toward the plugin (which acts as an ACP
//! Client). Host sends `va/thread_reply` extension notifications to
//! stream workspace-thread replies back to the plugin and receives `prompt()` /
//! `ext_notification` / `cancel` from it.
//!
//! ## Session ID convention
//!
//! ACP requires a `sessionId` on `PromptRequest`. Channel plugins use the
//! **chat room identifier** (chatId) as the ACP `sessionId`. This is NOT
//! the real agent session — the host maps `(channelKind, chatId)` to an
//! internal `RouteKey` and manages the real agent session transparently.
//! When forwarding `SessionNotification` back to the plugin, the host keeps
//! the real agent `sessionId` in the notification payload and carries the
//! plugin route separately as `chatId`.
//!
//! ## Prompt lifecycle
//!
//! Plugin calls `prompt()` → host calls the workspace-thread prompt handler →
//! session notifications stream to plugin during processing →
//! `prompt()` returns the real `PromptResponse` with actual `StopReason`.
//!
//! ## Module layout
//!
//! - `runtime`    — `StdioPluginRuntime` (output-sender shell; no lifecycle)
//! - `bridge`     — `run_acp_plugin_bridge` (the ACP IO driver, cancel-aware)
//! - `forwarder`  — `ChannelOutput` → ACP Client-method dispatch
//! - `handler`    — `PluginAgentHandler` (`acp::Agent` impl consumed by the plugin)
//!
//! Spawn + supervise lives in `process::Supervisor`; the ACP bridge is
//! wrapped into a `ProcessBridge` by `channels::plugin_bridge`.

mod bridge;
mod forwarder;
mod handler;
mod runtime;

pub(crate) use bridge::run_acp_plugin_bridge;
pub use runtime::StdioPluginRuntime;
