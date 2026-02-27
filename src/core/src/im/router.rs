//! Session router: decides which session to use for each incoming IM message.
//! Routes based on: slash commands > quote-reply (parent_id) > Runner classify_intent.

use std::sync::{Arc, Mutex};

use dashmap::DashMap;
use rusqlite::Connection;

use crate::headless::{
    ClassifyContext, CurrentSessionInfo, HeadlessRunner, IntentResult, ProjectInfo,
};
use crate::im::session as session_store;
use crate::im::session::ChatSession;
use crate::project;

/// Action the worker should take after routing.
pub enum SessionAction {
    /// Continue the current session (no switch).
    Continue(ChatSession),
    /// Switch to a different session (from quote-reply or classify_intent).
    SwitchTo { session: ChatSession, reason: String },
    /// Create a new session under an existing project.
    CreateNew { project_id: String, reason: String },
    /// Create a new project and a new session.
    CreateNewProject { suggested_name: String, reason: String },
    /// A slash command was detected; worker should handle it directly.
    Command(CommandAction),
}

/// Slash command actions.
pub enum CommandAction {
    NewSession,
    SwitchSession(String),
    ListSessions,
    ProjectNew(String),
    ProjectUse(String),
    ProjectList,
    ListProjects,
}

pub struct SessionResolver {
    db: Arc<Mutex<Connection>>,
    /// channel_id -> session_id (in-memory, not persisted)
    pub active_sessions: Arc<DashMap<String, String>>,
}

impl SessionResolver {
    pub fn new(db: Arc<Mutex<Connection>>) -> Self {
        Self {
            db,
            active_sessions: Arc::new(DashMap::new()),
        }
    }

    /// Main entry: resolve an incoming message to a SessionAction.
    pub async fn resolve(
        &self,
        channel_id: &str,
        text: &str,
        parent_id: Option<&str>,
        runner: &dyn HeadlessRunner,
    ) -> SessionAction {
        // 1. Slash commands
        if let Some(cmd) = parse_command(text) {
            return SessionAction::Command(cmd);
        }

        // 2. Quote-reply: look up parent_id in message_index
        if let Some(pid) = parent_id {
            let session_id = {
                let conn = self.db.lock().unwrap();
                session_store::lookup_message(&conn, channel_id, pid).ok().flatten()
            };
            if let Some(sid) = session_id {
                let session = {
                    let conn = self.db.lock().unwrap();
                    session_store::get_session(&conn, &sid).ok().flatten()
                };
                if let Some(session) = session {
                    // Permanently switch this channel to the quoted session
                    self.active_sessions.insert(channel_id.to_string(), session.session_id.clone());
                    return SessionAction::SwitchTo {
                        session,
                        reason: "Switched via quote-reply".to_string(),
                    };
                }
            }
        }

        // 3. Call Runner classify_intent
        let (projects, current_session_info) = self.build_classify_context(channel_id);
        let context = ClassifyContext {
            user_prompt: text.to_string(),
            projects: projects.clone(),
            current_session: current_session_info,
        };

        match runner.classify_intent(&context, None).await {
            Ok(IntentResult::ContinueCurrent { .. }) => {
                // Continue current session if one exists
                if let Some(entry) = self.active_sessions.get(channel_id) {
                    let sid = entry.value().clone();
                    let session = {
                        let conn = self.db.lock().unwrap();
                        session_store::get_session(&conn, &sid).ok().flatten()
                    };
                    if let Some(session) = session {
                        return SessionAction::Continue(session);
                    }
                }
                // No active session — fall through to create new
                SessionAction::CreateNewProject {
                    suggested_name: "untitled".to_string(),
                    reason: "No active session".to_string(),
                }
            }
            Ok(IntentResult::ExistingProject { project_id, reason }) => {
                // Check if this channel already has an active session for this project
                if let Some(entry) = self.active_sessions.get(channel_id) {
                    let sid = entry.value().clone();
                    let session = {
                        let conn = self.db.lock().unwrap();
                        session_store::get_session(&conn, &sid).ok().flatten()
                    };
                    if let Some(session) = session {
                        if session.project_id == project_id {
                            return SessionAction::Continue(session);
                        }
                    }
                }
                // Find the most recent session for this project
                let session = {
                    let conn = self.db.lock().unwrap();
                    session_store::list_sessions_by_project(&conn, &project_id)
                        .ok()
                        .and_then(|v| v.into_iter().next())
                };
                if let Some(session) = session {
                    self.active_sessions.insert(channel_id.to_string(), session.session_id.clone());
                    SessionAction::SwitchTo { session, reason }
                } else {
                    SessionAction::CreateNew { project_id, reason }
                }
            }
            Ok(IntentResult::NewProject { suggested_name, reason }) => {
                SessionAction::CreateNewProject { suggested_name, reason }
            }
            Err(_) => {
                // Fallback: if classify fails, try to continue current session
                if let Some(entry) = self.active_sessions.get(channel_id) {
                    let sid = entry.value().clone();
                    let session = {
                        let conn = self.db.lock().unwrap();
                        session_store::get_session(&conn, &sid).ok().flatten()
                    };
                    if let Some(session) = session {
                        return SessionAction::Continue(session);
                    }
                }
                SessionAction::CreateNewProject {
                    suggested_name: "untitled".to_string(),
                    reason: "New conversation".to_string(),
                }
            }
        }
    }

    fn build_classify_context(&self, channel_id: &str) -> (Vec<ProjectInfo>, Option<CurrentSessionInfo>) {
        let conn = self.db.lock().unwrap();
        let projects = project::list_projects(&conn)
            .unwrap_or_default()
            .into_iter()
            .map(|p| ProjectInfo {
                project_id: p.project_id,
                name: p.name,
                path: p.path,
            })
            .collect::<Vec<_>>();

        let current_session_info = self
            .active_sessions
            .get(channel_id)
            .and_then(|entry| {
                let sid = entry.value().clone();
                let session = session_store::get_session(&conn, &sid).ok().flatten()?;
                let project = project::get_project(&conn, &session.project_id).ok().flatten()?;
                Some(CurrentSessionInfo {
                    session_id: session.session_id,
                    project_name: project.name,
                    recent_summary: session.summary.unwrap_or_default(),
                })
            });

        (projects, current_session_info)
    }
}

/// Parse slash commands from message text.
fn parse_command(text: &str) -> Option<CommandAction> {
    let text = text.trim();
    if text.eq_ignore_ascii_case("/new") {
        return Some(CommandAction::NewSession);
    }
    if text.eq_ignore_ascii_case("/sessions") {
        return Some(CommandAction::ListSessions);
    }
    if text.eq_ignore_ascii_case("/list-project") {
        return Some(CommandAction::ListProjects);
    }
    if let Some(rest) = text.strip_prefix("/switch ").or_else(|| text.strip_prefix("/switch\t")) {
        let target = rest.trim().to_string();
        if !target.is_empty() {
            return Some(CommandAction::SwitchSession(target));
        }
    }
    if let Some(rest) = text.strip_prefix("/project ").or_else(|| text.strip_prefix("/project\t")) {
        let parts: Vec<&str> = rest.trim().splitn(2, ' ').collect();
        match parts.first().map(|s| s.to_lowercase()).as_deref() {
            Some("new") => {
                let name = parts.get(1).unwrap_or(&"untitled").trim().to_string();
                return Some(CommandAction::ProjectNew(name));
            }
            Some("use") => {
                let target = parts.get(1).unwrap_or(&"").trim().to_string();
                if !target.is_empty() {
                    return Some(CommandAction::ProjectUse(target));
                }
            }
            Some("list") => return Some(CommandAction::ProjectList),
            _ => {}
        }
    }
    None
}
