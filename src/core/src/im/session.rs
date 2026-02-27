//! Chat session storage: CRUD for `chat_sessions` table + `message_index` read/write.
//! A chat session is an IM conversation context (distinct from PTY sessions in session.rs).

use rusqlite::Connection;

#[derive(Debug, Clone)]
pub struct ChatSession {
    pub session_id: String,
    pub project_id: String,
    pub runner_session_id: Option<String>,
    pub cwd: String,
    pub created_at: String,
    pub last_active_at: String,
    pub total_tokens: i64,
    pub summary: Option<String>,
}

/// Create a new chat session under a project.
pub fn create_session(
    conn: &Connection,
    project_id: &str,
    cwd: &str,
) -> rusqlite::Result<ChatSession> {
    let session_id = uuid::Uuid::new_v4().to_string();
    let now = chrono::Utc::now().to_rfc3339();
    conn.execute(
        "INSERT INTO chat_sessions (session_id, project_id, cwd, created_at, last_active_at)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        rusqlite::params![session_id, project_id, cwd, now, now],
    )?;
    Ok(ChatSession {
        session_id,
        project_id: project_id.to_string(),
        runner_session_id: None,
        cwd: cwd.to_string(),
        created_at: now.clone(),
        last_active_at: now,
        total_tokens: 0,
        summary: None,
    })
}

/// Get a session by ID.
pub fn get_session(conn: &Connection, session_id: &str) -> rusqlite::Result<Option<ChatSession>> {
    let mut stmt = conn.prepare(
        "SELECT session_id, project_id, runner_session_id, cwd, created_at, last_active_at, total_tokens, summary
         FROM chat_sessions WHERE session_id = ?1",
    )?;
    let mut rows = stmt.query(rusqlite::params![session_id])?;
    match rows.next()? {
        Some(row) => Ok(Some(row_to_session(row)?)),
        None => Ok(None),
    }
}

/// List sessions belonging to a project, most recent first.
pub fn list_sessions_by_project(
    conn: &Connection,
    project_id: &str,
) -> rusqlite::Result<Vec<ChatSession>> {
    let mut stmt = conn.prepare(
        "SELECT session_id, project_id, runner_session_id, cwd, created_at, last_active_at, total_tokens, summary
         FROM chat_sessions WHERE project_id = ?1 ORDER BY last_active_at DESC",
    )?;
    let rows = stmt.query_map(rusqlite::params![project_id], |row| row_to_session(row))?;
    rows.collect()
}

/// List all sessions, most recent first.
pub fn list_all_sessions(conn: &Connection) -> rusqlite::Result<Vec<ChatSession>> {
    let mut stmt = conn.prepare(
        "SELECT session_id, project_id, runner_session_id, cwd, created_at, last_active_at, total_tokens, summary
         FROM chat_sessions ORDER BY last_active_at DESC",
    )?;
    let rows = stmt.query_map([], |row| row_to_session(row))?;
    rows.collect()
}

/// Update last_active_at to now.
pub fn touch_session(conn: &Connection, session_id: &str) -> rusqlite::Result<()> {
    let now = chrono::Utc::now().to_rfc3339();
    conn.execute(
        "UPDATE chat_sessions SET last_active_at = ?1 WHERE session_id = ?2",
        rusqlite::params![now, session_id],
    )?;
    Ok(())
}

/// Set the runner_session_id (e.g. Claude Code's session UUID).
pub fn set_runner_session_id(
    conn: &Connection,
    session_id: &str,
    runner_session_id: &str,
) -> rusqlite::Result<()> {
    conn.execute(
        "UPDATE chat_sessions SET runner_session_id = ?1 WHERE session_id = ?2",
        rusqlite::params![runner_session_id, session_id],
    )?;
    Ok(())
}

/// Update summary and total_tokens after context compression.
pub fn update_summary(
    conn: &Connection,
    session_id: &str,
    summary: &str,
    total_tokens: i64,
) -> rusqlite::Result<()> {
    conn.execute(
        "UPDATE chat_sessions SET summary = ?1, total_tokens = ?2 WHERE session_id = ?3",
        rusqlite::params![summary, total_tokens, session_id],
    )?;
    Ok(())
}

/// Increment total_tokens for a session.
pub fn add_tokens(conn: &Connection, session_id: &str, tokens: i64) -> rusqlite::Result<()> {
    conn.execute(
        "UPDATE chat_sessions SET total_tokens = total_tokens + ?1 WHERE session_id = ?2",
        rusqlite::params![tokens, session_id],
    )?;
    Ok(())
}

// -- message_index --

/// Record a bot-sent message_id -> session mapping (for quote-reply lookup).
pub fn index_message(
    conn: &Connection,
    channel_id: &str,
    message_id: &str,
    session_id: &str,
) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT OR REPLACE INTO message_index (channel_id, message_id, session_id) VALUES (?1, ?2, ?3)",
        rusqlite::params![channel_id, message_id, session_id],
    )?;
    Ok(())
}

/// Look up which session a message belongs to (for quote-reply routing).
pub fn lookup_message(
    conn: &Connection,
    channel_id: &str,
    message_id: &str,
) -> rusqlite::Result<Option<String>> {
    let mut stmt = conn.prepare(
        "SELECT session_id FROM message_index WHERE channel_id = ?1 AND message_id = ?2",
    )?;
    let mut rows = stmt.query(rusqlite::params![channel_id, message_id])?;
    match rows.next()? {
        Some(row) => Ok(Some(row.get(0)?)),
        None => Ok(None),
    }
}

fn row_to_session(row: &rusqlite::Row) -> rusqlite::Result<ChatSession> {
    Ok(ChatSession {
        session_id: row.get(0)?,
        project_id: row.get(1)?,
        runner_session_id: row.get(2)?,
        cwd: row.get(3)?,
        created_at: row.get(4)?,
        last_active_at: row.get(5)?,
        total_tokens: row.get(6)?,
        summary: row.get(7)?,
    })
}
