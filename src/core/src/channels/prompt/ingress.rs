use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;

use agent_client_protocol::schema::v1 as acp;
use dashmap::mapref::entry::Entry;
use dashmap::DashMap;
use parking_lot::Mutex as ParkingMutex;
use tokio::sync::{mpsc, oneshot, watch, Notify};

use crate::routing::ChannelTarget;
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
    Dispatch(Box<ChannelInput>),
    #[cfg(test)]
    Probe {
        work: std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send>>,
        done: oneshot::Sender<()>,
    },
}

struct QueuedCommand {
    stop_generation: u64,
    command: LaneCommand,
}

struct RouteLaneState {
    accepting: bool,
    stop_generation: u64,
}

struct RouteLane {
    tx: mpsc::Sender<QueuedCommand>,
    state: ParkingMutex<RouteLaneState>,
    stop_tx: watch::Sender<u64>,
}

impl RouteLane {
    fn try_send(&self, command: LaneCommand) -> Result<(), mpsc::error::TrySendError<LaneCommand>> {
        let state = self.state.lock();
        if !state.accepting {
            return Err(mpsc::error::TrySendError::Closed(command));
        }
        self.tx
            .try_send(QueuedCommand {
                stop_generation: state.stop_generation,
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

    fn stop(&self) {
        let mut state = self.state.lock();
        state.stop_generation = state.stop_generation.wrapping_add(1);
        self.stop_tx.send_replace(state.stop_generation);
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
    shutting_down: AtomicBool,
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
            shutting_down: AtomicBool::new(false),
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
                if self.shutting_down.load(Ordering::Acquire) {
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
        match &input {
            ChannelInput::Stop { route } => {
                if let Some(lane) = self.lanes.get(route) {
                    // The active lane performs the actual runtime cancel before
                    // it can run a command from the next route generation.
                    lane.stop();
                } else {
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
                let route = route.clone();
                let reason = reason.clone();
                tokio::spawn(async move {
                    let _ = workspace_threads.close_route(&route, reason).await;
                });
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
        if self.shutting_down.load(Ordering::Acquire) {
            return Err(command);
        }
        loop {
            let mut receiver = None;
            let lane = match self.lanes.entry(route.clone()) {
                Entry::Occupied(entry) => Arc::clone(entry.get()),
                Entry::Vacant(entry) => {
                    let (tx, rx) = mpsc::channel(ROUTE_LANE_CAPACITY);
                    let (stop_tx, _) = watch::channel(0);
                    let lane = Arc::new(RouteLane {
                        tx,
                        state: ParkingMutex::new(RouteLaneState {
                            accepting: true,
                            stop_generation: 0,
                        }),
                        stop_tx,
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
                    .execute_lane_command(
                        &route,
                        queued,
                        lane.stop_tx.subscribe(),
                        &mut shutdown_rx,
                    )
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
        mut stop_rx: watch::Receiver<u64>,
        shutdown_rx: &mut watch::Receiver<bool>,
    ) {
        match queued.command {
            LaneCommand::Prompt {
                reply_to,
                content_blocks,
                reply,
            } => {
                let result = tokio::select! {
                    biased;
                    _ = wait_for_stop(&mut stop_rx, queued.stop_generation) => {
                        let _ = self.workspace_threads.cancel_route(route).await;
                        cancelled_prompt_response()
                    }
                    _ = wait_for_shutdown(shutdown_rx) => {
                        Err(acp::Error::new(-32603, ROUTE_STOPPED_MESSAGE))
                    }
                    result = self.run_prompt(
                        ChannelTarget::new(route.clone(), reply_to),
                        content_blocks,
                    ) => result,
                };
                self.schedule_route_host_idle_shutdown(route).await;
                let _ = reply.send(result);
            }
            LaneCommand::Dispatch(input) => {
                let ran_prompt = matches!(
                    input.as_ref(),
                    ChannelInput::Message { .. } | ChannelInput::Callback { .. }
                );
                tokio::select! {
                    biased;
                    _ = wait_for_stop(&mut stop_rx, queued.stop_generation) => {
                        let _ = self.workspace_threads.cancel_route(route).await;
                        self.reject_stopped(route, (*input).clone());
                    }
                    _ = wait_for_shutdown(shutdown_rx) => {
                        self.reject_stopped(route, (*input).clone());
                    }
                    _ = self.dispatch_ordered((*input).clone()) => {}
                }
                if ran_prompt {
                    self.schedule_route_host_idle_shutdown(route).await;
                }
            }
            #[cfg(test)]
            LaneCommand::Probe { work, done } => {
                tokio::select! {
                    biased;
                    _ = wait_for_stop(&mut stop_rx, queued.stop_generation) => {}
                    _ = wait_for_shutdown(shutdown_rx) => {}
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
            self.shutting_down.store(true, Ordering::Release);
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
            ChannelInput::SwitchAgent { route, agent_kind } => {
                send_system_text(
                    &self.plugin_host,
                    &route,
                    &format!("Use /switch host {} with workspace threads.", agent_kind),
                );
            }
            ChannelInput::Stop { .. } | ChannelInput::Close { .. } | ChannelInput::Log { .. } => {}
        }
    }

    async fn run_prompt(
        &self,
        target: ChannelTarget,
        content_blocks: Vec<acp::ContentBlock>,
    ) -> acp::Result<acp::PromptResponse> {
        let result = handler::handle_prompt(
            &self.workspace_threads,
            &self.plugin_host,
            target.clone(),
            content_blocks,
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

        let target = ChannelTarget::new(route.clone(), message_id.clone());
        match self.run_prompt(target.clone(), content_blocks).await {
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

    fn reject_full_lane(&self, route: &RouteKey, input: ChannelInput) {
        let message_id = match input {
            ChannelInput::Message { envelope } | ChannelInput::Callback { envelope, .. } => {
                (!envelope.message_id.is_empty()).then_some(envelope.message_id)
            }
            _ => None,
        };
        let target = ChannelTarget::new(route.clone(), message_id.clone());
        send_system_text_to_target(&self.plugin_host, &target, ROUTE_LANE_FULL_MESSAGE);
        send_prompt_done(&self.plugin_host, route, message_id);
    }

    fn reject_stopped(&self, route: &RouteKey, input: ChannelInput) {
        let message_id = match input {
            ChannelInput::Message { envelope } | ChannelInput::Callback { envelope, .. } => {
                (!envelope.message_id.is_empty()).then_some(envelope.message_id)
            }
            _ => None,
        };
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

async fn wait_for_stop(stop_rx: &mut watch::Receiver<u64>, generation: u64) {
    while *stop_rx.borrow_and_update() == generation {
        if stop_rx.changed().await.is_err() {
            return;
        }
    }
}

async fn wait_for_shutdown(shutdown_rx: &mut watch::Receiver<bool>) {
    while !*shutdown_rx.borrow_and_update() {
        if shutdown_rx.changed().await.is_err() {
            return;
        }
    }
}

fn cancelled_prompt_response() -> acp::Result<acp::PromptResponse> {
    Ok(acp::PromptResponse::new(acp::StopReason::Cancelled))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_cancel_completes_the_prompt_with_cancelled_stop_reason() {
        let response = cancelled_prompt_response().expect("cancel is a successful ACP result");
        assert_eq!(response.stop_reason, acp::StopReason::Cancelled);
    }
}
