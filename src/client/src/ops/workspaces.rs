use crate::operation::Operation;
use crate::workspaces::{CreateWorkspaceResponse, WorkspacesResponse};
use crate::Result;

use super::decode_success;

pub fn workspaces() -> Operation<WorkspacesResponse> {
    Operation::new(crate::workspaces::list(), crate::workspaces::decode_list)
}

pub fn workspace_add(path: &str) -> Result<Operation<()>> {
    Ok(Operation::new(
        crate::workspaces::add(path)?,
        decode_success,
    ))
}

pub fn workspace_create(name: &str) -> Result<Operation<CreateWorkspaceResponse>> {
    Ok(Operation::new(
        crate::workspaces::create(name)?,
        crate::workspaces::decode_create,
    ))
}

pub fn workspace_remove(path: &str) -> Result<Operation<()>> {
    Ok(Operation::new(
        crate::workspaces::remove(path)?,
        decode_success,
    ))
}

pub fn workspace_reorder(paths: &[&str]) -> Result<Operation<WorkspacesResponse>> {
    Ok(Operation::new(
        crate::workspaces::reorder(paths)?,
        crate::workspaces::decode_list,
    ))
}

pub fn workspace_set_default(path: &str) -> Result<Operation<WorkspacesResponse>> {
    Ok(Operation::new(
        crate::workspaces::set_default(path)?,
        crate::workspaces::decode_list,
    ))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::http::HttpMethod;
    use crate::ResponseSpec;

    #[test]
    fn workspace_default_decodes_updated_list() {
        let op = workspace_set_default("/tmp/project").expect("operation");
        assert_eq!(op.request().method, HttpMethod::Put);
        assert_eq!(op.request().path, "/api/workspaces/default");

        let workspaces = op
            .decode(ResponseSpec::json(
                200,
                json!({
                    "default_workspace": "/tmp/project",
                    "workspaces": [{
                        "path": "/tmp/project",
                        "is_default": true,
                        "is_builtin": false
                    }]
                }),
            ))
            .expect("decode");
        assert_eq!(workspaces.default_workspace, "/tmp/project");
    }
}
