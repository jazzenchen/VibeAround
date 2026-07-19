use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use agent_client_protocol::schema::v1 as acp;
use tokio::sync::{mpsc, oneshot, watch};

use crate::routing::wait_for_signal;
use crate::routing::ChannelTarget;
use crate::workspace::WorkspaceThreadManager;

use super::{
    auto_close_reason_for_prompt_error, effective_input_text, envelope_content_blocks, handler,
    send_system_text, send_system_text_to_target, send_turn_status, ChannelEnvelope, ChannelInput,
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
}

impl OrderedInput {
    fn route_key(&self) -> &RouteKey {
        match self {
            Self::Message { envelope } | Self::Callback { envelope, .. } => &envelope.route,
        }
    }

    fn reply_to(&self) -> Option<String> {
        match self {
            Self::Message { envelope } | Self::Callback { envelope, .. } => envelope.reply_to(),
        }
    }
}

struct QueuedCommand {
    sequence: u64,
    cancellation: watch::Receiver<bool>,
    command: LaneCommand,
}

struct RouteLane {
    id: u64,
    last_sequence: u64,
    tx: mpsc::Sender<QueuedCommand>,
    cancel_tx: watch::Sender<bool>,
}

enum IngressCommand {
    Enqueue {
        route: RouteKey,
        command: LaneCommand,
        accepted: Option<oneshot::Sender<bool>>,
    },
    Stop(RouteKey),
    LaneCompleted {
        route: RouteKey,
        lane_id: u64,
        sequence: u64,
    },
    LaneExited(u64),
    Shutdown(oneshot::Sender<()>),
    #[cfg(test)]
    LaneCount(oneshot::Sender<usize>),
}

struct IngressOwner {
    lanes: HashMap<RouteKey, RouteLane>,
    active_lane_ids: HashSet<u64>,
    next_lane_id: u64,
    shutting_down: bool,
    shutdown_waiters: Vec<oneshot::Sender<()>>,
}

/// Concrete conversation entry point shared by every channel transport.
///
pub struct ConversationIngress {
    workspace_threads: Arc<WorkspaceThreadManager>,
    plugin_host: Arc<PluginHost>,
    command_tx: mpsc::UnboundedSender<IngressCommand>,
}

impl IngressOwner {
    fn enqueue(
        &mut self,
        ingress: &Arc<ConversationIngress>,
        route: RouteKey,
        command: LaneCommand,
    ) -> bool {
        if self.shutting_down {
            ingress.reject_lane_command(&route, command, true);
            return false;
        }

        let new_lane = !self.lanes.contains_key(&route);
        if new_lane {
            let lane_id = self.next_lane_id;
            self.next_lane_id = self.next_lane_id.wrapping_add(1);
            let (tx, rx) = mpsc::channel(ROUTE_LANE_CAPACITY);
            let (cancel_tx, _) = watch::channel(false);
            self.lanes.insert(
                route.clone(),
                RouteLane {
                    id: lane_id,
                    last_sequence: 0,
                    tx,
                    cancel_tx,
                },
            );
            self.active_lane_ids.insert(lane_id);
            ingress.spawn_lane(route.clone(), lane_id, rx);
        }

        let lane = self.lanes.get_mut(&route).expect("lane was just created");
        let sequence = lane.last_sequence.wrapping_add(1);
        let queued = QueuedCommand {
            sequence,
            cancellation: lane.cancel_tx.subscribe(),
            command,
        };
        match lane.tx.try_send(queued) {
            Ok(()) => {
                lane.last_sequence = sequence;
                if new_lane {
                    send_turn_status(&ingress.plugin_host, &route, true);
                }
                true
            }
            Err(mpsc::error::TrySendError::Full(queued)) => {
                ingress.reject_lane_command(&route, queued.command, false);
                false
            }
            Err(mpsc::error::TrySendError::Closed(queued)) => {
                if self.lanes.remove(&route).is_some() {
                    send_turn_status(&ingress.plugin_host, &route, false);
                }
                ingress.reject_lane_command(&route, queued.command, true);
                false
            }
        }
    }

    fn stop(&mut self, ingress: &Arc<ConversationIngress>, route: RouteKey) {
        if let Some(lane) = self.lanes.get_mut(&route) {
            lane.cancel_tx.send_replace(true);
            lane.cancel_tx = watch::channel(false).0;
            return;
        }
        let workspace_threads = Arc::clone(&ingress.workspace_threads);
        tokio::spawn(async move {
            let _ = workspace_threads.cancel_route(&route).await;
        });
    }

    fn lane_completed(
        &mut self,
        ingress: &ConversationIngress,
        route: &RouteKey,
        lane_id: u64,
        sequence: u64,
    ) {
        let remove = self
            .lanes
            .get(route)
            .is_some_and(|lane| lane.id == lane_id && lane.last_sequence == sequence);
        if remove {
            self.lanes.remove(route);
            send_turn_status(&ingress.plugin_host, route, false);
        }
    }

    fn lane_exited(&mut self, lane_id: u64) {
        self.active_lane_ids.remove(&lane_id);
        if self.shutting_down && self.active_lane_ids.is_empty() {
            for waiter in self.shutdown_waiters.drain(..) {
                let _ = waiter.send(());
            }
        }
    }

    fn shutdown(&mut self, reply: oneshot::Sender<()>) {
        if !self.shutting_down {
            self.shutting_down = true;
            for lane in self.lanes.values() {
                lane.cancel_tx.send_replace(true);
            }
            self.lanes.clear();
        }
        if self.active_lane_ids.is_empty() {
            let _ = reply.send(());
        } else {
            self.shutdown_waiters.push(reply);
        }
    }
}

impl ConversationIngress {
    pub(crate) fn new(
        workspace_threads: Arc<WorkspaceThreadManager>,
        plugin_host: Arc<PluginHost>,
    ) -> Arc<Self> {
        let (command_tx, command_rx) = mpsc::unbounded_channel();
        let ingress = Arc::new(Self {
            workspace_threads,
            plugin_host,
            command_tx,
        });
        tokio::spawn(Self::run_owner(Arc::downgrade(&ingress), command_rx));
        ingress
    }

    /// Run one prompt to completion and return its actual ACP stop reason.
    pub async fn prompt(
        self: &Arc<Self>,
        target: ChannelTarget,
        content_blocks: Vec<acp::ContentBlock>,
    ) -> acp::Result<acp::PromptResponse> {
        let (reply, response) = oneshot::channel();
        if self
            .command_tx
            .send(IngressCommand::Enqueue {
                route: target.route.clone(),
                command: LaneCommand::Prompt {
                    reply_to: target.reply_to,
                    content_blocks,
                    reply,
                },
                accepted: None,
            })
            .is_err()
        {
            return Err(acp::Error::new(-32000, ROUTE_STOPPED_MESSAGE));
        };
        response
            .await
            .unwrap_or_else(|_| Err(acp::Error::new(-32603, "conversation route stopped")))
    }

    /// Dispatch a channel command. Stop, Close, and log records bypass route queues;
    /// every other command is accepted into the route's bounded FIFO lane.
    pub fn dispatch(self: &Arc<Self>, input: ChannelInput) {
        let input = match input {
            ChannelInput::Stop { route } => {
                if self
                    .command_tx
                    .send(IngressCommand::Stop(route.clone()))
                    .is_err()
                {
                    let workspace_threads = Arc::clone(&self.workspace_threads);
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
            ChannelInput::SwitchAgent { route, agent_kind } => {
                send_system_text(
                    &self.plugin_host,
                    &route,
                    &format!("Use /switch host {} with workspace threads.", agent_kind),
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
        };

        let route = input.route_key().clone();
        let command = IngressCommand::Enqueue {
            route: route.clone(),
            command: LaneCommand::Dispatch(Box::new(input)),
            accepted: None,
        };
        if let Err(error) = self.command_tx.send(command) {
            if let IngressCommand::Enqueue {
                command: LaneCommand::Dispatch(rejected),
                ..
            } = error.0
            {
                self.reject_stopped(&route, *rejected);
            }
        }
    }

    async fn run_owner(
        ingress: std::sync::Weak<Self>,
        mut command_rx: mpsc::UnboundedReceiver<IngressCommand>,
    ) {
        let mut owner = IngressOwner {
            lanes: HashMap::new(),
            active_lane_ids: HashSet::new(),
            next_lane_id: 1,
            shutting_down: false,
            shutdown_waiters: Vec::new(),
        };
        while let Some(command) = command_rx.recv().await {
            match command {
                IngressCommand::Enqueue {
                    route,
                    command,
                    accepted,
                } => {
                    let Some(ingress) = ingress.upgrade() else {
                        break;
                    };
                    let was_accepted = owner.enqueue(&ingress, route, command);
                    if let Some(accepted) = accepted {
                        let _ = accepted.send(was_accepted);
                    }
                }
                IngressCommand::Stop(route) => {
                    let Some(ingress) = ingress.upgrade() else {
                        break;
                    };
                    owner.stop(&ingress, route);
                }
                IngressCommand::LaneCompleted {
                    route,
                    lane_id,
                    sequence,
                } => {
                    let Some(ingress) = ingress.upgrade() else {
                        break;
                    };
                    owner.lane_completed(&ingress, &route, lane_id, sequence);
                }
                IngressCommand::LaneExited(lane_id) => owner.lane_exited(lane_id),
                IngressCommand::Shutdown(reply) => {
                    owner.shutdown(reply);
                }
                #[cfg(test)]
                IngressCommand::LaneCount(reply) => {
                    let _ = reply.send(owner.lanes.len());
                }
            }
        }
    }

    fn spawn_lane(
        self: &Arc<Self>,
        route: RouteKey,
        lane_id: u64,
        mut rx: mpsc::Receiver<QueuedCommand>,
    ) {
        let ingress = Arc::clone(self);
        tokio::spawn(async move {
            while let Some(queued) = rx.recv().await {
                let sequence = queued.sequence;
                ingress.execute_lane_command(&route, queued).await;
                let _ = ingress.command_tx.send(IngressCommand::LaneCompleted {
                    route: route.clone(),
                    lane_id,
                    sequence,
                });
            }
            let _ = ingress.command_tx.send(IngressCommand::LaneExited(lane_id));
        });
    }

    async fn execute_lane_command(&self, route: &RouteKey, queued: QueuedCommand) {
        let mut cancellation = queued.cancellation;
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
                    cancellation.clone(),
                );
                tokio::pin!(prompt);
                let result = tokio::select! {
                    biased;
                    _ = wait_for_signal(&mut cancellation) => {
                        let _ = self.workspace_threads.cancel_route(route).await;
                        prompt.await
                    }
                    result = &mut prompt => result,
                };
                let _ = reply.send(result);
            }
            LaneCommand::Dispatch(input) => {
                if *cancellation.borrow() {
                    self.reject_stopped(route, *input);
                    return;
                }
                let dispatch = self.dispatch_ordered((*input).clone(), cancellation.clone());
                tokio::pin!(dispatch);
                tokio::select! {
                    biased;
                    _ = wait_for_signal(&mut cancellation) => {
                        let _ = self.workspace_threads.cancel_route(route).await;
                        dispatch.await;
                    }
                    _ = &mut dispatch => {}
                }
            }
            #[cfg(test)]
            LaneCommand::Probe { work, done } => {
                tokio::select! {
                    biased;
                    _ = wait_for_signal(&mut cancellation) => {}
                    _ = work => { let _ = done.send(()); }
                }
            }
        }
    }

    /// Stop accepting new work, cancel every active/queued lane command, and
    /// wait until all lane tasks have released their references.
    pub async fn shutdown(&self) {
        let (reply, done) = oneshot::channel();
        if self
            .command_tx
            .send(IngressCommand::Shutdown(reply))
            .is_ok()
        {
            let _ = done.await;
        }
    }

    async fn dispatch_ordered(&self, input: OrderedInput, cancellation: watch::Receiver<bool>) {
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
        }
    }

    async fn run_prompt(
        &self,
        target: ChannelTarget,
        content_blocks: Vec<acp::ContentBlock>,
        cancellation: watch::Receiver<bool>,
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

    async fn handle_prompt_input(
        &self,
        envelope: ChannelEnvelope,
        action_value: Option<String>,
        cancellation: watch::Receiver<bool>,
    ) {
        let route = envelope.route.clone();
        let cli_kind = envelope.cli_kind.clone();
        let text = effective_input_text(&envelope, action_value);
        let message_id = envelope.reply_to();
        tracing::debug!(
            route = %route,
            cli_kind = ?cli_kind,
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
    }

    fn reject_full_lane(&self, route: &RouteKey, input: OrderedInput) {
        let message_id = input.reply_to();
        let target = ChannelTarget::new(route.clone(), message_id.clone());
        send_system_text_to_target(&self.plugin_host, &target, ROUTE_LANE_FULL_MESSAGE);
    }

    fn reject_stopped(&self, _route: &RouteKey, _input: OrderedInput) {}

    fn reject_lane_command(&self, route: &RouteKey, command: LaneCommand, stopped: bool) {
        match command {
            LaneCommand::Prompt { reply, .. } => {
                let message = if stopped {
                    ROUTE_STOPPED_MESSAGE
                } else {
                    ROUTE_LANE_FULL_MESSAGE
                };
                let _ = reply.send(Err(acp::Error::new(-32000, message)));
            }
            LaneCommand::Dispatch(input) => {
                if stopped {
                    self.reject_stopped(route, *input);
                } else {
                    self.reject_full_lane(route, *input);
                }
            }
            #[cfg(test)]
            LaneCommand::Probe { .. } => {}
        }
    }

    #[cfg(test)]
    pub(super) async fn enqueue_probe<F>(
        self: &Arc<Self>,
        route: RouteKey,
        work: F,
    ) -> Result<oneshot::Receiver<()>, ()>
    where
        F: std::future::Future<Output = ()> + Send + 'static,
    {
        let (done, completed) = oneshot::channel();
        let (accepted, acceptance) = oneshot::channel();
        self.command_tx
            .send(IngressCommand::Enqueue {
                route,
                command: LaneCommand::Probe {
                    work: Box::pin(work),
                    done,
                },
                accepted: Some(accepted),
            })
            .map_err(|_| ())?;
        if acceptance.await.unwrap_or(false) {
            Ok(completed)
        } else {
            Err(())
        }
    }

    #[cfg(test)]
    pub(super) async fn active_lane_count(&self) -> usize {
        let (reply, count) = oneshot::channel();
        if self
            .command_tx
            .send(IngressCommand::LaneCount(reply))
            .is_err()
        {
            return 0;
        }
        count.await.unwrap_or(0)
    }
}

fn cancelled_prompt_response() -> acp::Result<acp::PromptResponse> {
    Ok(acp::PromptResponse::new(acp::StopReason::Cancelled))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn closed_lane_is_removed_and_emits_idle_once() {
        let base = std::env::temp_dir().join(format!(
            "vibearound-closed-route-lane-{}",
            std::process::id()
        ));
        let workspace_threads = WorkspaceThreadManager::with_paths(
            base.join("workspaces.jsonl"),
            base.join("threads.jsonl"),
            base.join("attachments.jsonl"),
        );
        let (input_tx, _input_rx) = mpsc::unbounded_channel();
        let plugin_host = Arc::new(PluginHost::new(input_tx));
        let (output_tx, mut output_rx) = mpsc::unbounded_channel();
        plugin_host.register_websocket_plugin("web", output_tx);
        let ingress = ConversationIngress::new(workspace_threads, plugin_host);
        let route = RouteKey::new("web", "chat-a");
        let (lane_tx, lane_rx) = mpsc::channel(1);
        drop(lane_rx);
        let (cancel_tx, _) = watch::channel(false);
        let mut owner = IngressOwner {
            lanes: HashMap::from([(
                route.clone(),
                RouteLane {
                    id: 1,
                    last_sequence: 0,
                    tx: lane_tx,
                    cancel_tx,
                },
            )]),
            active_lane_ids: HashSet::new(),
            next_lane_id: 2,
            shutting_down: false,
            shutdown_waiters: Vec::new(),
        };
        let (done, _) = oneshot::channel();

        assert!(!owner.enqueue(
            &ingress,
            route.clone(),
            LaneCommand::Probe {
                work: Box::pin(async {}),
                done,
            },
        ));
        assert!(!owner.lanes.contains_key(&route));
        assert_eq!(
            output_rx.recv().await.unwrap(),
            super::super::ChannelOutput::TurnStatus {
                route,
                active: false,
            }
        );
        assert!(output_rx.try_recv().is_err());
    }

    #[test]
    fn session_cancel_completes_the_prompt_with_cancelled_stop_reason() {
        let response = cancelled_prompt_response().expect("cancel is a successful ACP result");
        assert_eq!(response.stop_reason, acp::StopReason::Cancelled);
    }
}
