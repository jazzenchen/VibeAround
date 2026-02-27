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

/// Move all files from staging dir into session cache dir. Replaces existing files with same name.
/// Returns list of (file_name, final_path) for prompt.
fn move_staging_to_session_cache(staging_dir: &Path, session_cwd: &Path) -> Vec<(String, String)> {
    let cache_dir = session_cwd.join(".cache");
    let _ = std::fs::create_dir_all(&cache_dir);
    let mut result = Vec::new();
    let Ok(entries) = std::fs::read_dir(staging_dir) else { return result };
    for e in entries.filter_map(|e| e.ok()) {
        let path = e.path();
        if !path.is_file() {
            continue;
        }
        let name = match path.file_name().and_then(|n| n.to_str()) {
            Some(n) => n.to_string(),
            None => continue,
        };
        let dest = cache_dir.join(&name);
        if std::fs::rename(&path, &dest).is_err() {
            if std::fs::copy(&path, &dest).is_ok() && std::fs::remove_file(&path).is_ok() {
                result.push((name, dest.to_string_lossy().to_string()));
            }
        } else {
            result.push((name, dest.to_string_lossy().to_string()));
        }
    }
    let _ = std::fs::remove_dir(staging_dir);
    result
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

    // Open SQLite database (required — no fallback)
    let db = match crate::db::open_db(&working_dir) {
        Ok(conn) => Arc::new(Mutex::new(conn)),
        Err(e) => {
            eprintln!("[VibeAround][im][worker] FATAL: failed to open database: {}", e);
            panic!("Cannot start IM worker without database: {}", e);
        }
    };

    let runner = headless::ClaudeRunner;
    let resolver = SessionResolver::new(db.clone());
    let context_mgr = ContextManager::new(db.clone());

    while let Some(msg) = inbound_rx.recv().await {
        let channel_id = msg.channel_id.clone();
        busy_set.insert(channel_id.clone(), ());

        // 1. If message has attachments, download to staging dir first (workspace root .cache/incoming/{id})
        let staging_dir = if !msg.attachments.is_empty() {
            if let Some(ref ft) = feishu_transport {
                let incoming_root = working_dir.join(".cache").join("incoming");
                let _ = std::fs::create_dir_all(&incoming_root);
                let request_id = uuid::Uuid::new_v4().to_string();
                let staging = incoming_root.join(&request_id);
                let _ = std::fs::create_dir_all(&staging);
                let downloaded = download_attachments(&msg.attachments, &staging, ft).await;
                if downloaded.is_empty() {
                    let _ = std::fs::remove_dir(staging);
                    None
                } else {
                    Some(staging)
                }
            } else {
                None
            }
        } else {
            None
        };

        let action = resolver.resolve(
            &channel_id,
            &msg.text,
            msg.parent_id.as_deref(),
            &runner,
        ).await;

        /// Build pre_downloaded list from staging and session cwd; move files into session .cache.
        fn prepare_pre_downloaded(staging_dir: Option<&PathBuf>, session_cwd: &Path) -> Option<Vec<(String, String)>> {
            let st = staging_dir?;
            let list = move_staging_to_session_cache(st, session_cwd);
            if list.is_empty() {
                None
            } else {
                Some(list)
            }
        }

        match action {
            SessionAction::Command(cmd) => {
                if let Some(ref st) = staging_dir {
                    let _ = std::fs::remove_dir_all(st);
                }
                handle_command(cmd, &channel_id, &outbound, &resolver, &db, &working_dir).await;
                busy_set.remove(&channel_id);
                continue;
            }
            SessionAction::Continue(session) => {
                let pre = prepare_pre_downloaded(staging_dir.as_ref(), Path::new(&session.cwd));
                run_with_session(
                    &msg, &session, &channel_id, &outbound, &runner, &context_mgr,
                    &db, &working_dir, feishu_transport.as_deref(), None, pre,
                ).await;
            }
            SessionAction::SwitchTo { session, reason } => {
                let _ = outbound.send(&channel_id, OutboundMsg::StreamPart(
                    channel_id.clone(),
                    format!("[Switched to project: {}]", reason),
                )).await;
                let pre = prepare_pre_downloaded(staging_dir.as_ref(), Path::new(&session.cwd));
                run_with_session(
                    &msg, &session, &channel_id, &outbound, &runner, &context_mgr,
                    &db, &working_dir, feishu_transport.as_deref(), None, pre,
                ).await;
            }
            SessionAction::ChatOnly(Some(session)) => {
                let pre = prepare_pre_downloaded(staging_dir.as_ref(), Path::new(&session.cwd));
                run_with_session(
                    &msg, &session, &channel_id, &outbound, &runner, &context_mgr,
                    &db, &working_dir, feishu_transport.as_deref(), None, pre,
                ).await;
            }
            SessionAction::ChatOnly(None) | SessionAction::UseDefaultSession => {
                match resolver.get_or_create_default_session(&working_dir) {
                    Ok(session) => {
                        let pre = prepare_pre_downloaded(staging_dir.as_ref(), Path::new(&session.cwd));
                        run_with_session(
                            &msg, &session, &channel_id, &outbound, &runner, &context_mgr,
                            &db, &working_dir, feishu_transport.as_deref(), None, pre,
                        ).await;
                    }
                    Err(e) => {
                        if let Some(ref st) = staging_dir {
                            let _ = std::fs::remove_dir_all(st);
                        }
                        eprintln!("[VibeAround][im][worker] get_or_create_default_session error: {}", e);
                        let _ = outbound.send(&channel_id, OutboundMsg::StreamPart(
                            channel_id.to_string(), format!("Error: {}", e),
                        )).await;
                        let _ = outbound.send(&channel_id, OutboundMsg::StreamEnd(channel_id.to_string())).await;
                    }
                }
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
                        let pre = prepare_pre_downloaded(staging_dir.as_ref(), Path::new(&session.cwd));
                        run_with_session(
                            &msg, &session, &channel_id, &outbound, &runner, &context_mgr,
                            &db, &working_dir, feishu_transport.as_deref(), None, pre,
                        ).await;
                    }
                    Err(e) => {
                        if let Some(ref st) = staging_dir {
                            let _ = std::fs::remove_dir_all(st);
                        }
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
                                eprintln!(
                                    "[VibeAround][im][worker] active_sessions.insert channel={} session_id={} project_id={}",
                                    channel_id, session.session_id, session.project_id
                                );
                                let _ = outbound.send(&channel_id, OutboundMsg::StreamPart(
                                    channel_id.clone(),
                                    format!("[New project '{}': {}]", suggested_name, reason),
                                )).await;
                                let pre = prepare_pre_downloaded(staging_dir.as_ref(), Path::new(&session.cwd));
                                run_with_session(
                                    &msg, &session, &channel_id, &outbound, &runner, &context_mgr,
                                    &db, &working_dir, feishu_transport.as_deref(), None, pre,
                                ).await;
                            }
                            Err(e) => {
                                if let Some(ref st) = staging_dir {
                                    let _ = std::fs::remove_dir_all(st);
                                }
                                let _ = outbound.send(&channel_id, OutboundMsg::StreamPart(
                                    channel_id.clone(), format!("Error creating session: {}", e),
                                )).await;
                                let _ = outbound.send(&channel_id, OutboundMsg::StreamEnd(channel_id.clone())).await;
                            }
                        }
                    }
                    Err(e) => {
                        if let Some(ref st) = staging_dir {
                            let _ = std::fs::remove_dir_all(st);
                        }
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

/// Run Claude with a resolved session. Attachments are either pre_downloaded (moved from staging to session .cache) or downloaded here.
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
    pre_downloaded: Option<Vec<(String, String)>>,
) where
    T: crate::im::transport::ImTransport + 'static,
{
    let cwd = PathBuf::from(&session.cwd);
    let _ = std::fs::create_dir_all(&cwd);

    let mut prompt = msg.text.clone();
    if let Some(ref files) = pre_downloaded {
        if !files.is_empty() {
            let file_list: Vec<String> = files.iter()
                .map(|(name, path)| format!("{} ({})", name, path))
                .collect();
            prompt = format!(
                "{}\n\n[The user uploaded the following files. They have been saved to the .cache/ directory in the current workspace. \
Review the files and decide how to handle them based on the user's request. \
Files: {}]",
                prompt,
                file_list.join(", ")
            );
        }
    } else if !msg.attachments.is_empty() {
        if let Some(ft) = feishu_transport {
            let cache_dir = cwd.join(".cache");
            let _ = std::fs::create_dir_all(&cache_dir);
            let downloaded = download_attachments(&msg.attachments, &cache_dir, ft).await;
            if !downloaded.is_empty() {
                let file_list: Vec<String> = downloaded.iter()
                    .map(|(name, path)| format!("{} ({})", name, path))
                    .collect();
                prompt = format!(
                    "{}\n\n[The user uploaded the following files. They have been saved to the .cache/ directory in the current workspace. \
Review the files and decide how to handle them based on the user's request. \
Files: {}]",
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
            match resolver.get_or_create_default_session(working_dir) {
                Ok(session) => {
                    resolver.active_sessions.insert(channel_id.to_string(), session.session_id.clone());
                    "Switched to default session (general). Send your next message to continue.".to_string()
                }
                Err(e) => format!("Error: {}", e),
            }
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
            if target.trim().eq_ignore_ascii_case("default") {
                match resolver.get_or_create_default_session(working_dir) {
                    Ok(session) => {
                        resolver.active_sessions.insert(channel_id.to_string(), session.session_id.clone());
                        format!("Switched to default session (general) [{}]", &session.session_id[..8])
                    }
                    Err(e) => format!("Error: {}", e),
                }
            } else {
                let conn = db.lock().unwrap();
                let sessions = session_store::list_all_sessions(&conn).unwrap_or_default();
                let found = sessions.iter().find(|s| s.session_id.starts_with(target.trim()));
                match found {
                    Some(s) => {
                        resolver.active_sessions.insert(channel_id.to_string(), s.session_id.clone());
                        format!("Switched to session [{}]", &s.session_id[..8])
                    }
                    None => format!("Session '{}' not found. Use /switch default for the default (general) session.", target.trim()),
                }
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
