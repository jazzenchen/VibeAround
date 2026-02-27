//! IM worker: take (channel_id, prompt) from inbound queue, run headless CLI, push segments to outbound.
//! Each conversation creates a job workspace; when done, empty dirs are removed, dirs with HTML get a preview link.

use std::path::Path;
use std::sync::Arc;

use dashmap::DashMap;
use tokio::sync::mpsc;

use super::commands;
use super::daemon::{OutboundHub, OutboundMsg};
use super::log::{prefix, truncate_content_default};
use crate::config;
use crate::headless;
use crate::workspace;

/// Attachment metadata from Feishu file/image messages.
/// The actual download happens in the worker after the job workspace is created.
#[derive(Debug, Clone)]
pub struct FeishuAttachment {
    pub message_id: String,
    pub file_key: String,
    pub file_name: String,
    /// "file" or "image"
    pub resource_type: String,
}

/// Inbound message from any IM channel to the worker.
#[derive(Debug, Clone)]
pub struct InboundMessage {
    pub channel_id: String,
    pub text: String,
    /// Feishu attachments to download into the job workspace before running Claude.
    pub attachments: Vec<FeishuAttachment>,
}

impl InboundMessage {
    /// Simple text-only message (used by Telegram and plain Feishu text).
    pub fn text_only(channel_id: String, text: String) -> Self {
        Self { channel_id, text, attachments: vec![] }
    }
}

/// Download Feishu attachments into the given directory using the Feishu transport.
/// Returns a list of (file_name, local_path) for successfully downloaded files.
async fn download_attachments(
    attachments: &[FeishuAttachment],
    dest_dir: &Path,
    transport: &crate::im::channels::feishu::FeishuTransport,
) -> Vec<(String, String)> {
    let mut downloaded = Vec::new();
    for att in attachments {
        let local_name = att.file_name.clone();
        let dest = dest_dir.join(&local_name);
        match transport.download_resource(&att.message_id, &att.file_key, &att.resource_type).await {
            Ok(bytes) => {
                if let Err(e) = std::fs::write(&dest, &bytes) {
                    eprintln!("[VibeAround][im][worker] write attachment error: {} path={}", e, dest.display());
                    continue;
                }
                eprintln!("[VibeAround][im][worker] downloaded attachment: {} ({} bytes)", dest.display(), bytes.len());
                downloaded.push((local_name, dest.to_string_lossy().to_string()));
            }
            Err(e) => {
                eprintln!("[VibeAround][im][worker] download_resource error: {:?} file_key={}", e, att.file_key);
            }
        }
    }
    downloaded
}

pub async fn run_worker<T>(
    mut inbound_rx: mpsc::Receiver<InboundMessage>,
    outbound: Arc<OutboundHub<T>>,
    busy_set: Arc<DashMap<String, ()>>,
    feishu_transport: Option<Arc<crate::im::channels::feishu::FeishuTransport>>,
) where
    T: crate::im::transport::ImTransport + 'static,
{
    let working_dir = config::ensure_loaded().working_dir.clone();

    while let Some(msg) = inbound_rx.recv().await {
        let channel_id = msg.channel_id.clone();
        busy_set.insert(channel_id.clone(), ());

        if commands::is_list_project(&msg.text) {
            let body = commands::format_list_projects(&working_dir);
            let _ = outbound.send(&channel_id, OutboundMsg::StreamPart(channel_id.clone(), body)).await;
            let _ = outbound.send(&channel_id, OutboundMsg::StreamEnd(channel_id.clone())).await;
            busy_set.remove(&channel_id);
            continue;
        }

        let channel_id_segment = channel_id.clone();
        let tx = outbound.sender_for(&channel_id_segment);

        let job_name = msg.text.chars().take(50).collect::<String>();
        let job_name = if job_name.is_empty() { "IM".into() } else { job_name };
        let job = match workspace::create_job(&working_dir, job_name, String::new()) {
            Ok(j) => j,
            Err(e) => {
                eprintln!("{} create_job failed: {}", prefix(&channel_id), e);
                let _ = outbound.send(&channel_id, OutboundMsg::StreamPart(channel_id.clone(), format!("Error: {}", e))).await;
                let _ = outbound.send(&channel_id, OutboundMsg::StreamEnd(channel_id.clone())).await;
                busy_set.remove(&channel_id);
                continue;
            }
        };
        let job_id = job.job_id.clone();
        let cwd = match workspace::job_workspace_path(&working_dir, &job_id) {
            Some(p) => p,
            None => {
                let _ = outbound.send(&channel_id, OutboundMsg::StreamPart(channel_id.clone(), "Error: job path not found".into())).await;
                let _ = outbound.send(&channel_id, OutboundMsg::StreamEnd(channel_id.clone())).await;
                busy_set.remove(&channel_id);
                continue;
            }
        };

        // Download Feishu attachments into the job workspace
        let mut prompt = msg.text.clone();
        if !msg.attachments.is_empty() {
            if let Some(ref ft) = feishu_transport {
                let downloaded = download_attachments(&msg.attachments, &cwd, ft).await;
                if !downloaded.is_empty() {
                    let file_list: Vec<String> = downloaded.iter().map(|(name, _)| name.clone()).collect();
                    prompt = format!(
                        "{}\n\n[Attached files in current directory: {}]",
                        prompt,
                        file_list.join(", ")
                    );
                } else {
                    let _ = outbound.send(&channel_id, OutboundMsg::StreamPart(channel_id.clone(), "Warning: failed to download attachments.".into())).await;
                }
            }
        }

        let send_segment = |seg: headless::ClaudeSegment| {
            let msg = match &seg {
                headless::ClaudeSegment::Progress(p) => {
                    let s = match p {
                        headless::ClaudeProgress::Thinking => "Thinking...".to_string(),
                        headless::ClaudeProgress::ToolUse { name } => format!("Using tool: {}...", name),
                    };
                    OutboundMsg::StreamProgress(channel_id_segment.clone(), s)
                }
                headless::ClaudeSegment::TextPart(text) => {
                    OutboundMsg::StreamPart(channel_id_segment.clone(), text.clone())
                }
            };
            let _ = tx.try_send(msg);
        };

        let _ = outbound.send(&channel_id, OutboundMsg::StreamPart(channel_id.clone(), "…".to_string())).await;

        let result = headless::run_claude_prompt_to_stream_parts(&prompt, send_segment, Some(cwd)).await;

        if let Err(e) = result {
            eprintln!("{} chat_id={} direction=worker_error prompt={} error={}", prefix(&channel_id), channel_id, truncate_content_default(&prompt), e);
            let _ = outbound.send(&channel_id, OutboundMsg::StreamPart(channel_id.clone(), format!("Error: {}", e))).await;
        }

        if let Some(ref job_path) = workspace::job_workspace_path(&working_dir, &job_id) {
            if workspace::is_dir_empty(job_path) {
                let _ = workspace::delete_job(&working_dir, &job_id);
            } else if workspace::dir_has_html(job_path) {
                let preview = match config::preview_base_url() {
                    Some(base) => format!("Preview: {}/preview/{}", base.trim_end_matches('/'), job_id),
                    None => format!("Preview: /preview/{}", job_id),
                };
                let _ = outbound.send(&channel_id, OutboundMsg::StreamPart(channel_id.clone(), preview)).await;
            }
        }

        let _ = outbound.send(&channel_id, OutboundMsg::StreamEnd(channel_id.clone())).await;

        busy_set.remove(&channel_id);
    }
}
