use crate::error::encode_body;
use crate::http::{join_path, AuthRequirement, HttpMethod, RequestSpec, ResponseSpec};
use crate::Result;

use super::{CreateSessionBody, CreateSessionResponse, SessionListItem, TmuxSessionsResponse};

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
}
