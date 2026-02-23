//! Per-channel outbound: one FIFO queue and one send daemon task per channel.
//! Stream-edit (send then edit in place) or response-parts FIFO (Feishu: one message per Claude segment).

use std::collections::VecDeque;
use std::sync::Arc;
use std::time::Duration;

use dashmap::DashMap;
use tokio::sync::mpsc;
use tokio::time::Instant;

use super::log::{prefix, truncate_content_default};
use super::transport::{ImChannelCapabilities, ImTransport, SendError};
use crate::im::transport;

/// Outbound queue item: stream progress (label), stream part (text block), stream edit (legacy), stream end, or send.
#[derive(Debug, Clone)]
pub enum OutboundMsg {
    /// Progress label only (e.g. "Thinking..."). Edit: show until we have text; no-edit: push as one part.
    StreamProgress(String, String),
    /// One text segment from Claude. No-edit: push to response FIFO; edit: append and edit in place.
    StreamPart(String, String),
    /// Legacy: single content update (edit channel). Throttled.
    StreamEdit(String, String),
    /// End stream: flush edit or drain response FIFO.
    StreamEnd(String),
    /// Send as a new message. Rate-limited.
    Send(String, String),
}

/// Per-channel state.
struct ChannelSendState {
    last_send: Option<Instant>,
    retry_after: Option<Instant>,
    stream_message_id: Option<i32>,
    last_edit: Option<Instant>,
    /// Edit path: accumulated text from StreamPart; shown in place. Progress labels (StreamProgress) shown only when this is empty.
    pending_stream_text: Option<String>,
    /// Edit path: last progress label (Thinking... / Using tool: X...) when no text yet.
    last_progress: Option<String>,
    /// For !supports_stream_edit: FIFO of segments to send. Each sent with rate limit; if part > max_len, split into multiple messages.
    response_parts: VecDeque<String>,
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
                    truncate_content_default(chunk),
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

/// One send daemon for a single channel: drains that channel's FIFO queue, applies rate limit and stream-edit.
/// Branches on transport capabilities: supports_stream_edit => send then edit in place; else => buffer and send once on StreamEnd.
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
        stream_message_id: None,
        last_edit: None,
        pending_stream_text: None,
        last_progress: None,
        response_parts: VecDeque::new(),
    };

    while let Some(msg) = rx.recv().await {
        match msg {
            OutboundMsg::StreamProgress(_, label) => {
                if caps.supports_stream_edit {
                    state.last_progress = Some(label.clone());
                    let to_show = state.pending_stream_text.as_deref().unwrap_or(&label);
                    let now = Instant::now();
                    let can_edit = state
                        .last_edit
                        .map(|t| now.duration_since(t) >= MIN_EDIT_INTERVAL)
                        .unwrap_or(true);
                    if state.stream_message_id.is_none() {
                        if let Ok(Some(mid)) = transport.send(&channel_id, to_show).await {
                            state.stream_message_id = Some(mid);
                            state.last_edit = Some(Instant::now());
                        } else if let Ok(None) = transport.send(&channel_id, to_show).await {
                            state.stream_message_id = Some(-1);
                        }
                    } else if let Some(mid) = state.stream_message_id {
                        if mid >= 0 && can_edit {
                            let _ = transport.edit_message(&channel_id, mid, to_show).await;
                            state.last_edit = Some(Instant::now());
                        }
                    }
                }
                // No-edit channel: skip progress (Thinking... / Using tool: X...); only text parts are sent.
            }
            OutboundMsg::StreamPart(_, text) => {
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
                    if state.stream_message_id.is_none() {
                        if let Ok(Some(mid)) = transport.send(&channel_id, to_show).await {
                            state.stream_message_id = Some(mid);
                            state.last_edit = Some(Instant::now());
                        } else if let Ok(None) = transport.send(&channel_id, to_show).await {
                            state.stream_message_id = Some(-1);
                        }
                    } else if let Some(mid) = state.stream_message_id {
                        if mid >= 0 && can_edit {
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
                if !caps.supports_stream_edit {
                    state.response_parts.push_back(text);
                    while drain_one_response_part(&channel_id, &mut state, &transport, max_len).await
                    {}
                    continue;
                }
                if let Some(mid) = state.stream_message_id {
                    if mid == -1 {
                        state.pending_stream_text = Some(text);
                        continue;
                    }
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
                } else {
                    match transport.send(&channel_id, &text).await {
                        Ok(Some(mid)) => {
                            state.stream_message_id = Some(mid);
                            state.last_edit = Some(Instant::now());
                            state.pending_stream_text = None;
                        }
                        Ok(None) => {
                            state.stream_message_id = Some(-1);
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
                if let (Some(mid), Some(pending)) = (mid, pending) {
                    if mid >= 0 {
                        let _ = transport.edit_message(&channel_id, mid, &pending).await;
                    }
                }
            }
            OutboundMsg::Send(_, text) => {
                state.stream_message_id = None;
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

    /// Max message length for this transport (used by worker for truncation and chunking).
    pub fn max_message_len(&self) -> usize {
        self.transport.max_message_len()
    }

    /// Unified capabilities for this transport's channel (supports_stream_edit, max_message_len, etc.).
    pub fn capabilities(&self) -> ImChannelCapabilities {
        self.transport.capabilities()
    }
}
