//! Slash commands handled by code (no AI).
//! Legacy module — command parsing is now in router.rs, handling in worker.rs.
//! Kept for backward compatibility with web_server /list-project endpoint.

use std::path::Path;

use crate::config;
use crate::workspace;

/// Trim and check if the message is a slash command.
pub fn is_list_project(prompt: &str) -> bool {
    prompt.trim().eq_ignore_ascii_case("/list-project")
}

/// Handle /list-project: list jobs in workspace, each as a clickable preview link.
/// Returns the message body to send (plain text with one URL per line).
pub fn format_list_projects(working_dir: &Path) -> String {
    let jobs = workspace::list_jobs(working_dir);
    let base = config::preview_base_url();

    if jobs.is_empty() {
        return "No projects yet. Start a conversation and ask for a page to be created.".to_string();
    }

    let lines: Vec<String> = jobs
        .into_iter()
        .map(|j| {
            let link = match &base {
                Some(b) => format!("{}/preview/{}", b.trim_end_matches('/'), j.job_id),
                None => format!("/preview/{}", j.job_id),
            };
            format!("{} — {}", j.name, link)
        })
        .collect();

    lines.join("\n")
}
