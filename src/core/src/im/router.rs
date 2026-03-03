//! Session router: decides which session to use for each incoming IM message.
//! Routes based on: slash commands > quote-reply (parent_id) > Runner classify_intent.
//! A single default session (general project) is used when no session; in-memory only, shared by all channels.

use std::path::Path;
use std::sync::{Arc, Mutex, RwLock};

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
    /// Chat-only: no project/session create or switch. Some(session) = use it; None = use default session.
    ChatOnly(Option<ChatSession>),
    /// Use the channel's default session (general project). Worker resolves via get_or_create_default_session.
    UseDefaultSession,
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
    /// Switch the active agent backend (e.g. `/cli claude`, `/cli gemini`).
    SwitchAgent(crate::agent::AgentKind),
}

pub struct SessionResolver {
    db: Arc<Mutex<Connection>>,
    /// channel_id -> session_id (in-memory, not persisted)
    pub active_sessions: Arc<DashMap<String, String>>,
    /// Single default session (general project), shared by all channels. In-memory only; lifecycle ends when process exits.
    default_session_id: Arc<RwLock<Option<String>>>,
}

impl SessionResolver {
    pub fn new(db: Arc<Mutex<Connection>>) -> Self {
        Self {
            db,
            active_sessions: Arc::new(DashMap::new()),
            default_session_id: Arc::new(RwLock::new(None)),
        }
    }

    /// Get or create the single default session (under general project). Shared by all channels.
    pub fn get_or_create_default_session(&self, working_dir: &Path) -> Result<ChatSession, String> {
        {
            let guard = self.default_session_id.read().unwrap();
            if let Some(ref sid) = *guard {
                let conn = self.db.lock().unwrap();
                if let Ok(Some(session)) = session_store::get_session(&conn, sid) {
                    return Ok(session);
                }
            }
        }
        let (session_id, session) = {
            let conn = self.db.lock().unwrap();
            let proj = project::ensure_general_project(&conn, working_dir).map_err(|e| e.to_string())?;
            let cwd = project::project_workspace_path(working_dir, &proj.project_id);
            let _ = std::fs::create_dir_all(&cwd);
            let session = session_store::create_session(&conn, &proj.project_id, &cwd.to_string_lossy())
                .map_err(|e| e.to_string())?;
            (session.session_id.clone(), session)
        };
        {
            let mut guard = self.default_session_id.write().unwrap();
            *guard = Some(session_id);
        }
        Ok(session)
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
        let active_session_id = self.active_sessions.get(channel_id).map(|e| e.value().clone());
        eprintln!(
            "[VibeAround][im][router] channel={} before_classify active_session={} projects_count={}",
            channel_id,
            active_session_id.as_deref().unwrap_or("None"),
            projects.len()
        );
        let context = ClassifyContext {
            user_prompt: text.to_string(),
            projects: projects.clone(),
            current_session: current_session_info,
        };

        const CLASSIFY_TIMEOUT_SECS: u64 = 60;
        eprintln!(
            "[VibeAround][im][router] channel={} calling classify_intent (timeout {}s)...",
            channel_id, CLASSIFY_TIMEOUT_SECS
        );
        let intent_result = match tokio::time::timeout(
            std::time::Duration::from_secs(CLASSIFY_TIMEOUT_SECS),
            runner.classify_intent(&context, None),
        )
        .await
        {
            Ok(Ok(r)) => Ok(r),
            Ok(Err(e)) => Err(e),
            Err(_) => {
                eprintln!(
                    "[VibeAround][im][router] channel={} classify_intent timed out after {}s, using default session",
                    channel_id, CLASSIFY_TIMEOUT_SECS
                );
                return SessionAction::UseDefaultSession;
            }
        };

        // Log intent classification result for debugging
        match &intent_result {
            Ok(IntentResult::ContinueCurrent { reason }) => {
                eprintln!(
                    "[VibeAround][im][router] channel={} intent=ContinueCurrent reason=\"{}\"",
                    channel_id, reason
                );
            }
            Ok(IntentResult::ExistingProject { project_id, reason }) => {
                eprintln!(
                    "[VibeAround][im][router] channel={} intent=ExistingProject project_id={} reason=\"{}\"",
                    channel_id, project_id, reason
                );
            }
            Ok(IntentResult::NewProject { suggested_name, reason }) => {
                eprintln!(
                    "[VibeAround][im][router] channel={} intent=NewProject suggested_name=\"{}\" reason=\"{}\"",
                    channel_id, suggested_name, reason
                );
            }
            Ok(IntentResult::ChatOnly { reason }) => {
                eprintln!(
                    "[VibeAround][im][router] channel={} intent=ChatOnly reason=\"{}\"",
                    channel_id, reason
                );
            }
            Err(e) => {
                eprintln!(
                    "[VibeAround][im][router] channel={} intent=Err error=\"{}\"",
                    channel_id, e
                );
            }
        }

        match intent_result {
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
                // No active session — use channel's default session (general project)
                SessionAction::UseDefaultSession
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
            Ok(IntentResult::ChatOnly { .. }) => {
                // No project/session create or switch. Use current session if any.
                if let Some(entry) = self.active_sessions.get(channel_id) {
                    let sid = entry.value().clone();
                    let session = {
                        let conn = self.db.lock().unwrap();
                        session_store::get_session(&conn, &sid).ok().flatten()
                    };
                    if let Some(session) = session {
                        return SessionAction::ChatOnly(Some(session));
                    }
                }
                SessionAction::ChatOnly(None)
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
                SessionAction::UseDefaultSession
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
    // /cli <agent> — switch active agent backend
    if let Some(rest) = text.strip_prefix("/cli ").or_else(|| text.strip_prefix("/cli\t")) {
        let agent_name = rest.trim();
        if let Some(kind) = crate::agent::AgentKind::from_str_loose(agent_name) {
            return Some(CommandAction::SwitchAgent(kind));
        }
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

/// Public wrapper for `parse_command` — used by worker.rs to intercept commands before routing.
pub fn parse_command_public(text: &str) -> Option<CommandAction> {
    parse_command(text)
}
