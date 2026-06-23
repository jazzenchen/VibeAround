use crate::operation::Operation;
use crate::sessions::{
    CreateSessionBody, CreateSessionResponse, LaunchSessionInfo, LaunchSessionsQuery,
    SessionListItem, TmuxSessionsResponse,
};
use crate::Result;

use super::decode_success;

pub fn sessions() -> Operation<Vec<SessionListItem>> {
    Operation::new(crate::sessions::list(), crate::sessions::decode_list)
}

pub fn session_create(body: CreateSessionBody<'_>) -> Result<Operation<CreateSessionResponse>> {
    Ok(Operation::new(
        crate::sessions::create(body)?,
        crate::sessions::decode_create,
    ))
}

pub fn session_delete(session_id: &str) -> Operation<()> {
    Operation::new(crate::sessions::delete(session_id), decode_success)
}

pub fn tmux_sessions() -> Operation<TmuxSessionsResponse> {
    Operation::new(
        crate::sessions::list_tmux(),
        crate::sessions::decode_list_tmux,
    )
}

pub fn launch_sessions(
    agent_id: &str,
    query: LaunchSessionsQuery<'_>,
) -> Operation<Vec<LaunchSessionInfo>> {
    Operation::new(
        crate::sessions::list_launch_sessions(agent_id, query),
        crate::sessions::decode_launch_sessions,
    )
}

pub fn launch_sessions_batch(
    agent_ids: &[&str],
    workspace_paths: &[&str],
    include_archived: Option<bool>,
    limit: Option<usize>,
) -> Result<Operation<Vec<LaunchSessionInfo>>> {
    Ok(Operation::new(
        crate::sessions::list_launch_sessions_batch(
            agent_ids,
            workspace_paths,
            include_archived,
            limit,
        )?,
        crate::sessions::decode_launch_sessions,
    ))
}

pub fn launch_session_archive(
    agent_id: &str,
    session_id: &str,
    workspace_path: Option<&str>,
) -> Result<Operation<()>> {
    Ok(Operation::new(
        crate::sessions::archive_launch_session(agent_id, session_id, workspace_path)?,
        decode_success,
    ))
}

pub fn launch_session_unarchive(
    agent_id: &str,
    session_id: &str,
    workspace_path: Option<&str>,
) -> Result<Operation<()>> {
    Ok(Operation::new(
        crate::sessions::unarchive_launch_session(agent_id, session_id, workspace_path)?,
        decode_success,
    ))
}

#[cfg(test)]
mod tests {
    use serde_json::{json, Value};

    use super::*;
    use crate::ResponseSpec;

    #[test]
    fn launch_session_archive_uses_success_decoder() {
        let op =
            launch_session_archive("codex", "abc/123", Some("/tmp/project")).expect("operation");
        assert_eq!(
            op.request().path,
            "/api/agents/codex/launch-sessions/abc%2F123/archive"
        );
        assert_eq!(
            op.request().body,
            Some(json!({ "workspace_path": "/tmp/project" }))
        );
        op.decode(ResponseSpec::json(204, Value::Null))
            .expect("success");
    }
}
