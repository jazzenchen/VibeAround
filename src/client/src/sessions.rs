use serde::{Deserialize, Serialize};

use crate::error::encode_body;
use crate::http::{
    join_path, path_segment, path_with_query, AuthRequirement, HttpMethod, RequestSpec,
    ResponseSpec,
};
use crate::Result;

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

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct LaunchSessionsBatchBody<'a> {
    agent_ids: &'a [&'a str],
    workspace_paths: &'a [&'a str],
    include_archived: Option<bool>,
    limit: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct LaunchSessionArchiveBody<'a> {
    workspace_path: Option<&'a str>,
}

pub fn list_tmux() -> RequestSpec {
    RequestSpec::new(
        HttpMethod::Get,
        "/api/tmux/sessions",
        AuthRequirement::BearerToken,
    )
}

pub fn decode_list_tmux(response: ResponseSpec) -> Result<TmuxSessionsResponse> {
    response.decode()
}

pub fn list() -> RequestSpec {
    RequestSpec::new(
        HttpMethod::Get,
        "/api/sessions",
        AuthRequirement::BearerToken,
    )
}

pub fn decode_list(response: ResponseSpec) -> Result<Vec<SessionListItem>> {
    response.decode()
}

pub fn create(body: CreateSessionBody<'_>) -> Result<RequestSpec> {
    Ok(RequestSpec::new(
        HttpMethod::Post,
        "/api/sessions",
        AuthRequirement::BearerToken,
    )
    .with_body(encode_body(body)?))
}

pub fn decode_create(response: ResponseSpec) -> Result<CreateSessionResponse> {
    response.decode()
}

pub fn delete(session_id: &str) -> RequestSpec {
    RequestSpec::new(
        HttpMethod::Delete,
        join_path("/api/sessions", session_id),
        AuthRequirement::BearerToken,
    )
}

pub fn list_launch_sessions(agent_id: &str, query: LaunchSessionsQuery<'_>) -> RequestSpec {
    let base = format!("/api/agents/{}/launch-sessions", path_segment(agent_id));
    let path = path_with_query(
        &base,
        &[
            (
                "workspace_path",
                query.workspace_path.map(ToOwned::to_owned),
            ),
            (
                "include_archived",
                query.include_archived.map(|value| value.to_string()),
            ),
            ("limit", query.limit.map(|value| value.to_string())),
        ],
    );
    RequestSpec::new(HttpMethod::Get, path, AuthRequirement::BearerToken)
}

pub fn list_launch_sessions_batch(
    agent_ids: &[&str],
    workspace_paths: &[&str],
    include_archived: Option<bool>,
    limit: Option<usize>,
) -> Result<RequestSpec> {
    Ok(RequestSpec::new(
        HttpMethod::Post,
        "/api/launch-sessions",
        AuthRequirement::BearerToken,
    )
    .with_body(encode_body(LaunchSessionsBatchBody {
        agent_ids,
        workspace_paths,
        include_archived,
        limit,
    })?))
}

pub fn decode_launch_sessions(response: ResponseSpec) -> Result<Vec<LaunchSessionInfo>> {
    response.decode()
}

pub fn archive_launch_session(
    agent_id: &str,
    session_id: &str,
    workspace_path: Option<&str>,
) -> Result<RequestSpec> {
    launch_session_archive_request(agent_id, session_id, "archive", workspace_path)
}

pub fn unarchive_launch_session(
    agent_id: &str,
    session_id: &str,
    workspace_path: Option<&str>,
) -> Result<RequestSpec> {
    launch_session_archive_request(agent_id, session_id, "unarchive", workspace_path)
}

fn launch_session_archive_request(
    agent_id: &str,
    session_id: &str,
    action: &str,
    workspace_path: Option<&str>,
) -> Result<RequestSpec> {
    Ok(RequestSpec::new(
        HttpMethod::Post,
        format!(
            "/api/agents/{}/launch-sessions/{}/{}",
            path_segment(agent_id),
            path_segment(session_id),
            action
        ),
        AuthRequirement::BearerToken,
    )
    .with_body(encode_body(LaunchSessionArchiveBody { workspace_path })?))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn create_profile_session_body_matches_server_shape() {
        let request = create(CreateSessionBody {
            profile_id: Some("p1"),
            launch_target: Some("claude"),
            cols: Some(120),
            rows: Some(40),
            ..Default::default()
        })
        .expect("request");
        assert_eq!(request.method, HttpMethod::Post);
        assert_eq!(request.path, "/api/sessions");
        assert_eq!(
            request.body,
            Some(json!({
                "tool": null,
                "profile_id": "p1",
                "launch_target": "claude",
                "project_path": null,
                "tmux_session": null,
                "theme": null,
                "cols": 120,
                "rows": 40
            }))
        );
    }

    #[test]
    fn launch_sessions_query_encodes_workspace() {
        let request = list_launch_sessions(
            "qwen-code",
            LaunchSessionsQuery {
                workspace_path: Some("/tmp/a b"),
                include_archived: Some(true),
                limit: Some(10),
            },
        );
        assert_eq!(
            request.path,
            "/api/agents/qwen-code/launch-sessions?workspace_path=%2Ftmp%2Fa%20b&include_archived=true&limit=10"
        );
    }
}
