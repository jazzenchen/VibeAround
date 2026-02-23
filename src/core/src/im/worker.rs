//! IM worker: take (channel_id, prompt) from inbound queue, run headless CLI, push segments to outbound.
//! Each conversation creates a job workspace; when done, empty dirs are removed, dirs with HTML get a preview link.

use std::sync::Arc;

use dashmap::DashMap;
use tokio::sync::mpsc;

use super::commands;
use super::daemon::{OutboundHub, OutboundMsg};
use super::log::{prefix, truncate_content_default};
use crate::config;
use crate::headless;
use crate::workspace;

pub async fn run_worker<T>(
    mut inbound_rx: mpsc::Receiver<(String, String)>,
    outbound: Arc<OutboundHub<T>>,
    busy_set: Arc<DashMap<String, ()>>,
) where
    T: crate::im::transport::ImTransport + 'static,
{
    let working_dir = config::ensure_loaded().working_dir.clone();

    while let Some((channel_id, prompt)) = inbound_rx.recv().await {
        busy_set.insert(channel_id.clone(), ());

        if commands::is_list_project(&prompt) {
            let body = commands::format_list_projects(&working_dir);
            let _ = outbound.send(&channel_id, OutboundMsg::StreamPart(channel_id.clone(), body)).await;
            let _ = outbound.send(&channel_id, OutboundMsg::StreamEnd(channel_id.clone())).await;
            busy_set.remove(&channel_id);
            continue;
        }

        let channel_id_segment = channel_id.clone();
        let tx = outbound.sender_for(&channel_id_segment);

        let job_name = prompt.chars().take(50).collect::<String>();
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
