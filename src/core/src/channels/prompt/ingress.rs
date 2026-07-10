use std::sync::Arc;

use agent_client_protocol::schema::v1 as acp;
use dashmap::mapref::entry::Entry;
use dashmap::DashMap;
use parking_lot::Mutex as ParkingMutex;
use tokio::sync::{mpsc, oneshot};

use crate::workspace::WorkspaceThreadManager;

use super::{
    auto_close_reason_for_prompt_error, effective_input_text, envelope_content_blocks, handler,
    send_prompt_done, send_system_text, ChannelEnvelope, ChannelInput, PluginHost, RouteKey,
};

pub(super) const ROUTE_LANE_CAPACITY: usize = 16;
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
    pub(super) fn enqueue_probe<F>(
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
    pub(super) fn active_lane_count(&self) -> usize {
        self.lanes.len()
    }
}
