use serde::Serialize;

use crate::error::encode_body;
use crate::http::{
    path_segment, path_with_query, AuthRequirement, HttpMethod, RequestSpec, ResponseSpec,
};
use crate::Result;

use super::{LaunchSessionInfo, LaunchSessionsQuery};

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
    use super::*;

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
