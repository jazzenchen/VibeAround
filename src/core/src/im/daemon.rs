//! Per-channel outbound: one FIFO queue and one send daemon task per channel.
//! Stream-edit: send then edit in place (all channels are streaming-only now).

use std::sync::Arc;
use std::time::Duration;

use dashmap::DashMap;
use tokio::sync::mpsc;
use tokio::time::Instant;

use super::log::{prefix, truncate_content_default};
use super::transport::{ImChannelCapabilities, ImTransport, InteractiveOption, SendError};

/// Outbound queue item.
#[derive(Debug, Clone)]
pub enum OutboundMsg {
    /// Progress label only (e.g. "Thinking...").
    StreamProgress(String, String),
    /// One text segment from agent.
    StreamPart(String, String),
    /// Single content update (edit channel). Throttled.
    StreamEdit(String, String),
    /// End of one content block: finalize stream. More blocks may follow.
    StreamEnd(String),
    /// End of the entire agent turn: finalize final block.
    StreamDone(String),
    /// Send as a new message. Rate-limited.
    Send(String, String),
    /// Send as a reply to a specific message.
    Reply(String, String, String), // channel_id, reply_to_message_id, text
    /// Add a reaction to a message.
    AddReaction(String, String, String), // channel_id, message_id, emoji
    /// Remove a reaction from a message.
    RemoveReaction(String, String, String), // channel_id, message_id, reaction_id
    /// Set the reply_to message_id (first flushed message quotes the user).
    SetReplyTo(String, String), // channel_id, reply_to_message_id
    /// Send an interactive card/inline keyboard.
    SendInteractive {
        channel_id: String,
        prompt: String,
        options: Vec<InteractiveOption>,
        reply_to: Option<String>,
    },
    /// Update an existing interactive card in place.
    UpdateInteractive {
        channel_id: String,
        message_id: String,
        prompt: String,
        options: Vec<InteractiveOption>,
    },
}

/// Per-channel state.
struct ChannelSendState {
    last_send: Option<Instant>,
    retry_after: Option<Instant>,
    /// Whether the initial stream message has been sent.
    stream_sent: bool,
    /// The message_id of the initial stream message (for edit_message).
    stream_message_id: Option<String>,
    last_edit: Option<Instant>,
    /// Accumulated text from StreamPart; shown in place via edit_message.
    pending_stream_text: Option<String>,
    /// Last progress label (Thinking...) when no text yet.
    last_progress: Option<String>,
    /// The reply_to message_id for the first reply.
    reply_to_message_id: Option<String>,
    /// Last reaction_id returned by add_reaction.
    last_reaction_id: Option<String>,
    /// The message_id that the last reaction was added to.
    last_reaction_message_id: Option<String>,
}

const MIN_INTERVAL: Duration = Duration::from_secs(1);

/// One send daemon for a single channel: drains FIFO queue, applies rate limit and stream-edit.
async fn run_send_daemon_for_channel<T>(
    mut rx: mpsc::Receiver<OutboundMsg>,
    tx: mpsc::Sender<OutboundMsg>,
    channel_id: String,
    transport: Arc<T>,
) where T: ImTransport {
    let caps = transport.capabilities();
    let mut state = ChannelSendState {
        last_send: None, retry_after: None,
        stream_sent: false, stream_message_id: None, last_edit: None,
        pending_stream_text: None, last_progress: None,
        reply_to_message_id: None,
        last_reaction_id: None, last_reaction_message_id: None,
    };

    while let Some(msg) = rx.recv().await {
        match msg {
            OutboundMsg::StreamProgress(_, label) => {
                state.last_progress = Some(label.clone());
                let to_show = state.pending_stream_text.as_deref().unwrap_or(&label);
                let now = Instant::now();
                let can_edit = state.last_edit.map(|t| now.duration_since(t) >= caps.min_edit_interval).unwrap_or(true);
                if !state.stream_sent {
                    if let Ok(mid) = transport.send(&channel_id, to_show).await {
                        state.stream_sent = true;
                        state.stream_message_id = mid;
                        state.last_edit = Some(Instant::now());
                    }
                } else if let Some(ref mid) = state.stream_message_id {
                    if can_edit {
                        let _ = transport.edit_message(&channel_id, mid, to_show).await;
                        state.last_edit = Some(Instant::now());
                    }
                }
            }
            OutboundMsg::StreamPart(_, text) => {
                state.last_progress = None;
                let acc = state.pending_stream_text.take().unwrap_or_default();
                state.pending_stream_text = Some(acc + &text);
                let to_show = state.pending_stream_text.as_deref().unwrap_or("…");
                let now = Instant::now();
                let can_edit = state.last_edit.map(|t| now.duration_since(t) >= caps.min_edit_interval).unwrap_or(true);
                if !state.stream_sent {
                    if let Ok(mid) = transport.send(&channel_id, to_show).await {
                        state.stream_sent = true;
                        state.stream_message_id = mid;
                        state.last_edit = Some(Instant::now());
                    }
                } else if let Some(ref mid) = state.stream_message_id {
                    if can_edit {
                        let _ = transport.edit_message(&channel_id, mid, to_show).await;
                        state.last_edit = Some(Instant::now());
                    }
                }
            }
            OutboundMsg::StreamEdit(_, text) => {
                if let Some(ref mid) = state.stream_message_id {
                    let now = Instant::now();
                    let can_edit = state.last_edit.map(|t| now.duration_since(t) >= caps.min_edit_interval).unwrap_or(true);
                    if can_edit {
                        let to_show = state.pending_stream_text.take().unwrap_or_else(|| text.clone());
                        match transport.edit_message(&channel_id, mid, &to_show).await {
                            Ok(()) => state.last_edit = Some(Instant::now()),
                            Err(SendError::RateLimited { retry_after_secs }) => {
                                state.retry_after = Some(Instant::now() + Duration::from_secs_f64(retry_after_secs));
                                state.pending_stream_text = Some(text.clone());
                                let _ = tx.send(OutboundMsg::StreamEdit(channel_id.clone(), text)).await;
                            }
                            Err(SendError::Other(e)) => {
                                eprintln!("{} chat_id={} message_id={} direction=edit error={}", prefix(&channel_id), channel_id, mid, e);
                                state.pending_stream_text = Some(text);
                            }
                        }
                    } else {
                        state.pending_stream_text = Some(text);
                    }
                } else if state.stream_sent {
                    state.pending_stream_text = Some(text);
                } else {
                    match transport.send(&channel_id, &text).await {
                        Ok(mid) => {
                            state.stream_sent = true;
                            state.stream_message_id = mid;
                            state.last_edit = Some(Instant::now());
                            state.pending_stream_text = None;
                        }
                        Err(SendError::RateLimited { retry_after_secs }) => {
                            state.retry_after = Some(Instant::now() + Duration::from_secs_f64(retry_after_secs));
                            let _ = tx.send(OutboundMsg::StreamEdit(channel_id.clone(), text)).await;
                        }
                        Err(SendError::Other(e)) => {
                            eprintln!("{} chat_id={} direction=send content={} error={}", prefix(&channel_id), channel_id, truncate_content_default(&text), e);
                        }
                    }
                }
            }
            OutboundMsg::StreamEnd(_) => {
                // Block boundary — finalize current message, reset state for next block.
                let mid = state.stream_message_id.take();
                let pending = state.pending_stream_text.take();
                state.last_progress = None;
                state.stream_sent = false;
                if let (Some(mid), Some(pending)) = (mid, pending) {
                    let _ = transport.edit_message(&channel_id, &mid, &pending).await;
                    let _ = transport.finalize_stream(&channel_id, &mid, &pending).await;
                }
                // Send a "Working..." placeholder for the next block
                if let Ok(mid) = transport.send(&channel_id, "⏳ Working...").await {
                    state.stream_sent = true;
                    state.stream_message_id = mid;
                    state.last_edit = Some(Instant::now());
                    state.pending_stream_text = Some("⏳ Working...".to_string());
                }
            }
            OutboundMsg::StreamDone(_) => {
                // Entire turn is done. Nothing to do — the last StreamEnd already
                // finalized content and sent a "Working..." which will be naturally
                // replaced by the next turn's first StreamPart.
                state.stream_message_id = None;
                state.pending_stream_text = None;
                state.last_progress = None;
                state.stream_sent = false;
            }
            OutboundMsg::Send(_, text) => {
                state.stream_message_id = None;
                state.stream_sent = false;
                state.pending_stream_text = None;
                state.last_progress = None;
                let now = Instant::now();
                let wait_until = state.retry_after.filter(|t| *t > now).or_else(|| {
                    state.last_send.and_then(|t| {
                        let next = t + MIN_INTERVAL;
                        if next > now { Some(next) } else { None }
                    })
                });
                if let Some(until) = wait_until {
                    tokio::time::sleep_until(until).await;
                    let _ = tx.send(OutboundMsg::Send(channel_id.clone(), text)).await;
                    continue;
                }
                match transport.send(channel_id.as_str(), &text).await {
                    Ok(_) => { state.last_send = Some(Instant::now()); state.retry_after = None; }
                    Err(SendError::RateLimited { retry_after_secs }) => {
                        state.retry_after = Some(Instant::now() + Duration::from_secs_f64(retry_after_secs));
                        let _ = tx.send(OutboundMsg::Send(channel_id.clone(), text)).await;
                    }
                    Err(SendError::Other(e)) => {
                        eprintln!("{} chat_id={} direction=send content={} error={}", prefix(&channel_id), channel_id, truncate_content_default(&text), e);
                    }
                }
            }
            OutboundMsg::Reply(_, reply_to, text) => {
                let now = Instant::now();
                let wait_until = state.retry_after.filter(|t| *t > now).or_else(|| {
                    state.last_send.and_then(|t| {
                        let next = t + MIN_INTERVAL;
                        if next > now { Some(next) } else { None }
                    })
                });
                if let Some(until) = wait_until { tokio::time::sleep_until(until).await; }
                match transport.reply(&channel_id, &reply_to, &text).await {
                    Ok(_) => { state.last_send = Some(Instant::now()); state.retry_after = None; }
                    Err(SendError::RateLimited { retry_after_secs }) => {
                        state.retry_after = Some(Instant::now() + Duration::from_secs_f64(retry_after_secs));
                        let _ = tx.send(OutboundMsg::Reply(channel_id.clone(), reply_to, text)).await;
                    }
                    Err(SendError::Other(e)) => {
                        eprintln!("{} chat_id={} direction=reply error={}", prefix(&channel_id), channel_id, e);
                    }
                }
            }
            OutboundMsg::AddReaction(_, message_id, emoji) => {
                match transport.add_reaction(&channel_id, &message_id, &emoji).await {
                    Ok(Some(rid)) => {
                        state.last_reaction_id = Some(rid);
                        state.last_reaction_message_id = Some(message_id);
                    }
                    Ok(None) => {
                        state.last_reaction_id = Some(emoji.clone());
                        state.last_reaction_message_id = Some(message_id);
                    }
                    Err(e) => {
                        eprintln!("{} chat_id={} direction=add_reaction error={:?}", prefix(&channel_id), channel_id, e);
                    }
                }
            }
            OutboundMsg::RemoveReaction(_, message_id, _reaction_id_hint) => {
                let (mid, rid) = if state.last_reaction_message_id.as_deref() == Some(&message_id) {
                    if let Some(rid) = state.last_reaction_id.take() {
                        state.last_reaction_message_id = None;
                        (message_id, rid)
                    } else {
                        (message_id, _reaction_id_hint)
                    }
                } else {
                    (message_id, _reaction_id_hint)
                };
                if let Err(e) = transport.remove_reaction(&channel_id, &mid, &rid).await {
                    eprintln!("{} chat_id={} direction=remove_reaction error={:?}", prefix(&channel_id), channel_id, e);
                }
            }
            OutboundMsg::SetReplyTo(_, reply_to) => {
                state.reply_to_message_id = Some(reply_to);
            }
            OutboundMsg::SendInteractive { prompt, options, reply_to, .. } => {
                let now = Instant::now();
                let wait_until = state.retry_after.filter(|t| *t > now).or_else(|| {
                    state.last_send.and_then(|t| {
                        let next = t + MIN_INTERVAL;
                        if next > now { Some(next) } else { None }
                    })
                });
                if let Some(until) = wait_until { tokio::time::sleep_until(until).await; }
                match transport.send_interactive(&channel_id, &prompt, &options, reply_to.as_deref()).await {
                    Ok(_) => { state.last_send = Some(Instant::now()); }
                    Err(e) => {
                        eprintln!("{} chat_id={} direction=send_interactive error={:?}", prefix(&channel_id), channel_id, e);
                    }
                }
            }
            OutboundMsg::UpdateInteractive { message_id, prompt, options, .. } => {
                match transport.update_interactive(&channel_id, &message_id, &prompt, &options).await {
                    Ok(()) => {}
                    Err(e) => {
                        eprintln!("{} chat_id={} direction=update_interactive error={:?}", prefix(&channel_id), channel_id, e);
                    }
                }
            }
        }
    }
}

/// Hub that routes outbound messages to per-channel FIFO queues with dedicated daemon tasks.
pub struct OutboundHub<T> {
    channels: DashMap<String, mpsc::Sender<OutboundMsg>>,
    transport: Arc<T>,
}

impl<T> OutboundHub<T>
where T: ImTransport + 'static,
{
    pub fn new(transport: Arc<T>) -> Arc<Self> {
        Arc::new(Self { channels: DashMap::new(), transport })
    }

    /// Get or create the sender for this channel.
    pub fn sender_for(&self, channel_id: &str) -> mpsc::Sender<OutboundMsg> {
        self.channels.entry(channel_id.to_string()).or_insert_with(|| {
            let channel_id = channel_id.to_string();
            let (tx, rx) = mpsc::channel::<OutboundMsg>(256);
            let transport = Arc::clone(&self.transport);
            tokio::spawn(run_send_daemon_for_channel(rx, tx.clone(), channel_id, transport));
            tx
        }).clone()
    }

    pub async fn send(&self, channel_id: &str, msg: OutboundMsg) {
        let tx = self.sender_for(channel_id);
        let _ = tx.send(msg).await;
    }

    pub async fn set_reply_to(&self, channel_id: &str, reply_to_message_id: String) {
        let tx = self.sender_for(channel_id);
        let _ = tx.send(OutboundMsg::SetReplyTo(channel_id.to_string(), reply_to_message_id)).await;
    }

    pub fn max_message_len(&self) -> usize {
        self.transport.max_message_len()
    }

    pub fn capabilities(&self) -> ImChannelCapabilities {
        self.transport.capabilities()
    }

    /// Send a message directly (bypassing the queue) and return the message_id.
    pub async fn send_direct(&self, channel_id: &str, text: &str) -> Option<String> {
        self.transport.send(channel_id, text).await.ok().flatten()
    }

    /// Send an interactive card directly (bypassing queue). Returns message_id if available.
    pub async fn send_interactive_direct(
        &self,
        channel_id: &str,
        prompt: &str,
        options: &[crate::im::transport::InteractiveOption],
        reply_to: Option<&str>,
    ) -> Option<String> {
        self.transport.send_interactive(channel_id, prompt, options, reply_to).await.ok().flatten()
    }

    /// Update an existing interactive card directly (bypassing queue).
    pub async fn update_interactive_direct(
        &self,
        channel_id: &str,
        message_id: &str,
        prompt: &str,
        options: &[crate::im::transport::InteractiveOption],
    ) {
        let _ = self.transport.update_interactive(channel_id, message_id, prompt, options).await;
    }

    /// Edit a message directly (bypassing the queue).
    pub async fn edit_direct(&self, channel_id: &str, message_id: &str, text: &str) {
        let _ = self.transport.edit_message(channel_id, message_id, text).await;
    }
}
