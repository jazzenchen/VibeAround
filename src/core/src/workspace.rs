//! Job workspace isolation and persistent registry (projects.json).
//! Jobs are stored under {working_dir}/workspaces/{job_id}/; metadata in {working_dir}/projects.json.

use std::path::{Path, PathBuf};


const PROJECTS_FILE: &str = "projects.json";
const WORKSPACES_DIR: &str = "workspaces";

/// Status of a job in the registry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum JobStatus {
    #[default]
    Pending,
    InProgress,
    Completed,
}

/// One job record in projects.json. Path is relative to working_dir (e.g. ./workspaces/{job_id}).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct JobRecord {
    pub job_id: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    /// Relative path from working_dir, e.g. "./workspaces/550e8400-..."
    pub path: String,
    pub created_at: String,
    #[serde(default)]
    pub status: JobStatus,
}

impl JobRecord {
    /// Absolute path to this job's workspace directory.
    pub fn absolute_path(&self, working_dir: &Path) -> PathBuf {
        working_dir.join(self.path.trim_start_matches("./"))
    }
}

fn projects_path(working_dir: &Path) -> PathBuf {
    working_dir.join(PROJECTS_FILE)
}

fn workspaces_root(working_dir: &Path) -> PathBuf {
    working_dir.join(WORKSPACES_DIR)
}

/// Ensure working_dir and workspaces subdir exist.
pub fn ensure_workspace_dirs(working_dir: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(workspaces_root(working_dir))
}

/// Load all job records from projects.json. Returns empty vec if file missing or invalid.
pub fn list_jobs(working_dir: &Path) -> Vec<JobRecord> {
    let path = projects_path(working_dir);
    let data = match std::fs::read_to_string(&path) {
        Ok(d) => d,
        Err(_) => return Vec::new(),
    };
    match serde_json::from_str::<Vec<JobRecord>>(&data) {
        Ok(list) => list,
        Err(_) => Vec::new(),
    }
}

/// Find a job by id.
pub fn get_job(working_dir: &Path, job_id: &str) -> Option<JobRecord> {
    list_jobs(working_dir)
        .into_iter()
        .find(|j| j.job_id == job_id)
}

/// Create a new job: generate job_id, create directory, append to projects.json (atomic write).
pub fn create_job(
    working_dir: &Path,
    name: String,
    description: String,
) -> Result<JobRecord, Box<dyn std::error::Error + Send + Sync>> {
    ensure_workspace_dirs(working_dir)?;
    let job_id = uuid::Uuid::new_v4().to_string();
    let rel_path = format!("./{}/{}", WORKSPACES_DIR, job_id);
    let abs_path = working_dir.join(WORKSPACES_DIR).join(&job_id);
    std::fs::create_dir_all(&abs_path)?;

    let created_at = chrono::Utc::now().to_rfc3339();
    let record = JobRecord {
        job_id: job_id.clone(),
        name,
        description,
        path: rel_path,
        created_at: created_at.clone(),
        status: JobStatus::Pending,
    };

    let mut jobs = list_jobs(working_dir);
    jobs.push(record.clone());
    write_projects_atomic(working_dir, &jobs)?;

    Ok(record)
}

/// Write projects.json atomically: write to .tmp then rename.
fn write_projects_atomic(
    working_dir: &Path,
    jobs: &[JobRecord],
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let path = projects_path(working_dir);
    let tmp = path.with_extension("json.tmp");
    let data = serde_json::to_string_pretty(jobs)?;
    std::fs::write(&tmp, data)?;
    std::fs::rename(tmp, path)?;
    Ok(())
}

/// Resolve job_id to absolute path. Returns None if job not found or path is outside workspaces.
pub fn job_workspace_path(working_dir: &Path, job_id: &str) -> Option<PathBuf> {
    let job = get_job(working_dir, job_id)?;
    let abs = job.absolute_path(working_dir);
    let workspaces = workspaces_root(working_dir);
    if abs.starts_with(&workspaces) {
        Some(abs)
    } else {
        None
    }
}

/// Remove job from projects.json and delete its workspace directory. Idempotent if job already gone.
pub fn delete_job(
    working_dir: &Path,
    job_id: &str,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let path = match job_workspace_path(working_dir, job_id) {
        Some(p) => p,
        None => return Ok(()),
    };
    let mut jobs = list_jobs(working_dir);
    let before = jobs.len();
    jobs.retain(|j| j.job_id != job_id);
    if jobs.len() < before {
        write_projects_atomic(working_dir, &jobs)?;
    }
    let _ = std::fs::remove_dir_all(&path);
    Ok(())
}

/// True if the directory exists and has no entries (or only . and ..).
pub fn is_dir_empty(path: &Path) -> bool {
    let Ok(entries) = std::fs::read_dir(path) else {
        return true;
    };
    entries.filter_map(|e| e.ok()).next().is_none()
}

/// True if the directory contains any file whose name ends with .html (case-insensitive).
pub fn dir_has_html(path: &Path) -> bool {
    let Ok(entries) = std::fs::read_dir(path) else {
        return false;
    };
    entries
        .filter_map(|e| e.ok())
        .any(|e| {
            e.path().file_name().and_then(|n| n.to_str()).map_or(false, |n| {
                n.to_lowercase().ends_with(".html")
            })
        })
}
