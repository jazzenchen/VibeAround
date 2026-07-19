use axum::{http::StatusCode, Json};

use common::config;

fn workspace_item(
    ws: &std::path::Path,
    default_workspace: &std::path::Path,
    builtin: &std::path::Path,
) -> crate::api_types::WorkspaceItem {
    crate::api_types::WorkspaceItem {
        path: ws.to_string_lossy().to_string(),
        is_default: config::workspace_paths_equal(ws, default_workspace),
        is_builtin: config::workspace_paths_equal(ws, builtin),
    }
}

fn workspaces_response() -> Result<crate::api_types::WorkspacesResponse, String> {
    let root = config::read_settings_json()?;
    let settings = config::workspace_settings_from_json(&root);
    let builtin = config::builtin_workspaces_dir();
    let default_workspace = settings.default_workspace;
    let mut all = vec![default_workspace.clone()];
    if !all
        .iter()
        .any(|path| config::workspace_paths_equal(path, &builtin))
    {
        all.push(builtin.clone());
    }
    for workspace in settings.workspaces {
        if !all
            .iter()
            .any(|path| config::workspace_paths_equal(path, &workspace))
        {
            all.push(workspace);
        }
    }

    let workspaces = all
        .iter()
        .map(|ws| workspace_item(ws, &default_workspace, &builtin))
        .collect();

    Ok(crate::api_types::WorkspacesResponse {
        workspaces,
        default_workspace: default_workspace.to_string_lossy().to_string(),
    })
}

/// GET /api/workspaces -- list all workspaces.
pub async fn list_workspaces_handler(
) -> Result<Json<crate::api_types::WorkspacesResponse>, (StatusCode, String)> {
    super::run_blocking_io(|| {
        workspaces_response()
            .map(Json)
            .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error))
    })
    .await
}

#[derive(serde::Deserialize)]
pub(crate) struct WorkspacePathBody {
    path: String,
}

#[derive(serde::Deserialize)]
pub(crate) struct WorkspaceOrderBody {
    paths: Vec<String>,
}

#[derive(serde::Deserialize)]
pub(crate) struct CreateWorkspaceBody {
    name: String,
}

fn validate_workspace_name(name: &str) -> Result<String, (StatusCode, String)> {
    let name = name.trim();
    if name.is_empty() {
        return Err((StatusCode::BAD_REQUEST, "Workspace name is required".into()));
    }
    if name == "." || name == ".." || name.contains('/') || name.contains('\\') {
        return Err((
            StatusCode::BAD_REQUEST,
            "Workspace name must be a single folder name".into(),
        ));
    }
    Ok(name.to_string())
}

/// POST /api/workspaces -- add a workspace path.
pub async fn add_workspace_handler(
    Json(body): Json<WorkspacePathBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    super::run_blocking_io(move || {
        let path = common::workspace::normalize_workspace_cwd(std::path::PathBuf::from(&body.path));
        if !path.exists() || !path.is_dir() {
            return Err((
                StatusCode::BAD_REQUEST,
                format!(
                    "Path does not exist or is not a directory: {}",
                    path.to_string_lossy()
                ),
            ));
        }
        config::register_workspace_path(&path)
            .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error))?;
        let path_string = path.to_string_lossy().to_string();

        Ok(Json(serde_json::json!({ "added": path_string })))
    })
    .await
}

/// POST /api/workspaces/create -- create and register a workspace under the built-in root.
pub async fn create_workspace_handler(
    Json(body): Json<CreateWorkspaceBody>,
) -> Result<Json<crate::api_types::CreateWorkspaceResponse>, (StatusCode, String)> {
    super::run_blocking_io(move || {
        let name = validate_workspace_name(&body.name)?;
        let root = config::read_settings_json()
            .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error))?;
        let default_workspace = config::workspace_settings_from_json(&root).default_workspace;
        let builtin = config::builtin_workspaces_dir();
        let path = default_workspace.join(name);

        if path.exists() && !path.is_dir() {
            return Err((
                StatusCode::BAD_REQUEST,
                format!("Path exists but is not a directory: {}", path.display()),
            ));
        }
        std::fs::create_dir_all(&path)
            .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?;

        config::register_workspace_path(&path)
            .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error))?;

        let response =
            workspaces_response().map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error))?;
        Ok(Json(crate::api_types::CreateWorkspaceResponse {
            workspace: workspace_item(&path, &default_workspace, &builtin),
            workspaces: response.workspaces,
            default_workspace: response.default_workspace,
        }))
    })
    .await
}

/// POST /api/workspaces/remove -- remove a workspace path (cannot remove built-in).
pub async fn remove_workspace_handler(
    Json(body): Json<WorkspacePathBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    super::run_blocking_io(move || {
        let path = std::path::PathBuf::from(&body.path);
        config::remove_registered_workspace(&path)
            .map_err(|error| remove_workspace_error(error, &path))?;

        Ok(Json(serde_json::json!({ "removed": body.path })))
    })
    .await
}

/// PUT /api/workspaces/order -- reorder registered user workspaces.
pub async fn reorder_workspaces_handler(
    Json(body): Json<WorkspaceOrderBody>,
) -> Result<Json<crate::api_types::WorkspacesResponse>, (StatusCode, String)> {
    super::run_blocking_io(move || {
        let requested = body
            .paths
            .into_iter()
            .map(std::path::PathBuf::from)
            .collect::<Vec<_>>();
        config::reorder_workspace_paths(&requested)
            .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error))?;
        workspaces_response()
            .map(Json)
            .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error))
    })
    .await
}

/// PUT /api/workspaces/default -- set the default workspace root.
pub async fn set_default_workspace_handler(
    Json(body): Json<WorkspacePathBody>,
) -> Result<Json<crate::api_types::WorkspacesResponse>, (StatusCode, String)> {
    super::run_blocking_io(move || {
        let path = common::workspace::normalize_workspace_cwd(std::path::PathBuf::from(&body.path));
        std::fs::create_dir_all(&path)
            .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?;
        config::set_default_workspace_path(&path)
            .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error))?;
        workspaces_response()
            .map(Json)
            .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error))
    })
    .await
}

fn remove_workspace_error(
    error: config::RemoveWorkspaceError,
    path: &std::path::Path,
) -> (StatusCode, String) {
    match error {
        config::RemoveWorkspaceError::Default => (
            StatusCode::BAD_REQUEST,
            "Cannot remove the default workspace".to_string(),
        ),
        config::RemoveWorkspaceError::Builtin => (
            StatusCode::BAD_REQUEST,
            "Cannot remove the built-in workspace".to_string(),
        ),
        config::RemoveWorkspaceError::Unregistered => (
            StatusCode::NOT_FOUND,
            format!("Workspace is not registered: {}", path.display()),
        ),
        config::RemoveWorkspaceError::Storage(message) => {
            (StatusCode::INTERNAL_SERVER_ERROR, message)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_workspace_folder_names() {
        assert_eq!(validate_workspace_name(" project-a ").unwrap(), "project-a");
        assert!(validate_workspace_name("").is_err());
        assert!(validate_workspace_name("../project").is_err());
        assert!(validate_workspace_name("nested/project").is_err());
        assert!(validate_workspace_name("nested\\project").is_err());
    }
}
