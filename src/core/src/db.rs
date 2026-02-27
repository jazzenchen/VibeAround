//! SQLite database: single connection, WAL mode, all tables created on open.
//! DB file lives at {working_dir}/vibearound.db.

use std::path::Path;
use rusqlite::Connection;

const DB_FILE: &str = "vibearound.db";

/// Open (or create) the SQLite database and ensure all tables exist.
pub fn open_db(working_dir: &Path) -> rusqlite::Result<Connection> {
    let db_path = working_dir.join(DB_FILE);
    let conn = Connection::open(db_path)?;
    conn.pragma_update(None, "journal_mode", "WAL")?;
    conn.pragma_update(None, "foreign_keys", "ON")?;
    create_tables(&conn)?;
    Ok(conn)
}

fn create_tables(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS projects (
            project_id  TEXT PRIMARY KEY,
            name        TEXT NOT NULL,
            path        TEXT NOT NULL,
            created_at  TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS chat_sessions (
            session_id        TEXT PRIMARY KEY,
            project_id        TEXT NOT NULL REFERENCES projects(project_id),
            runner_session_id TEXT,
            cwd               TEXT NOT NULL,
            created_at        TEXT NOT NULL,
            last_active_at    TEXT NOT NULL,
            total_tokens      INTEGER NOT NULL DEFAULT 0,
            summary           TEXT
        );

        CREATE TABLE IF NOT EXISTS message_index (
            channel_id  TEXT NOT NULL,
            message_id  TEXT NOT NULL,
            session_id  TEXT NOT NULL REFERENCES chat_sessions(session_id),
            PRIMARY KEY (channel_id, message_id)
        );
        ",
    )
}
