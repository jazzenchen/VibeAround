use serde::Deserialize;

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct LaunchSessionInfo {
    pub agent_id: String,
    pub host_agent_id: Option<String>,
    pub host_profile_id: Option<String>,
    pub host_profile_label: Option<String>,
    pub host_provider: Option<String>,
    pub host_provider_label: Option<String>,
    pub session_id: String,
    pub title: String,
    pub workspace: String,
    pub updated_at: u64,
    pub short_id: String,
    pub archived: bool,
    pub active: bool,
    pub thread_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct LaunchSessionsQuery<'a> {
    pub workspace_path: Option<&'a str>,
    pub include_archived: Option<bool>,
    pub limit: Option<usize>,
}
