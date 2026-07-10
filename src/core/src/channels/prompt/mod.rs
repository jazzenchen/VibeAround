//! Channel-input dispatch.
//!
//! [`ConversationIngress`] is the single business-logic and ordering entry
//! point for inbound prompts. Stdio plugins use its request/response
//! [`prompt`](ConversationIngress::prompt) method, while web/TUI inputs use
//! fire-and-forget [`dispatch`](ConversationIngress::dispatch). Both enter a
//! bounded lane keyed by the full [`RouteKey`]; unrelated routes run in
//! parallel, while one route remains FIFO. Stop bypasses the lane so it can
//! cancel a long-running turn immediately.
//!
//! - `Message` / `Callback` → [`handler::handle_prompt`] (workspace thread
//!   slash command parse → thread runtime prompt).
//! - `Stop` / `Close` / `SwitchAgent` → workspace thread control.
//! - `Log` → forward to the daemon log stream.
//!
//! Sub-modules:
//! - [`handler`] — `handle_prompt` + workspace-thread command dispatch.

mod handler;

use std::sync::Arc;

use agent_client_protocol::schema::v1 as acp;
use dashmap::mapref::entry::Entry;
use dashmap::DashMap;
use parking_lot::Mutex as ParkingMutex;
use tokio::sync::{mpsc, oneshot};

use crate::routing::{
    is_external_attachment_uri, is_safe_attachment_file_key, Attachment, RouteKey,
};
use crate::workspace::WorkspaceThreadManager;

use super::plugin_host::PluginHost;
use super::types::{ChannelEnvelope, ChannelInput, ChannelOutput};

pub use handler::{send_runtime_multi_agent_state_and_replay, start_runtime_and_notify};

const ROUTE_LANE_CAPACITY: usize = 16;
const ROUTE_LANE_FULL_MESSAGE: &str =
    "conversation route is busy; wait for an earlier message to finish";

enum LaneCommand {
    Prompt {
        content_blocks: Vec<acp::ContentBlock>,
        reply: oneshot::Sender<acp::Result<acp::PromptResponse>>,
    },
    Dispatch(ChannelInput),
    #[cfg(test)]
    Probe {
        work: std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send>>,
        done: oneshot::Sender<()>,
    },
}

struct RouteLane {
    tx: mpsc::Sender<LaneCommand>,
    accepting: ParkingMutex<bool>,
}

impl RouteLane {
    fn try_send(&self, command: LaneCommand) -> Result<(), mpsc::error::TrySendError<LaneCommand>> {
        let accepting = self.accepting.lock();
        if !*accepting {
            return Err(mpsc::error::TrySendError::Closed(command));
        }
        self.tx.try_send(command)
    }

    fn close_if_empty(&self, rx: &mpsc::Receiver<LaneCommand>) -> bool {
        let mut accepting = self.accepting.lock();
        if rx.is_empty() {
            *accepting = false;
            true
        } else {
            false
        }
    }
}

/// Concrete conversation entry point shared by every channel transport.
///
pub struct ConversationIngress {
    workspace_threads: Arc<WorkspaceThreadManager>,
    plugin_host: Arc<PluginHost>,
    lanes: DashMap<RouteKey, Arc<RouteLane>>,
}

impl ConversationIngress {
    pub(crate) fn new(
        workspace_threads: Arc<WorkspaceThreadManager>,
        plugin_host: Arc<PluginHost>,
    ) -> Self {
        Self {
            workspace_threads,
            plugin_host,
            lanes: DashMap::new(),
        }
    }

    /// Run one prompt to completion and return its actual ACP stop reason.
    pub async fn prompt(
        self: &Arc<Self>,
        route: RouteKey,
        content_blocks: Vec<acp::ContentBlock>,
    ) -> acp::Result<acp::PromptResponse> {
        let (reply, response) = oneshot::channel();
        if self
            .enqueue(
                route,
                LaneCommand::Prompt {
                    content_blocks,
                    reply,
                },
            )
            .is_err()
        {
            return Err(acp::Error::new(-32000, ROUTE_LANE_FULL_MESSAGE));
        }
        response
            .await
            .unwrap_or_else(|_| Err(acp::Error::new(-32603, "conversation route stopped")))
    }

    /// Dispatch a channel command. Stop and log records bypass route queues;
    /// every other command is accepted into the route's bounded FIFO lane.
    pub async fn dispatch(self: &Arc<Self>, input: ChannelInput) {
        match &input {
            ChannelInput::Stop { route } => {
                let _ = self.workspace_threads.cancel_route(route).await;
                return;
            }
            ChannelInput::Log { level, message } => {
                tracing::info!(
                    level = %level.clone().unwrap_or_else(|| "info".to_string()),
                    message = %message,
                    "channel log"
                );
                return;
            }
            _ => {}
        }

        let route = input
            .route_key()
            .expect("non-log channel input must carry a route")
            .clone();
        if let Err(LaneCommand::Dispatch(rejected)) =
            self.enqueue(route.clone(), LaneCommand::Dispatch(input))
        {
            self.reject_full_lane(&route, rejected).await;
        }
    }

    fn enqueue(
        self: &Arc<Self>,
        route: RouteKey,
        mut command: LaneCommand,
    ) -> Result<(), LaneCommand> {
        loop {
            let mut receiver = None;
            let lane = match self.lanes.entry(route.clone()) {
                Entry::Occupied(entry) => Arc::clone(entry.get()),
                Entry::Vacant(entry) => {
                    let (tx, rx) = mpsc::channel(ROUTE_LANE_CAPACITY);
                    let lane = Arc::new(RouteLane {
                        tx,
                        accepting: ParkingMutex::new(true),
                    });
                    entry.insert(Arc::clone(&lane));
                    receiver = Some(rx);
                    lane
                }
            };
            if let Some(rx) = receiver {
                self.spawn_lane(route.clone(), Arc::clone(&lane), rx);
            }

            match lane.try_send(command) {
                Ok(()) => return Ok(()),
                Err(mpsc::error::TrySendError::Full(returned)) => return Err(returned),
                Err(mpsc::error::TrySendError::Closed(returned)) => {
                    command = returned;
                    self.lanes
                        .remove_if(&route, |_, current| Arc::ptr_eq(current, &lane));
                }
            }
        }
    }

    fn spawn_lane(
        self: &Arc<Self>,
        route: RouteKey,
        lane: Arc<RouteLane>,
        mut rx: mpsc::Receiver<LaneCommand>,
    ) {
        let ingress = Arc::clone(self);
        tokio::spawn(async move {
            while let Some(command) = rx.recv().await {
                ingress.execute_lane_command(&route, command).await;
                if lane.close_if_empty(&rx) {
                    ingress
                        .lanes
                        .remove_if(&route, |_, current| Arc::ptr_eq(current, &lane));
                    return;
                }
            }
            ingress
                .lanes
                .remove_if(&route, |_, current| Arc::ptr_eq(current, &lane));
        });
    }

    async fn execute_lane_command(&self, route: &RouteKey, command: LaneCommand) {
        match command {
            LaneCommand::Prompt {
                content_blocks,
                reply,
            } => {
                let result = self.run_prompt(route.clone(), content_blocks).await;
                let _ = reply.send(result);
            }
            LaneCommand::Dispatch(input) => self.dispatch_ordered(input).await,
            #[cfg(test)]
            LaneCommand::Probe { work, done } => {
                work.await;
                let _ = done.send(());
            }
        }
    }

    async fn dispatch_ordered(&self, input: ChannelInput) {
        match input {
            ChannelInput::Message { envelope } => {
                self.handle_prompt_input(envelope, None).await;
            }
            ChannelInput::Callback {
                envelope,
                action_value,
            } => {
                self.handle_prompt_input(envelope, action_value).await;
            }
            ChannelInput::Close { route, reason } => {
                let _ = self.workspace_threads.close_route(&route, reason).await;
            }
            ChannelInput::SwitchAgent { route, agent_kind } => {
                send_system_text(
                    &self.plugin_host,
                    &route,
                    &format!("Use /switch host {} with workspace threads.", agent_kind),
                )
                .await;
            }
            ChannelInput::Stop { route } => {
                let _ = self.workspace_threads.cancel_route(&route).await;
            }
            ChannelInput::Log { .. } => {}
        }
    }

    async fn run_prompt(
        &self,
        route: RouteKey,
        content_blocks: Vec<acp::ContentBlock>,
    ) -> acp::Result<acp::PromptResponse> {
        let result = handler::handle_prompt(
            &self.workspace_threads,
            &self.plugin_host,
            route.clone(),
            content_blocks,
        )
        .await;
        if let Err(error) = &result {
            if let Some(reason) = auto_close_reason_for_prompt_error(error) {
                if let Err(close_error) = self
                    .workspace_threads
                    .close_route(&route, Some(reason))
                    .await
                {
                    tracing::warn!(
                        route = %route,
                        error = %close_error,
                        "failed to auto-close failed workspace thread"
                    );
                }
            }
        }
        if let Err(error) = self
            .workspace_threads
            .schedule_route_host_idle_shutdown(&route)
            .await
        {
            tracing::debug!(
                route = %route,
                error = %error,
                "failed to schedule agent host idle shutdown"
            );
        }
        result
    }

    async fn handle_prompt_input(&self, envelope: ChannelEnvelope, action_value: Option<String>) {
        let route = envelope.route.clone();
        let cli_kind = envelope.cli_kind.clone();
        let text = effective_input_text(&envelope, action_value);
        let message_id = if envelope.message_id.is_empty() {
            None
        } else {
            Some(envelope.message_id.clone())
        };
        tracing::debug!(
            route = %route,
            cli_kind = ?cli_kind,
            text = %text,
            "channel input"
        );

        let content_blocks = envelope_content_blocks(&text, &envelope.attachments);

        match self.run_prompt(route.clone(), content_blocks).await {
            Ok(_resp) => {
                tracing::debug!(route = %route, "prompt ok");
            }
            Err(e) => {
                tracing::warn!(route = %route, error = %e, "prompt failed");
                send_system_text(&self.plugin_host, &route, &format!("❌ {}", e)).await;
            }
        }
        send_prompt_done(&self.plugin_host, &route, message_id).await;
    }

    async fn reject_full_lane(&self, route: &RouteKey, input: ChannelInput) {
        let message_id = match input {
            ChannelInput::Message { envelope } | ChannelInput::Callback { envelope, .. } => {
                (!envelope.message_id.is_empty()).then_some(envelope.message_id)
            }
            _ => None,
        };
        send_system_text(&self.plugin_host, route, ROUTE_LANE_FULL_MESSAGE).await;
        send_prompt_done(&self.plugin_host, route, message_id).await;
    }

    #[cfg(test)]
    fn enqueue_probe<F>(
        self: &Arc<Self>,
        route: RouteKey,
        work: F,
    ) -> Result<oneshot::Receiver<()>, ()>
    where
        F: std::future::Future<Output = ()> + Send + 'static,
    {
        let (done, completed) = oneshot::channel();
        self.enqueue(
            route,
            LaneCommand::Probe {
                work: Box::pin(work),
                done,
            },
        )
        .map_err(|_| ())?;
        Ok(completed)
    }

    #[cfg(test)]
    fn active_lane_count(&self) -> usize {
        self.lanes.len()
    }
}

fn effective_input_text(envelope: &ChannelEnvelope, action_value: Option<String>) -> String {
    if envelope.text.is_empty() {
        action_value.unwrap_or_default()
    } else {
        envelope.text.clone()
    }
}

fn envelope_content_blocks(text: &str, attachments: &[Attachment]) -> Vec<acp::ContentBlock> {
    let mut blocks = Vec::with_capacity(usize::from(!text.is_empty()) + attachments.len());
    if !text.is_empty() {
        blocks.push(acp::ContentBlock::Text(acp::TextContent::new(text)));
    }
    blocks.extend(attachments.iter().filter_map(attachment_content_block));
    blocks
}

fn attachment_content_block(attachment: &Attachment) -> Option<acp::ContentBlock> {
    let uri = match attachment_uri(&attachment.file_key) {
        Some(uri) => uri,
        None => {
            tracing::warn!(
                file_key = %attachment.file_key,
                "dropping attachment with unsafe file key"
            );
            return None;
        }
    };
    let mut link = acp::ResourceLink::new(attachment.file_name.clone(), uri);
    if !attachment.resource_type.trim().is_empty() {
        link.mime_type = Some(attachment.resource_type.clone());
    }
    link.size = attachment.size;
    Some(acp::ContentBlock::ResourceLink(link))
}

fn attachment_uri(file_key: &str) -> Option<String> {
    if is_external_attachment_uri(file_key) {
        return Some(file_key.to_string());
    }
    if !is_safe_attachment_file_key(file_key) {
        return None;
    }
    Some(format!(
        "file://{}",
        crate::config::data_dir()
            .join(".cache")
            .join(file_key)
            .to_string_lossy()
    ))
}

/// Fire-and-forget helper: emit a `SystemText` to the plugin for this route.
/// Shared by every sub-module in this folder.
pub(super) async fn send_system_text(plugin_host: &Arc<PluginHost>, route: &RouteKey, text: &str) {
    plugin_host
        .send_output(ChannelOutput::SystemText {
            route: route.clone(),
            text: text.to_string(),
            reply_to: None,
        })
        .await;
}

async fn send_prompt_done(
    plugin_host: &Arc<PluginHost>,
    route: &RouteKey,
    message_id: Option<String>,
) {
    plugin_host
        .send_output(ChannelOutput::PromptDone {
            route: route.clone(),
            message_id,
        })
        .await;
}

fn auto_close_reason_for_prompt_error(error: &acp::Error) -> Option<String> {
    if error.code == acp::ErrorCode::AuthRequired {
        return Some("agent authentication required".to_string());
    }

    let message = error.message.trim().to_ascii_lowercase();
    if message == "workspace thread is closed" {
        return Some("workspace thread already closed".to_string());
    }
    if message.contains("authentication required") {
        return Some("agent authentication required".to_string());
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    fn test_ingress() -> Arc<ConversationIngress> {
        static NEXT_ID: AtomicU64 = AtomicU64::new(1);
        let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        let base =
            std::env::temp_dir().join(format!("vibearound-route-lane-{}-{id}", std::process::id()));
        let workspace_threads = WorkspaceThreadManager::with_paths(
            base.join("workspaces.jsonl"),
            base.join("threads.jsonl"),
            base.join("attachments.jsonl"),
        );
        let (input_tx, _input_rx) = mpsc::unbounded_channel();
        let plugin_host = Arc::new(PluginHost::new(input_tx));
        Arc::new(ConversationIngress::new(workspace_threads, plugin_host))
    }

    async fn wait_for_lanes_to_drain(ingress: &ConversationIngress) {
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(1);
        while ingress.active_lane_count() != 0 && tokio::time::Instant::now() < deadline {
            tokio::task::yield_now().await;
        }
        assert_eq!(ingress.active_lane_count(), 0);
    }

    fn envelope_with_text(text: &str) -> ChannelEnvelope {
        ChannelEnvelope {
            route: RouteKey::new("feishu", "chat-a"),
            message_id: String::new(),
            turn_id: None,
            text: text.to_string(),
            sender_id: String::new(),
            attachments: Vec::new(),
            parent_id: None,
            cli_kind: None,
        }
    }

    #[test]
    fn callback_action_value_becomes_prompt_text() {
        let envelope = envelope_with_text("");

        assert_eq!(
            effective_input_text(&envelope, Some("approve".to_string())),
            "approve"
        );
    }

    #[test]
    fn message_text_takes_precedence_over_callback_action_value() {
        let envelope = envelope_with_text("typed text");

        assert_eq!(
            effective_input_text(&envelope, Some("button".to_string())),
            "typed text"
        );
    }

    #[test]
    fn channel_envelope_builds_shared_prompt_content() {
        let attachments = vec![Attachment {
            message_id: "message-a".to_string(),
            file_key: "https://example.com/report.md".to_string(),
            file_name: "report.md".to_string(),
            resource_type: "text/markdown".to_string(),
            size: Some(42),
        }];

        let blocks = envelope_content_blocks("review this", &attachments);

        assert_eq!(blocks.len(), 2);
        let acp::ContentBlock::Text(text) = &blocks[0] else {
            panic!("first block must preserve the message text");
        };
        assert_eq!(text.text, "review this");
        assert!(matches!(blocks[1], acp::ContentBlock::ResourceLink(_)));
    }

    #[test]
    fn unsafe_relative_attachment_key_is_rejected() {
        assert!(attachment_uri("../../secret").is_none());
        assert!(attachment_uri(r"nested\secret").is_none());
        assert!(attachment_uri("safe_upload_key").is_some());
        assert_eq!(
            attachment_uri("file:///tmp/report.md").as_deref(),
            Some("file:///tmp/report.md")
        );
    }

    #[test]
    fn auto_close_only_for_unrecoverable_prompt_errors() {
        assert_eq!(
            auto_close_reason_for_prompt_error(&acp::Error::auth_required()).as_deref(),
            Some("agent authentication required")
        );
        assert_eq!(
            auto_close_reason_for_prompt_error(&acp::Error::new(
                -32603,
                "workspace thread is closed"
            ))
            .as_deref(),
            Some("workspace thread already closed")
        );
        assert_eq!(
            auto_close_reason_for_prompt_error(&acp::Error::new(
                -32603,
                "ACP initialize failed for claude: Authentication required"
            ))
            .as_deref(),
            Some("agent authentication required")
        );
        assert!(auto_close_reason_for_prompt_error(&acp::Error::new(
            -32603,
            "upstream request failed"
        ))
        .is_none());
    }

    #[tokio::test]
    async fn same_route_lane_is_fifo_and_removed_when_drained() {
        let ingress = test_ingress();
        let route = RouteKey::new("web", "chat-a");
        let (first_started, first_started_rx) = oneshot::channel();
        let (release_first, release_first_rx) = oneshot::channel();
        let (second_started, mut second_started_rx) = oneshot::channel();

        let first_done = ingress
            .enqueue_probe(route.clone(), async move {
                let _ = first_started.send(());
                let _ = release_first_rx.await;
            })
            .unwrap();
        let second_done = ingress
            .enqueue_probe(route, async move {
                let _ = second_started.send(());
            })
            .unwrap();

        first_started_rx.await.unwrap();
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(50), &mut second_started_rx)
                .await
                .is_err()
        );
        release_first.send(()).unwrap();
        second_started_rx.await.unwrap();
        first_done.await.unwrap();
        second_done.await.unwrap();
        wait_for_lanes_to_drain(&ingress).await;
    }

    #[tokio::test]
    async fn different_routes_do_not_block_each_other() {
        let ingress = test_ingress();
        let (first_started, first_started_rx) = oneshot::channel();
        let (release_first, release_first_rx) = oneshot::channel();
        let (second_started, second_started_rx) = oneshot::channel();

        let first_done = ingress
            .enqueue_probe(RouteKey::new("web", "chat-a"), async move {
                let _ = first_started.send(());
                let _ = release_first_rx.await;
            })
            .unwrap();
        first_started_rx.await.unwrap();
        let second_done = ingress
            .enqueue_probe(RouteKey::new("web", "chat-b"), async move {
                let _ = second_started.send(());
            })
            .unwrap();

        tokio::time::timeout(std::time::Duration::from_millis(100), second_started_rx)
            .await
            .expect("unrelated route was blocked")
            .unwrap();
        release_first.send(()).unwrap();
        first_done.await.unwrap();
        second_done.await.unwrap();
        wait_for_lanes_to_drain(&ingress).await;
    }

    #[tokio::test]
    async fn stop_bypasses_a_busy_route_lane() {
        let ingress = test_ingress();
        let route = RouteKey::new("web", "chat-a");
        let (started, started_rx) = oneshot::channel();
        let (release, release_rx) = oneshot::channel();
        let done = ingress
            .enqueue_probe(route.clone(), async move {
                let _ = started.send(());
                let _ = release_rx.await;
            })
            .unwrap();
        started_rx.await.unwrap();

        tokio::time::timeout(
            std::time::Duration::from_millis(100),
            ingress.dispatch(ChannelInput::Stop { route }),
        )
        .await
        .expect("stop waited behind the active turn");

        release.send(()).unwrap();
        done.await.unwrap();
        wait_for_lanes_to_drain(&ingress).await;
    }

    #[tokio::test]
    async fn route_lane_rejects_work_at_capacity() {
        let ingress = test_ingress();
        let route = RouteKey::new("web", "chat-a");
        let (started, started_rx) = oneshot::channel();
        let (release, release_rx) = oneshot::channel();
        let first_done = ingress
            .enqueue_probe(route.clone(), async move {
                let _ = started.send(());
                let _ = release_rx.await;
            })
            .unwrap();
        started_rx.await.unwrap();

        let mut queued = Vec::new();
        for _ in 0..ROUTE_LANE_CAPACITY {
            queued.push(ingress.enqueue_probe(route.clone(), async {}).unwrap());
        }
        assert!(ingress.enqueue_probe(route, async {}).is_err());

        release.send(()).unwrap();
        first_done.await.unwrap();
        for done in queued {
            done.await.unwrap();
        }
        wait_for_lanes_to_drain(&ingress).await;
    }
}
