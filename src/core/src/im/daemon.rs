//! Per-channel outbound: one FIFO queue and one send daemon task per channel.
//! Stream-edit (send then edit in place), buffer (accumulate then send on end), or passthrough (web chat).

use std::collections::VecDeque;
use std::sync::Arc;
use std::time::Duration;

use dashmap::DashMap;
use tokio::sync::mpsc;
use tokio::time::Instant;

use super::log::{prefix, truncate_content_default};
use super::transport::{ImChannelCapabilities, ImTransport, InteractiveOption, SendError};
use crate::im::transport;

/// Outbound queue item.
#[derive(Debug, Clone)]
pub enum OutboundMsg {
    /// Progress label only (e.g. "Thinking...").
    StreamProgress(String, String),
    /// One text segment from agent.
    StreamPart(String, String),
    /// Legacy: single content update (edit channel). Throttled.
    StreamEdit(String, String),
    /// End of one content block: flush buffer. More blocks may follow.
    StreamEnd(String),
    /// End of the entire agent turn: flush final block, remove processing reaction, add done reaction.
    StreamDone(String),
    /// Send as a new message. Rate-limited.
    Send(String, String),
    /// Send as a reply to a specific message.
    Reply(String, String, String), // channel_id, reply_to_message_id, text
    /// Add a reaction to a message.
    AddReaction(String, String, String), // channel_id, message_id, emoji
    /// Remove a reaction from a message.
    RemoveReaction(String, String, String), // channel_id, message_id, reaction_id
    /// Set the reply_to message_id for buffer mode (first flushed message quotes the user).
    SetReplyTo(String, String), // channel_id, reply_to_message_id
    /// Send an interactive card/inline keyboard.
    SendInteractive {
        channel_id: String,
        prompt: String,
        options: Vec<InteractiveOption>,
        reply_to: Option<String>,
    },
}

/// Per-channel state.
struct ChannelSendState {
    last_send: Option<Instant>,
    retry_after: Option<Instant>,
    /// Whether the initial stream message has been sent (regardless of whether we got a message_id).
    stream_sent: bool,
    /// The message_id of the initial stream message (for edit_message). None if platform didn't return one.
    stream_message_id: Option<String>,
    last_edit: Option<Instant>,
    /// Edit path: accumulated text from StreamPart; shown in place. Progress labels (StreamProgress) shown only when this is empty.
    pending_stream_text: Option<String>,
    /// Edit path: last progress label (Thinking... / Using tool: X...) when no text yet.
    last_progress: Option<String>,
    /// For !supports_stream_edit: FIFO of segments to send. Each sent with rate limit; if part > max_len, split into multiple messages.
    response_parts: VecDeque<String>,
    /// For buffer mode: accumulated text blocks, sent as complete messages on StreamEnd.
    buffer_parts: Vec<String>,
    /// For buffer mode: the reply_to message_id for the first reply.
    reply_to_message_id: Option<String>,
    /// Last reaction_id returned by add_reaction (for remove_reaction).
    last_reaction_id: Option<String>,
    /// The message_id that the last reaction was added to.
    last_reaction_message_id: Option<String>,
    /// The message_id of the last bot message sent (for post-turn done reaction).
    last_bot_message_id: Option<String>,
}

const MIN_INTERVAL: Duration = Duration::from_secs(1);
const MIN_EDIT_INTERVAL: Duration = Duration::from_secs(2);

/// For no-edit: wait for rate limit, then send one part from response_parts (split if over max_len). Returns true if sent one.
async fn drain_one_response_part<T: ImTransport>(
    channel_id: &str,
    state: &mut ChannelSendState,
    transport: &Arc<T>,
    max_len: usize,
) -> bool {
    let part = match state.response_parts.pop_front() {
        Some(p) => p,
        None => return false,
    };
    let now = Instant::now();
    let wait_until = state
        .retry_after
        .filter(|t| *t > now)
        .or_else(|| {
            state.last_send.and_then(|t| {
                let next = t + MIN_INTERVAL;
                if next > now {
                    Some(next)
                } else {
                    None
                }
            })
        });
    if let Some(until) = wait_until {
        tokio::time::sleep_until(until).await;
    }
    let chunks = if part.len() <= max_len {
        vec![part]
    } else {
        transport::chunk_message(&part, max_len)
    };
    for (i, chunk) in chunks.iter().enumerate() {
        match transport.send(channel_id, chunk).await {
            Ok(_) => {
                state.last_send = Some(Instant::now());
                state.retry_after = None;
            }
            Err(SendError::RateLimited { retry_after_secs }) => {
                state.retry_after =
                    Some(Instant::now() + Duration::from_secs_f64(retry_after_secs));
                for j in (i..chunks.len()).rev() {
                    state.response_parts.push_front(chunks[j].clone());
                }
                return true;
            }
            Err(SendError::Other(e)) => {
                eprintln!(
                    "{} chat_id={} direction=send part content={} error={}",
                    prefix(channel_id),
                    channel_id,
                    truncate_content_default(&chunk),
                    e
                );
                return true;
            }
        }
        if i + 1 < chunks.len() {
            let now = Instant::now();
            tokio::time::sleep_until(now + MIN_INTERVAL).await;
        }
    }
    true
}

/// Flush buffered content: join all buffer_parts, chunk, and send (first as reply if reply_to set).
/// Returns the message_id of the last sent message (if any).
/// When `is_final` is true, the done reaction is added directly (skipping the intermediate processing reaction).
async fn flush_buffer<T: ImTransport>(
    channel_id: &str,
    state: &mut ChannelSendState,
    transport: &Arc<T>,
    caps: &ImChannelCapabilities,
    max_len: usize,
    is_final: bool,
) -> Option<String> {
    if state.buffer_parts.is_empty() {
        return None;
    }
    let full_text = state.buffer_parts.drain(..).collect::<String>();
    if full_text.trim().is_empty() {
        return None;
    }
    let chunks = transport::chunk_message(&full_text, max_len);
    let mut last_sent_id: Option<String> = None;
    for (i, chunk) in chunks.iter().enumerate() {
        // Rate limit
        let now = Instant::now();
        let wait_until = state
            .retry_after
            .filter(|t| *t > now)
            .or_else(|| {
                state.last_send.and_then(|t| {
                    let next = t + MIN_INTERVAL;
                    if next > now { Some(next) } else { None }
                })
            });
        if let Some(until) = wait_until {
            tokio::time::sleep_until(until).await;
        }
        // First chunk: reply to user message if available
        let result = if i == 0 {
            if let Some(ref reply_to) = state.reply_to_message_id {
                transport.reply(channel_id, reply_to, chunk).await
            } else {
                transport.send(channel_id, chunk).await
            }
        } else {
            transport.send(channel_id, chunk).await
        };
        match result {
            Ok(mid) => {
                state.last_send = Some(Instant::now());
                state.retry_after = None;
                last_sent_id = mid;
            }
            Err(SendError::RateLimited { retry_after_secs }) => {
                state.retry_after = Some(Instant::now() + Duration::from_secs_f64(retry_after_secs));
                // Re-queue remaining chunks
                for j in (i + 1..chunks.len()).rev() {
                    state.response_parts.push_front(chunks[j].clone());
                }
                break;
            }
            Err(SendError::Other(e)) => {
                eprintln!("{} chat_id={} direction=send buffer content={} error={}", prefix(channel_id), channel_id, truncate_content_default(chunk), e);
            }
        }
        if i + 1 < chunks.len() {
            tokio::time::sleep_until(Instant::now() + MIN_INTERVAL).await;
        }
    }
    state.reply_to_message_id = None;

    // If we got a message_id for the sent message, manage reactions:
    // - intermediate flush: remove old processing reaction, add new processing reaction
    // - final flush: remove old processing reaction, add done reaction directly
    if let Some(ref new_mid) = last_sent_id {
        // Remove processing reaction from previous bot message
        if let Some(ref prev_mid) = state.last_bot_message_id.clone() {
            if let Some(rid) = state.last_reaction_id.take() {
                let _ = transport.remove_reaction(channel_id, prev_mid, &rid).await;
                state.last_reaction_message_id = None;
            }
        }
        let reaction_emoji = if is_final { caps.done_reaction } else { caps.processing_reaction };
        match transport.add_reaction(channel_id, new_mid, reaction_emoji).await {
            Ok(Some(rid)) => { state.last_reaction_id = Some(rid); }
            Ok(None) => { state.last_reaction_id = Some(reaction_emoji.to_string()); }
            Err(e) => { eprintln!("{} chat_id={} direction=add_reaction({}) error={:?}", prefix(channel_id), channel_id, if is_final { "done" } else { "intermediate" }, e); }
        }
        state.last_reaction_message_id = Some(new_mid.clone());
        state.last_bot_message_id = Some(new_mid.clone());
    }

    last_sent_id
}

/// One send daemon for a single channel: drains that channel's FIFO queue, applies rate limit and stream-edit.
/// Branches on transport capabilities:
///   buffer_stream => accumulate all parts, send as complete messages on StreamEnd
///   supports_stream_edit => send then edit in place
///   else => buffer and send once on StreamEnd (no-edit passthrough)
async fn run_send_daemon_for_channel<T>(
    mut rx: mpsc::Receiver<OutboundMsg>,
    tx: mpsc::Sender<OutboundMsg>,
    channel_id: String,
    transport: Arc<T>,
) where
    T: ImTransport,
{
    let caps = transport.capabilities();
    let max_len = transport.max_message_len();
    let mut state = ChannelSendState {
        last_send: None,
        retry_after: None,
        stream_sent: false,
        stream_message_id: None,
        last_edit: None,
        pending_stream_text: None,
        last_progress: None,
        response_parts: VecDeque::new(),
        buffer_parts: Vec::new(),
        reply_to_message_id: None,
        last_reaction_id: None,
        last_reaction_message_id: None,
        last_bot_message_id: None,
    };

    while let Some(msg) = rx.recv().await {
        match msg {
            OutboundMsg::StreamProgress(_, label) => {
                if caps.buffer_stream {
                    // Buffer mode: skip progress labels, they'll be shown via reaction
                    continue;
                }
                if caps.supports_stream_edit {
                    state.last_progress = Some(label.clone());
                    let to_show = state.pending_stream_text.as_deref().unwrap_or(&label);
                    let now = Instant::now();
                    let can_edit = state
                        .last_edit
                        .map(|t| now.duration_since(t) >= MIN_EDIT_INTERVAL)
                        .unwrap_or(true);
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
                // No-edit, no-buffer channel: skip progress
            }
            OutboundMsg::StreamPart(_, text) => {
                if caps.buffer_stream {
                    // Buffer mode: accumulate text, don't send yet
                    state.buffer_parts.push(text);
                    continue;
                }
                if caps.supports_stream_edit {
                    state.last_progress = None;
                    let acc = state.pending_stream_text.take().unwrap_or_default();
                    state.pending_stream_text = Some(acc + &text);
                    let to_show = state.pending_stream_text.as_deref().unwrap_or("…");
                    let now = Instant::now();
                    let can_edit = state
                        .last_edit
                        .map(|t| now.duration_since(t) >= MIN_EDIT_INTERVAL)
                        .unwrap_or(true);
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
                } else {
                    state.response_parts.push_back(text);
                    while drain_one_response_part(&channel_id, &mut state, &transport, max_len).await
                    {}
                }
            }
            OutboundMsg::StreamEdit(_, text) => {
                if caps.buffer_stream {
                    state.buffer_parts.push(text);
                    continue;
                }
                if !caps.supports_stream_edit {
                    state.response_parts.push_back(text);
                    while drain_one_response_part(&channel_id, &mut state, &transport, max_len).await
                    {}
                    continue;
                }
                if let Some(ref mid) = state.stream_message_id {
                    let now = Instant::now();
                    let can_edit = state
                        .last_edit
                        .map(|t| now.duration_since(t) >= MIN_EDIT_INTERVAL)
                        .unwrap_or(true);
                    if can_edit {
                        let to_show = state.pending_stream_text.take().unwrap_or_else(|| text.clone());
                        match transport.edit_message(&channel_id, mid, &to_show).await {
                            Ok(()) => state.last_edit = Some(Instant::now()),
                            Err(SendError::RateLimited { retry_after_secs }) => {
                                state.retry_after =
                                    Some(Instant::now() + Duration::from_secs_f64(retry_after_secs));
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
                            state.retry_after =
                                Some(Instant::now() + Duration::from_secs_f64(retry_after_secs));
                            let _ = tx.send(OutboundMsg::StreamEdit(channel_id.clone(), text)).await;
                        }
                        Err(SendError::Other(e)) => {
                            eprintln!("{} chat_id={} direction=send content={} error={}", prefix(&channel_id), channel_id, truncate_content_default(&text), e);
                        }
                    }
                }
            }
            OutboundMsg::StreamEnd(_) => {
                if caps.buffer_stream {
                    // Buffer mode: flush this block (intermediate — more may follow)
                    flush_buffer(&channel_id, &mut state, &transport, &caps, max_len, false).await;
                    // Also drain any remaining response_parts
                    while !state.response_parts.is_empty() {
                        if !drain_one_response_part(&channel_id, &mut state, &transport, max_len).await {
                            break;
                        }
                    }
                    state.buffer_parts.clear();
                    state.stream_sent = false;
                    state.stream_message_id = None;
                    state.pending_stream_text = None;
                    state.last_progress = None;
                    continue;
                }
                if !caps.supports_stream_edit {
                    while !state.response_parts.is_empty() {
                        if !drain_one_response_part(&channel_id, &mut state, &transport, max_len).await
                        {
                            break;
                        }
                    }
                    if !state.response_parts.is_empty() {
                        let _ = tx.send(OutboundMsg::StreamEnd(channel_id.clone())).await;
                    }
                    continue;
                }
                let mid = state.stream_message_id.take();
                let pending = state.pending_stream_text.take();
                state.pending_stream_text = None;
                state.last_progress = None;
                state.stream_sent = false;
                if let (Some(mid), Some(pending)) = (mid, pending) {
                    let _ = transport.edit_message(&channel_id, &mid, &pending).await;
                }
            }
            OutboundMsg::StreamDone(_) => {
                if caps.buffer_stream {
                    // Flush the final block — flush_buffer with is_final=true adds done reaction directly
                    flush_buffer(&channel_id, &mut state, &transport, &caps, max_len, true).await;
                    while !state.response_parts.is_empty() {
                        if !drain_one_response_part(&channel_id, &mut state, &transport, max_len).await {
                            break;
                        }
                    }
                    state.buffer_parts.clear();
                    state.stream_sent = false;
                    state.stream_message_id = None;
                    state.pending_stream_text = None;
                    state.last_progress = None;

                    // If flush_buffer had nothing to flush (empty final block),
                    // the done reaction was already placed by the last intermediate flush's message.
                    // In that case, swap processing → done on the last bot message.
                    if state.last_bot_message_id.is_some() {
                        // Check if the last reaction is still a processing reaction (not yet swapped to done)
                        if let Some(ref last_mid) = state.last_bot_message_id.clone() {
                            if state.last_reaction_id.is_some() {
                                // Still has a processing reaction — swap to done
                                if let Some(rid) = state.last_reaction_id.take() {
                                    let _ = transport.remove_reaction(&channel_id, last_mid, &rid).await;
                                    state.last_reaction_message_id = None;
                                }
                                match transport.add_reaction(&channel_id, last_mid, caps.done_reaction).await {
                                    Ok(Some(rid)) => { state.last_reaction_id = Some(rid); state.last_reaction_message_id = Some(last_mid.clone()); }
                                    Ok(None) => {}
                                    Err(e) => { eprintln!("{} chat_id={} direction=add_reaction(done_fallback) error={:?}", prefix(&channel_id), channel_id, e); }
                                }
                            }
                        }
                    }
                    state.last_bot_message_id = None;
                    continue;
                }
                // Non-buffer: treat same as StreamEnd
                if !caps.supports_stream_edit {
                    while !state.response_parts.is_empty() {
                        if !drain_one_response_part(&channel_id, &mut state, &transport, max_len).await {
                            break;
                        }
                    }
                    continue;
                }
                let mid = state.stream_message_id.take();
                let pending = state.pending_stream_text.take();
                state.pending_stream_text = None;
                state.last_progress = None;
                state.stream_sent = false;
                if let (Some(mid), Some(pending)) = (mid, pending) {
                    let _ = transport.edit_message(&channel_id, &mid, &pending).await;
                }
            }
            OutboundMsg::Send(_, text) => {
                state.stream_message_id = None;
                state.stream_sent = false;
                state.pending_stream_text = None;
                state.last_progress = None;
                state.response_parts.clear();

                let now = Instant::now();
                let wait_until = state
                    .retry_after
                    .filter(|t| *t > now)
                    .or_else(|| {
                        state.last_send.and_then(|t| {
                            let next = t + MIN_INTERVAL;
                            if next > now {
                                Some(next)
                            } else {
                                None
                            }
                        })
                    });

                if let Some(until) = wait_until {
                    tokio::time::sleep_until(until).await;
                    let _ = tx.send(OutboundMsg::Send(channel_id.clone(), text)).await;
                    continue;
                }

                match transport.send(channel_id.as_str(), &text).await {
                    Ok(_) => {
                        state.last_send = Some(Instant::now());
                        state.retry_after = None;
                    }
                    Err(SendError::RateLimited { retry_after_secs }) => {
                        state.retry_after =
                            Some(Instant::now() + Duration::from_secs_f64(retry_after_secs));
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
                if let Some(until) = wait_until {
                    tokio::time::sleep_until(until).await;
                }
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
                        // Platform didn't return a reaction_id (e.g. Telegram uses emoji as id)
                        state.last_reaction_id = Some(emoji.clone());
                        state.last_reaction_message_id = Some(message_id);
                    }
                    Err(e) => {
                        eprintln!("{} chat_id={} direction=add_reaction error={:?}", prefix(&channel_id), channel_id, e);
                    }
                }
            }
            OutboundMsg::RemoveReaction(_, message_id, _reaction_id_hint) => {
                // Use the stored reaction_id from the last AddReaction if the message matches
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
                if let Some(until) = wait_until {
                    tokio::time::sleep_until(until).await;
                }
                match transport.send_interactive(&channel_id, &prompt, &options, reply_to.as_deref()).await {
                    Ok(_) => { state.last_send = Some(Instant::now()); }
                    Err(e) => {
                        eprintln!("{} chat_id={} direction=send_interactive error={:?}", prefix(&channel_id), channel_id, e);
                    }
                }
            }
        }
    }
}

/// Hub that routes outbound messages to a per-channel FIFO queue and a dedicated daemon task per channel.
pub struct OutboundHub<T> {
    channels: DashMap<String, mpsc::Sender<OutboundMsg>>,
    transport: Arc<T>,
}

impl<T> OutboundHub<T>
where
    T: ImTransport + 'static,
{
    pub fn new(transport: Arc<T>) -> Arc<Self> {
        Arc::new(Self {
            channels: DashMap::new(),
            transport,
        })
    }

    /// Get or create the sender for this channel (creates a new FIFO queue and spawns a daemon task).
    pub fn sender_for(&self, channel_id: &str) -> mpsc::Sender<OutboundMsg> {
        self.channels
            .entry(channel_id.to_string())
            .or_insert_with(|| {
                let channel_id = channel_id.to_string();
                let (tx, rx) = mpsc::channel::<OutboundMsg>(256);
                let transport = Arc::clone(&self.transport);
                tokio::spawn(run_send_daemon_for_channel(rx, tx.clone(), channel_id, transport));
                tx
            })
            .clone()
    }

    /// Enqueue an outbound message for the given channel (FIFO per channel).
    pub async fn send(&self, channel_id: &str, msg: OutboundMsg) {
        let tx = self.sender_for(channel_id);
        let _ = tx.send(msg).await;
    }

    /// Set the reply_to message_id for buffer mode (so first flushed message quotes the user).
    /// This is a hint stored in the daemon state via a special message.
    pub async fn set_reply_to(&self, channel_id: &str, reply_to_message_id: String) {
        let tx = self.sender_for(channel_id);
        let _ = tx.send(OutboundMsg::SetReplyTo(
            channel_id.to_string(), reply_to_message_id,
        )).await;
    }

    /// Max message length for this transport (used by worker for truncation and chunking).
    pub fn max_message_len(&self) -> usize {
        self.transport.max_message_len()
    }

    /// Unified capabilities for this transport's channel (supports_stream_edit, max_message_len, etc.).
    pub fn capabilities(&self) -> ImChannelCapabilities {
        self.transport.capabilities()
    }

    /// Send a message directly (bypassing the queue) and return the message_id.
    /// Used when the caller needs the message_id immediately (e.g. to add a reaction to it later).
    pub async fn send_direct(&self, channel_id: &str, text: &str) -> Option<String> {
        self.transport.send(channel_id, text).await.ok().flatten()
    }
}
