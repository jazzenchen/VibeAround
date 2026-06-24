use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum PtyTool {
    Generic,
    Claude,
    Codex,
    Pi,
    Gemini,
    #[serde(rename = "opencode")]
    OpenCode,
    Cursor,
    Kiro,
    #[serde(rename = "qwen-code")]
    QwenCode,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum PtyRunState {
    Running { tool: PtyTool },
    Exited { tool: PtyTool, exit_code: u32 },
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct SessionListItem {
    pub session_id: String,
    pub tool: PtyTool,
    pub status: PtyRunState,
    pub created_at: u64,
    pub project_path: Option<String>,
    pub profile_id: Option<String>,
    pub profile_label: Option<String>,
    pub launch_target: Option<String>,
    pub tmux_session: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct CreateSessionResponse {
    pub session_id: String,
    pub tool: PtyTool,
    pub created_at: u64,
    pub project_path: Option<String>,
    pub profile_id: Option<String>,
    pub profile_label: Option<String>,
    pub launch_target: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct LaunchSessionInfo {
    pub agent_id: String,
    pub session_id: String,
    pub title: String,
    pub workspace: String,
    pub updated_at: u64,
    pub short_id: String,
    pub archived: bool,
    pub active: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct TmuxSessionsResponse {
    pub available: bool,
    pub sessions: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize)]
pub struct CreateSessionBody<'a> {
    pub tool: Option<PtyTool>,
    pub profile_id: Option<&'a str>,
    pub launch_target: Option<&'a str>,
    pub resume_session_id: Option<&'a str>,
    pub project_path: Option<&'a str>,
    pub tmux_session: Option<&'a str>,
    pub theme: Option<&'a str>,
    pub cols: Option<u16>,
    pub rows: Option<u16>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct LaunchSessionsQuery<'a> {
    pub workspace_path: Option<&'a str>,
    pub include_archived: Option<bool>,
    pub limit: Option<usize>,
}
