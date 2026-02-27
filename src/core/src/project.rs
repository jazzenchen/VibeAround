//! Project management: CRUD backed by SQLite `projects` table.
//! A project represents a code repository / working directory.
//! Each project's files live under {working_dir}/workspaces/{project_id}/.

use std::path::{Path, PathBuf};

use rusqlite::Connection;

const WORKSPACES_DIR: &str = "workspaces";

#[derive(Debug, Clone)]
pub struct Project {
    pub project_id: String,
    pub name: String,
    pub path: String,
    pub created_at: String,
}

/// Create a new project, its workspace directory, and insert into DB.
pub fn create_project(conn: &Connection, working_dir: &Path, name: String) -> rusqlite::Result<Project> {
    let project_id = uuid::Uuid::new_v4().to_string();
    let rel_path = format!("./{}/{}", WORKSPACES_DIR, project_id);
    let abs_path = working_dir.join(WORKSPACES_DIR).join(&project_id);
    let _ = std::fs::create_dir_all(&abs_path);
    let now = chrono::Utc::now().to_rfc3339();

    conn.execute(
        "INSERT INTO projects (project_id, name, path, created_at) VALUES (?1, ?2, ?3, ?4)",
        rusqlite::params![project_id, name, rel_path, now],
    )?;

    Ok(Project { project_id, name, path: rel_path, created_at: now })
}

/// Get a project by ID.
pub fn get_project(conn: &Connection, project_id: &str) -> rusqlite::Result<Option<Project>> {
    let mut stmt = conn.prepare(
        "SELECT project_id, name, path, created_at FROM projects WHERE project_id = ?1",
    )?;
    let mut rows = stmt.query(rusqlite::params![project_id])?;
    match rows.next()? {
        Some(row) => Ok(Some(Project {
            project_id: row.get(0)?,
            name: row.get(1)?,
            path: row.get(2)?,
            created_at: row.get(3)?,
        })),
        None => Ok(None),
    }
}

/// List all projects.
pub fn list_projects(conn: &Connection) -> rusqlite::Result<Vec<Project>> {
    let mut stmt = conn.prepare(
        "SELECT project_id, name, path, created_at FROM projects ORDER BY created_at DESC",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok(Project {
            project_id: row.get(0)?,
            name: row.get(1)?,
            path: row.get(2)?,
            created_at: row.get(3)?,
        })
    })?;
    rows.collect()
}

/// Delete a project record (does NOT remove files on disk).
pub fn delete_project(conn: &Connection, project_id: &str) -> rusqlite::Result<()> {
    conn.execute("DELETE FROM projects WHERE project_id = ?1", rusqlite::params![project_id])?;
    Ok(())
}

/// Absolute path to a project's workspace directory.
pub fn project_workspace_path(working_dir: &Path, project_id: &str) -> PathBuf {
    working_dir.join(WORKSPACES_DIR).join(project_id)
}

/// Ensure the workspaces root directory exists.
pub fn ensure_workspace_dirs(working_dir: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(working_dir.join(WORKSPACES_DIR))
}
