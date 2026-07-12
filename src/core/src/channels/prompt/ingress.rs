use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use agent_client_protocol::schema::v1 as acp;
use dashmap::mapref::entry::Entry;
use dashmap::DashMap;
use parking_lot::Mutex as ParkingMutex;
use tokio::sync::{mpsc, oneshot, watch, Notify};

#[cfg(test)]
use crate::routing::wait_for_signal;
use crate::routing::ChannelTarget;
use crate::workspace::threads::runtime::PromptCancellation;
use crate::workspace::WorkspaceThreadManager;

use super::{
    auto_close_reason_for_prompt_error, effective_input_text, envelope_content_blocks, handler,
    send_prompt_done, send_system_text, send_system_text_to_target, ChannelEnvelope, ChannelInput,
    PluginHost, RouteKey,
};

pub(super) const ROUTE_LANE_CAPACITY: usize = 16;
const ROUTE_LANE_FULL_MESSAGE: &str =
    "conversation route is busy; wait for an earlier message to finish";
const ROUTE_STOPPED_MESSAGE: &str = "conversation route stopped";

enum LaneCommand {
    Prompt {
        reply_to: Option<String>,
        content_blocks: Vec<acp::ContentBlock>,
        reply: oneshot::Sender<acp::Result<acp::PromptResponse>>,
    },
    Dispatch(Box<OrderedInput>),
    #[cfg(test)]
    Probe {
        work: std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send>>,
        done: oneshot::Sender<()>,
    },
}

#[derive(Clone)]
enum OrderedInput {
    Message {
        envelope: ChannelEnvelope,
    },
    Callback {
        envelope: ChannelEnvelope,
        action_value: Option<String>,
    },
    SwitchAgent {
        route: RouteKey,
        agent_kind: String,
    },
}

impl OrderedInput {
    fn route_key(&self) -> &RouteKey {
        match self {
            Self::Message { envelope } | Self::Callback { envelope, .. } => &envelope.route,
            Self::SwitchAgent { route, .. } => route,
        }
    }

    fn reply_to(&self) -> Option<String> {
        match self {
            Self::Message { envelope } | Self::Callback { envelope, .. } => envelope.reply_to(),
            Self::SwitchAgent { .. } => None,
        }
    }
}

struct QueuedCommand {
    cancellation: watch::Receiver<bool>,
    command: LaneCommand,
}

struct RouteLaneState {
    accepting: bool,
    cancel_tx: watch::Sender<bool>,
}

struct RouteLane {
    tx: mpsc::Sender<QueuedCommand>,
    state: ParkingMutex<RouteLaneState>,
}

impl RouteLane {
    fn try_send(&self, command: LaneCommand) -> Result<(), mpsc::error::TrySendError<LaneCommand>> {
        let state = self.state.lock();
        if !state.accepting {
            return Err(mpsc::error::TrySendError::Closed(command));
        }
        self.tx
            .try_send(QueuedCommand {
                cancellation: state.cancel_tx.subscribe(),
                command,
            })
            .map_err(|error| match error {
                mpsc::error::TrySendError::Full(queued) => {
                    mpsc::error::TrySendError::Full(queued.command)
                }
                mpsc::error::TrySendError::Closed(queued) => {
                    mpsc::error::TrySendError::Closed(queued.command)
                }
            })
    }

    fn stop(&self) -> bool {
        let mut state = self.state.lock();
        if !state.accepting {
            return false;
        }
        state.cancel_tx.send_replace(true);
        state.cancel_tx = watch::channel(false).0;
        true
    }

    fn close_if_empty(&self, rx: &mpsc::Receiver<QueuedCommand>) -> bool {
        let mut state = self.state.lock();
        if rx.is_empty() {
            state.accepting = false;
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
    lifecycle: ParkingMutex<()>,
    shutdown_tx: watch::Sender<bool>,
    active_lane_tasks: AtomicUsize,
    lanes_drained: Notify,
}

impl ConversationIngress {
    pub(crate) fn new(
        workspace_threads: Arc<WorkspaceThreadManager>,
        plugin_host: Arc<PluginHost>,
    ) -> Self {
        let (shutdown_tx, _) = watch::channel(false);
        Self {
            workspace_threads,
            plugin_host,
            lanes: DashMap::new(),
            lifecycle: ParkingMutex::new(()),
            shutdown_tx,
            active_lane_tasks: AtomicUsize::new(0),
            lanes_drained: Notify::new(),
        }
    }

    /// Run one prompt to completion and return its actual ACP stop reason.
    pub async fn prompt(
        self: &Arc<Self>,
        target: ChannelTarget,
        content_blocks: Vec<acp::ContentBlock>,
    ) -> acp::Result<acp::PromptResponse> {
        let (reply, response) = oneshot::channel();
        if self
            .enqueue(
                target.route.clone(),
                LaneCommand::Prompt {
                    reply_to: target.reply_to,
                    content_blocks,
                    reply,
                },
            )
            .is_err()
        {
            return Err(acp::Error::new(
                -32000,
                if *self.shutdown_tx.borrow() {
                    ROUTE_STOPPED_MESSAGE
                } else {
                    ROUTE_LANE_FULL_MESSAGE
                },
            ));
        }
        response
            .await
            .unwrap_or_else(|_| Err(acp::Error::new(-32603, "conversation route stopped")))
    }

    /// Dispatch a channel command. Stop, Close, and log records bypass route queues;
    /// every other command is accepted into the route's bounded FIFO lane.
    pub fn dispatch(self: &Arc<Self>, input: ChannelInput) {
        let input = match input {
            ChannelInput::Stop { route } => {
                let stopped_lane = self.lanes.get(&route).is_some_and(|lane| lane.stop());
                if !stopped_lane {
                    let workspace_threads = Arc::clone(&self.workspace_threads);
                    let route = route.clone();
                    tokio::spawn(async move {
                        let _ = workspace_threads.cancel_route(&route).await;
                    });
                }
                return;
            }
            ChannelInput::Close { route, reason } => {
                let workspace_threads = Arc::clone(&self.workspace_threads);
                tokio::spawn(async move {
                    let _ = workspace_threads.close_route(&route, reason).await;
                });
                return;
            }
            ChannelInput::Log { level, message } => {
                tracing::info!(
                    level = %level.unwrap_or_else(|| "info".to_string()),
                    message = %message,
                    "channel log"
                );
                return;
            }
            ChannelInput::Message { envelope } => OrderedInput::Message { envelope },
            ChannelInput::Callback {
                envelope,
                action_value,
            } => OrderedInput::Callback {
                envelope,
                action_value,
            },
            ChannelInput::SwitchAgent { route, agent_kind } => {
                OrderedInput::SwitchAgent { route, agent_kind }
            }
        };

        let route = input.route_key().clone();
        if let Err(LaneCommand::Dispatch(rejected)) =
            self.enqueue(route.clone(), LaneCommand::Dispatch(Box::new(input)))
        {
            self.reject_full_lane(&route, *rejected);
        }
    }

    fn enqueue(
        self: &Arc<Self>,
        route: RouteKey,
        mut command: LaneCommand,
    ) -> Result<(), LaneCommand> {
        // Serialize lane creation with shutdown. Once shutdown owns this
        // guard, no task can appear after the drain barrier observes zero.
        let _lifecycle = self.lifecycle.lock();
        if *self.shutdown_tx.borrow() {
            return Err(command);
        }
        loop {
            let mut receiver = None;
            let lane = match self.lanes.entry(route.clone()) {
                Entry::Occupied(entry) => Arc::clone(entry.get()),
                Entry::Vacant(entry) => {
                    let (tx, rx) = mpsc::channel(ROUTE_LANE_CAPACITY);
                    let (cancel_tx, _) = watch::channel(false);
                    let lane = Arc::new(RouteLane {
                        tx,
                        state: ParkingMutex::new(RouteLaneState {
                            accepting: true,
                            cancel_tx,
                        }),
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
        mut rx: mpsc::Receiver<QueuedCommand>,
    ) {
        self.active_lane_tasks.fetch_add(1, Ordering::AcqRel);
        let ingress = Arc::clone(self);
        tokio::spawn(async move {
            let mut shutdown_rx = ingress.shutdown_tx.subscribe();
            while let Some(queued) = rx.recv().await {
                ingress
                    .execute_lane_command(&route, queued, &mut shutdown_rx)
                    .await;
                if *shutdown_rx.borrow() {
                    break;
                }
                if lane.close_if_empty(&rx) {
                    ingress
                        .lanes
                        .remove_if(&route, |_, current| Arc::ptr_eq(current, &lane));
                    break;
                }
            }
            ingress
                .lanes
                .remove_if(&route, |_, current| Arc::ptr_eq(current, &lane));
            if ingress.active_lane_tasks.fetch_sub(1, Ordering::AcqRel) == 1 {
                ingress.lanes_drained.notify_waiters();
            }
        });
    }

    async fn execute_lane_command(
        &self,
        route: &RouteKey,
        queued: QueuedCommand,
        shutdown_rx: &mut watch::Receiver<bool>,
    ) {
        let cancellation = queued.cancellation;
        match queued.command {
            LaneCommand::Prompt {
                reply_to,
                content_blocks,
                reply,
            } => {
                if *cancellation.borrow() {
                    let _ = reply.send(cancelled_prompt_response());
                    return;
                }
                let prompt = self.run_prompt(
                    ChannelTarget::new(route.clone(), reply_to),
                    content_blocks,
                    PromptCancellation::new(cancellation.clone(), shutdown_rx.clone()),
                );
                let result = prompt.await;
                self.schedule_route_host_idle_shutdown(route).await;
                let _ = reply.send(result);
            }
            LaneCommand::Dispatch(input) => {
                let ran_prompt = matches!(
                    input.as_ref(),
                    OrderedInput::Message { .. } | OrderedInput::Callback { .. }
                );
                if *cancellation.borrow() {
                    self.reject_stopped(route, *input);
                    return;
                }
                self.dispatch_ordered(
                    (*input).clone(),
                    PromptCancellation::new(cancellation.clone(), shutdown_rx.clone()),
                )
                .await;
                if ran_prompt {
                    self.schedule_route_host_idle_shutdown(route).await;
                }
            }
            #[cfg(test)]
            LaneCommand::Probe { work, done } => {
                let mut cancellation = cancellation;
                tokio::select! {
                    biased;
                    _ = wait_for_signal(&mut cancellation) => {}
                    _ = wait_for_signal(shutdown_rx) => {}
                    _ = work => { let _ = done.send(()); }
                }
            }
        }
    }

    /// Stop accepting new work, cancel every active/queued lane command, and
    /// wait until all lane tasks have released their references.
    pub async fn shutdown(&self) {
        {
            let _lifecycle = self.lifecycle.lock();
            self.shutdown_tx.send_replace(true);
            for lane in self.lanes.iter() {
                let mut state = lane.state.lock();
                state.accepting = false;
            }
        }
        loop {
            let notified = self.lanes_drained.notified();
            if self.active_lane_tasks.load(Ordering::Acquire) == 0 {
                break;
            }
            notified.await;
        }
    }

    async fn dispatch_ordered(&self, input: OrderedInput, cancellation: PromptCancellation) {
        match input {
            OrderedInput::Message { envelope } => {
                self.handle_prompt_input(envelope, None, cancellation).await;
            }
            OrderedInput::Callback {
                envelope,
                action_value,
            } => {
                self.handle_prompt_input(envelope, action_value, cancellation)
                    .await;
            }
            OrderedInput::SwitchAgent { route, agent_kind } => {
                send_system_text(
                    &self.plugin_host,
                    &route,
                    &format!("Use /switch host {} with workspace threads.", agent_kind),
                );
            }
        }
    }

    async fn run_prompt(
        &self,
        target: ChannelTarget,
        content_blocks: Vec<acp::ContentBlock>,
        cancellation: PromptCancellation,
    ) -> acp::Result<acp::PromptResponse> {
        let result = handler::handle_prompt(
            &self.workspace_threads,
            &self.plugin_host,
            target.clone(),
            content_blocks,
            cancellation,
        )
        .await;
        if let Err(error) = &result {
            if let Some(reason) = auto_close_reason_for_prompt_error(error) {
                if let Err(close_error) = self
                    .workspace_threads
                    .close_route(&target.route, Some(reason))
                    .await
                {
                    tracing::warn!(
                        route = %target.route,
                        error = %close_error,
                        "failed to auto-close failed workspace thread"
                    );
                }
            }
        }
        result
    }

    async fn schedule_route_host_idle_shutdown(&self, route: &RouteKey) {
        if let Err(error) = self
            .workspace_threads
            .schedule_route_host_idle_shutdown(route)
            .await
        {
            tracing::debug!(
                route = %route,
                error = %error,
                "failed to schedule agent host idle shutdown"
            );
        }
    }

    async fn handle_prompt_input(
        &self,
        envelope: ChannelEnvelope,
        action_value: Option<String>,
        cancellation: PromptCancellation,
    ) {
        let route = envelope.route.clone();
        let cli_kind = envelope.cli_kind.clone();
        let text = effective_input_text(&envelope, action_value);
        let message_id = envelope.reply_to();
        tracing::debug!(
            route = %route,
            cli_kind = ?cli_kind,
            text = %text,
            "channel input"
        );

        let content_blocks = envelope_content_blocks(&text, &envelope.attachments);

        let target = ChannelTarget::new(route.clone(), message_id.clone());
        match self
            .run_prompt(target.clone(), content_blocks, cancellation)
            .await
        {
            Ok(_resp) => {
                tracing::debug!(route = %route, "prompt ok");
            }
            Err(e) => {
                tracing::warn!(route = %route, error = %e, "prompt failed");
                send_system_text_to_target(&self.plugin_host, &target, &format!("❌ {}", e));
            }
        }
        send_prompt_done(&self.plugin_host, &route, message_id);
    }

    fn reject_full_lane(&self, route: &RouteKey, input: OrderedInput) {
        let message_id = input.reply_to();
        let target = ChannelTarget::new(route.clone(), message_id.clone());
        send_system_text_to_target(&self.plugin_host, &target, ROUTE_LANE_FULL_MESSAGE);
        send_prompt_done(&self.plugin_host, route, message_id);
    }

    fn reject_stopped(&self, route: &RouteKey, input: OrderedInput) {
        let message_id = input.reply_to();
        send_prompt_done(&self.plugin_host, route, message_id);
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

fn cancelled_prompt_response() -> acp::Result<acp::PromptResponse> {
    Ok(acp::PromptResponse::new(acp::StopReason::Cancelled))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stopped_lane_does_not_claim_a_stop() {
        let (tx, _rx) = mpsc::channel(1);
        let (cancel_tx, _) = watch::channel(false);
        let lane = RouteLane {
            tx,
            state: ParkingMutex::new(RouteLaneState {
                accepting: false,
                cancel_tx,
            }),
        };

        assert!(!lane.stop());
    }

    #[test]
    fn session_cancel_completes_the_prompt_with_cancelled_stop_reason() {
        let response = cancelled_prompt_response().expect("cancel is a successful ACP result");
        assert_eq!(response.stop_reason, acp::StopReason::Cancelled);
    }
}
