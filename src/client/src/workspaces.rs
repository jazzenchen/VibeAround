use serde::{Deserialize, Serialize};

use crate::error::encode_body;
use crate::http::{AuthRequirement, HttpMethod, RequestSpec, ResponseSpec};
use crate::Result;

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct WorkspaceItem {
    pub path: String,
    pub is_default: bool,
    pub is_builtin: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct WorkspacesResponse {
    pub workspaces: Vec<WorkspaceItem>,
    pub default_workspace: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct CreateWorkspaceResponse {
    pub workspace: WorkspaceItem,
    pub workspaces: Vec<WorkspaceItem>,
    pub default_workspace: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct WorkspacePathBody<'a> {
    path: &'a str,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct WorkspaceOrderBody<'a> {
    paths: &'a [&'a str],
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct CreateWorkspaceBody<'a> {
    name: &'a str,
}

pub fn list() -> RequestSpec {
    RequestSpec::new(
        HttpMethod::Get,
        "/api/workspaces",
        AuthRequirement::BearerToken,
    )
}

pub fn decode_list(response: ResponseSpec) -> Result<WorkspacesResponse> {
    response.decode()
}

pub fn add(path: &str) -> Result<RequestSpec> {
    Ok(RequestSpec::new(
        HttpMethod::Post,
        "/api/workspaces",
        AuthRequirement::BearerToken,
    )
    .with_body(encode_body(WorkspacePathBody { path })?))
}

pub fn create(name: &str) -> Result<RequestSpec> {
    Ok(RequestSpec::new(
        HttpMethod::Post,
        "/api/workspaces/create",
        AuthRequirement::BearerToken,
    )
    .with_body(encode_body(CreateWorkspaceBody { name })?))
}

pub fn decode_create(response: ResponseSpec) -> Result<CreateWorkspaceResponse> {
    response.decode()
}

pub fn remove(path: &str) -> Result<RequestSpec> {
    Ok(RequestSpec::new(
        HttpMethod::Post,
        "/api/workspaces/remove",
        AuthRequirement::BearerToken,
    )
    .with_body(encode_body(WorkspacePathBody { path })?))
}

pub fn reorder(paths: &[&str]) -> Result<RequestSpec> {
    Ok(RequestSpec::new(
        HttpMethod::Put,
        "/api/workspaces/order",
        AuthRequirement::BearerToken,
    )
    .with_body(encode_body(WorkspaceOrderBody { paths })?))
}

pub fn set_default(path: &str) -> Result<RequestSpec> {
    Ok(RequestSpec::new(
        HttpMethod::Put,
        "/api/workspaces/default",
        AuthRequirement::BearerToken,
    )
    .with_body(encode_body(WorkspacePathBody { path })?))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn add_workspace_builds_expected_body() {
        let request = add("/Users/jazzen/project").expect("request");
        assert_eq!(request.method, HttpMethod::Post);
        assert_eq!(request.path, "/api/workspaces");
        assert_eq!(request.auth, AuthRequirement::BearerToken);
        assert_eq!(
            request.body,
            Some(json!({ "path": "/Users/jazzen/project" }))
        );
    }

    #[test]
    fn reorder_workspace_builds_expected_body() {
        let request = reorder(&["/a", "/b"]).expect("request");
        assert_eq!(request.method, HttpMethod::Put);
        assert_eq!(request.path, "/api/workspaces/order");
        assert_eq!(request.body, Some(json!({ "paths": ["/a", "/b"] })));
    }
}
