//! IM worker: take InboundMessage from inbound queue, route to session, run headless CLI, push segments to outbound.
//! Integrates: router (session resolution), session store (SQLite), context manager, project management.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use dashmap::DashMap;
use rusqlite::Connection;
use tokio::sync::mpsc;

use super::daemon::{OutboundHub, OutboundMsg};
use super::log::{prefix, truncate_content_default};
use super::router::{CommandAction, SessionAction, SessionResolver};
use crate::config;
use crate::headless;
use crate::headless::context::ContextManager;
use crate::im::session as session_store;
use crate::project;
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
    /// If this message is a quote-reply, the message_id of the parent message.
    /// Used by the router to look up which session the parent belongs to.
    pub parent_id: Option<String>,
}

impl InboundMessage {
    /// Simple text-only message (used by Telegram and plain Feishu text).
    pub fn text_only(channel_id: String, text: String) -> Self {
        Self { channel_id, text, attachments: vec![], parent_id: None }
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

    // Open SQLite database
    let db = match crate::db::open_db(&working_dir) {
        Ok(conn) => Arc::new(Mutex::new(conn)),
        Err(e) => {
            eprintln!("[VibeAround][im][worker] failed to open database: {}", e);
            // Fallback: run without session management (legacy mode)
            run_worker_legacy(inbound_rx, outbound, busy_set, feishu_transport, &working_dir).await;
            return;
        }
    };

    let _ = workspace::ensure_workspace_dirs(&working_dir);

    let runner = headless::ClaudeRunner;
    let resolver = SessionResolver::new(db.clone());
    let context_mgr = ContextManager::new(db.clone());

    while let Some(msg) = inbound_rx.recv().await {
        let channel_id = msg.channel_id.clone();
        busy_set.insert(channel_id.clone(), ());

        let action = resolver.resolve(
            &channel_id,
            &msg.text,
            msg.parent_id.as_deref(),
            &runner,
        ).await;

        match action {
            SessionAction::Command(cmd) => {
                handle_command(cmd, &channel_id, &outbound, &resolver, &db, &working_dir).await;
                busy_set.remove(&channel_id);
                continue;
            }
            SessionAction::Continue(session) => {
                run_with_session(
                    &msg, &session, &channel_id, &outbound, &runner, &context_mgr,
                    &db, &working_dir, feishu_transport.as_deref(), None,
                ).await;
            }
            SessionAction::SwitchTo { session, reason } => {
                // Notify user about the switch
                let _ = outbound.send(&channel_id, OutboundMsg::StreamPart(
                    channel_id.clone(),
                    format!("[Switched to project: {}]", reason),
                )).await;
                run_with_session(
                    &msg, &session, &channel_id, &outbound, &runner, &context_mgr,
                    &db, &working_dir, feishu_transport.as_deref(), None,
                ).await;
            }
            SessionAction::CreateNew { project_id, reason } => {
                let cwd = project::project_workspace_path(&working_dir, &project_id);
                let _ = std::fs::create_dir_all(&cwd);
                let session = {
                    let conn = db.lock().unwrap();
                    session_store::create_session(&conn, &project_id, &cwd.to_string_lossy())
                };
                match session {
                    Ok(session) => {
                        resolver.active_sessions.insert(channel_id.clone(), session.session_id.clone());
                        let _ = outbound.send(&channel_id, OutboundMsg::StreamPart(
                            channel_id.clone(),
                            format!("[New session: {}]", reason),
                        )).await;
                        run_with_session(
                            &msg, &session, &channel_id, &outbound, &runner, &context_mgr,
                            &db, &working_dir, feishu_transport.as_deref(), None,
                        ).await;
                    }
                    Err(e) => {
                        let _ = outbound.send(&channel_id, OutboundMsg::StreamPart(
                            channel_id.clone(), format!("Error creating session: {}", e),
                        )).await;
                        let _ = outbound.send(&channel_id, OutboundMsg::StreamEnd(channel_id.clone())).await;
                    }
                }
            }
            SessionAction::CreateNewProject { suggested_name, reason } => {
                let proj = {
                    let conn = db.lock().unwrap();
                    project::create_project(&conn, &working_dir, suggested_name.clone())
                };
                match proj {
                    Ok(proj) => {
                        let cwd = project::project_workspace_path(&working_dir, &proj.project_id);
                        let session = {
                            let conn = db.lock().unwrap();
                            session_store::create_session(&conn, &proj.project_id, &cwd.to_string_lossy())
                        };
                        match session {
                            Ok(session) => {
                                resolver.active_sessions.insert(channel_id.clone(), session.session_id.clone());
                                let _ = outbound.send(&channel_id, OutboundMsg::StreamPart(
                                    channel_id.clone(),
                                    format!("[New project '{}': {}]", suggested_name, reason),
                                )).await;
                                run_with_session(
                                    &msg, &session, &channel_id, &outbound, &runner, &context_mgr,
                                    &db, &working_dir, feishu_transport.as_deref(), None,
                                ).await;
                            }
                            Err(e) => {
                                let _ = outbound.send(&channel_id, OutboundMsg::StreamPart(
                                    channel_id.clone(), format!("Error creating session: {}", e),
                                )).await;
                                let _ = outbound.send(&channel_id, OutboundMsg::StreamEnd(channel_id.clone())).await;
                            }
                        }
                    }
                    Err(e) => {
                        let _ = outbound.send(&channel_id, OutboundMsg::StreamPart(
                            channel_id.clone(), format!("Error creating project: {}", e),
                        )).await;
                        let _ = outbound.send(&channel_id, OutboundMsg::StreamEnd(channel_id.clone())).await;
                    }
                }
            }
        }

        busy_set.remove(&channel_id);
    }
}

/// Run Claude with a resolved session: download attachments, build prompt, stream output.
async fn run_with_session<T>(
    msg: &InboundMessage,
    session: &session_store::ChatSession,
    channel_id: &str,
    outbound: &Arc<OutboundHub<T>>,
    runner: &headless::ClaudeRunner,
    context_mgr: &ContextManager,
    db: &Arc<Mutex<Connection>>,
    _working_dir: &Path,
    feishu_transport: Option<&crate::im::channels::feishu::FeishuTransport>,
    _preview_session_id: Option<&str>,
) where
    T: crate::im::transport::ImTransport + 'static,
{
    let cwd = PathBuf::from(&session.cwd);
    let _ = std::fs::create_dir_all(&cwd);

    // Download Feishu attachments
    let mut prompt = msg.text.clone();
    if !msg.attachments.is_empty() {
        if let Some(ft) = feishu_transport {
            let downloaded = download_attachments(&msg.attachments, &cwd, ft).await;
            if !downloaded.is_empty() {
                let file_list: Vec<String> = downloaded.iter().map(|(name, _)| name.clone()).collect();
                prompt = format!(
                    "{}\n\n[Attached files in current directory: {}]",
                    prompt,
                    file_list.join(", ")
                );
            } else {
                let _ = outbound.send(channel_id, OutboundMsg::StreamPart(
                    channel_id.to_string(), "Warning: failed to download attachments.".into(),
                )).await;
            }
        }
    }

    // Context prefix (for runners without native resume)
    if let Some(ctx_prefix) = context_mgr.get_context_prefix(&session.session_id, runner) {
        prompt = format!("{}{}", ctx_prefix, prompt);
    }

    // Determine session mode
    let session_mode = if let Some(ref rsid) = session.runner_session_id {
        Some(headless::SessionMode::Resume(rsid.clone()))
    } else {
        let new_id = uuid::Uuid::new_v4().to_string();
        Some(headless::SessionMode::New(new_id))
    };

    let channel_id_segment = channel_id.to_string();
    let tx = outbound.sender_for(&channel_id_segment);

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

    let result = headless::run_claude_prompt_to_stream_parts(
        &prompt, send_segment, Some(cwd.clone()), session_mode,
    ).await;

    match result {
        Ok(runner_result) => {
            // Store runner_session_id if we got one and didn't have one before
            if session.runner_session_id.is_none() {
                if let Some(ref rsid) = runner_result.session_id {
                    let conn = db.lock().unwrap();
                    let _ = session_store::set_runner_session_id(&conn, &session.session_id, rsid);
                }
            }
            // Touch session
            let conn = db.lock().unwrap();
            let _ = session_store::touch_session(&conn, &session.session_id);
        }
        Err(e) => {
            eprintln!("{} chat_id={} direction=worker_error prompt={} error={}",
                prefix(channel_id), channel_id, truncate_content_default(&prompt), e);
            let _ = outbound.send(channel_id, OutboundMsg::StreamPart(
                channel_id.to_string(), format!("Error: {}", e),
            )).await;
        }
    }

    // Check for HTML preview
    if workspace::dir_has_html(&cwd) {
        let preview = match config::preview_base_url() {
            Some(base) => format!("Preview: {}/preview/{}", base.trim_end_matches('/'), session.project_id),
            None => format!("Preview: /preview/{}", session.project_id),
        };
        let _ = outbound.send(channel_id, OutboundMsg::StreamPart(channel_id.to_string(), preview)).await;
    }

    let _ = outbound.send(channel_id, OutboundMsg::StreamEnd(channel_id.to_string())).await;
}

/// Handle slash commands.
async fn handle_command<T>(
    cmd: CommandAction,
    channel_id: &str,
    outbound: &Arc<OutboundHub<T>>,
    resolver: &SessionResolver,
    db: &Arc<Mutex<Connection>>,
    working_dir: &Path,
) where
    T: crate::im::transport::ImTransport + 'static,
{
    let reply = match cmd {
        CommandAction::NewSession => {
            resolver.active_sessions.remove(channel_id);
            "New session started. Send your next message to begin.".to_string()
        }
        CommandAction::ListSessions => {
            let conn = db.lock().unwrap();
            let sessions = session_store::list_all_sessions(&conn).unwrap_or_default();
            if sessions.is_empty() {
                "No sessions.".to_string()
            } else {
                let active_sid = resolver.active_sessions.get(channel_id).map(|e| e.value().clone());
                sessions.iter().enumerate().map(|(i, s)| {
                    let marker = if active_sid.as_deref() == Some(&s.session_id) { " (active)" } else { "" };
                    let proj = {
                        project::get_project(&conn, &s.project_id).ok().flatten()
                            .map(|p| p.name).unwrap_or_else(|| "?".to_string())
                    };
                    format!("{}. [{}] project='{}' last_active={}{}", i + 1, &s.session_id[..8], proj, s.last_active_at, marker)
                }).collect::<Vec<_>>().join("\n")
            }
        }
        CommandAction::SwitchSession(target) => {
            let conn = db.lock().unwrap();
            let sessions = session_store::list_all_sessions(&conn).unwrap_or_default();
            let found = sessions.iter().find(|s| s.session_id.starts_with(&target));
            match found {
                Some(s) => {
                    resolver.active_sessions.insert(channel_id.to_string(), s.session_id.clone());
                    format!("Switched to session [{}]", &s.session_id[..8])
                }
                None => format!("Session '{}' not found.", target),
            }
        }
        CommandAction::ListProjects | CommandAction::ProjectList => {
            let conn = db.lock().unwrap();
            let projects = project::list_projects(&conn).unwrap_or_default();
            if projects.is_empty() {
                "No projects.".to_string()
            } else {
                projects.iter().enumerate().map(|(i, p)| {
                    format!("{}. [{}] {}", i + 1, &p.project_id[..8], p.name)
                }).collect::<Vec<_>>().join("\n")
            }
        }
        CommandAction::ProjectNew(name) => {
            let conn = db.lock().unwrap();
            match project::create_project(&conn, working_dir, name.clone()) {
                Ok(p) => format!("Created project '{}' [{}]", name, &p.project_id[..8]),
                Err(e) => format!("Error creating project: {}", e),
            }
        }
        CommandAction::ProjectUse(target) => {
            let conn = db.lock().unwrap();
            let projects = project::list_projects(&conn).unwrap_or_default();
            let found = projects.iter().find(|p| {
                p.project_id.starts_with(&target) || p.name.to_lowercase().contains(&target.to_lowercase())
            });
            match found {
                Some(p) => {
                    // Find or create a session for this project
                    let sessions = session_store::list_sessions_by_project(&conn, &p.project_id).unwrap_or_default();
                    if let Some(s) = sessions.first() {
                        resolver.active_sessions.insert(channel_id.to_string(), s.session_id.clone());
                        format!("Switched to project '{}' session [{}]", p.name, &s.session_id[..8])
                    } else {
                        let cwd = project::project_workspace_path(working_dir, &p.project_id);
                        match session_store::create_session(&conn, &p.project_id, &cwd.to_string_lossy()) {
                            Ok(s) => {
                                resolver.active_sessions.insert(channel_id.to_string(), s.session_id.clone());
                                format!("Switched to project '{}' (new session [{}])", p.name, &s.session_id[..8])
                            }
                            Err(e) => format!("Error creating session: {}", e),
                        }
                    }
                }
                None => format!("Project '{}' not found.", target),
            }
        }
    };

    let _ = outbound.send(channel_id, OutboundMsg::StreamPart(channel_id.to_string(), reply)).await;
    let _ = outbound.send(channel_id, OutboundMsg::StreamEnd(channel_id.to_string())).await;
}

/// Legacy worker: runs without session management (fallback if DB fails to open).
async fn run_worker_legacy<T>(
    mut inbound_rx: mpsc::Receiver<InboundMessage>,
    outbound: Arc<OutboundHub<T>>,
    busy_set: Arc<DashMap<String, ()>>,
    feishu_transport: Option<Arc<crate::im::channels::feishu::FeishuTransport>>,
    working_dir: &Path,
) where
    T: crate::im::transport::ImTransport + 'static,
{
    while let Some(msg) = inbound_rx.recv().await {
        let channel_id = msg.channel_id.clone();
        busy_set.insert(channel_id.clone(), ());

        let channel_id_segment = channel_id.clone();
        let tx = outbound.sender_for(&channel_id_segment);

        let job_name = msg.text.chars().take(50).collect::<String>();
        let job_name = if job_name.is_empty() { "IM".into() } else { job_name };
        let job = match crate::workspace::create_job(working_dir, job_name, String::new()) {
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
        let cwd = match crate::workspace::job_workspace_path(working_dir, &job_id) {
            Some(p) => p,
            None => {
                let _ = outbound.send(&channel_id, OutboundMsg::StreamPart(channel_id.clone(), "Error: job path not found".into())).await;
                let _ = outbound.send(&channel_id, OutboundMsg::StreamEnd(channel_id.clone())).await;
                busy_set.remove(&channel_id);
                continue;
            }
        };

        let mut prompt = msg.text.clone();
        if !msg.attachments.is_empty() {
            if let Some(ref ft) = feishu_transport {
                let downloaded = download_attachments(&msg.attachments, &cwd, ft).await;
                if !downloaded.is_empty() {
                    let file_list: Vec<String> = downloaded.iter().map(|(name, _)| name.clone()).collect();
                    prompt = format!("{}\n\n[Attached files in current directory: {}]", prompt, file_list.join(", "));
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

        let result = headless::run_claude_prompt_to_stream_parts(&prompt, send_segment, Some(cwd), None).await;
        if let Err(e) = result {
            let _ = outbound.send(&channel_id, OutboundMsg::StreamPart(channel_id.clone(), format!("Error: {}", e))).await;
        }
        let _ = outbound.send(&channel_id, OutboundMsg::StreamEnd(channel_id.clone())).await;
        busy_set.remove(&channel_id);
    }
}
