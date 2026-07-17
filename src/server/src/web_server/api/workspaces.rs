use axum::{http::StatusCode, Json};

use common::config;

fn workspace_item(
    ws: &std::path::Path,
    default_workspace: &std::path::Path,
    builtin: &std::path::Path,
) -> crate::api_types::WorkspaceItem {
    crate::api_types::WorkspaceItem {
        path: ws.to_string_lossy().to_string(),
        is_default: paths_equal(ws, default_workspace),
        is_builtin: paths_equal(ws, builtin),
    }
}

fn workspaces_response() -> crate::api_types::WorkspacesResponse {
    let cfg = config::ensure_loaded();
    let builtin = config::builtin_workspaces_dir();
    let default_workspace = cfg.resolve_workspace("");
    let all = cfg.all_workspaces();

    let workspaces = all
        .iter()
        .map(|ws| workspace_item(ws, &default_workspace, &builtin))
        .collect();

    crate::api_types::WorkspacesResponse {
        workspaces,
        default_workspace: default_workspace.to_string_lossy().to_string(),
    }
}

/// GET /api/workspaces -- list all workspaces.
pub async fn list_workspaces_handler() -> Json<crate::api_types::WorkspacesResponse> {
    Json(workspaces_response())
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
    let path_string = path.to_string_lossy().to_string();
    let stored_path = path_string.clone();
    config::update_settings_json_async(move |root| {
        if let Some(obj) = root.as_object_mut() {
            let workspaces = obj
                .entry("workspaces")
                .or_insert_with(|| serde_json::json!([]));
            if let Some(arr) = workspaces.as_array_mut() {
                let val = serde_json::Value::String(stored_path.clone());
                if !arr.contains(&val) {
                    arr.push(val);
                }
            }
        }
    })
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;

    Ok(Json(serde_json::json!({ "added": path_string })))
}

/// POST /api/workspaces/create -- create and register a workspace under the built-in root.
pub async fn create_workspace_handler(
    Json(body): Json<CreateWorkspaceBody>,
) -> Result<Json<crate::api_types::CreateWorkspaceResponse>, (StatusCode, String)> {
    let name = validate_workspace_name(&body.name)?;
    let builtin = config::builtin_workspaces_dir();
    let cfg = config::ensure_loaded();
    let default_workspace = cfg.resolve_workspace("");
    let path = default_workspace.join(name);

    if path.exists() && !path.is_dir() {
        return Err((
            StatusCode::BAD_REQUEST,
            format!("Path exists but is not a directory: {}", path.display()),
        ));
    }
    std::fs::create_dir_all(&path)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let path_string = path.to_string_lossy().to_string();
    config::update_settings_json_async(move |root| {
        if let Some(obj) = root.as_object_mut() {
            let workspaces = obj
                .entry("workspaces")
                .or_insert_with(|| serde_json::json!([]));
            if let Some(arr) = workspaces.as_array_mut() {
                let val = serde_json::Value::String(path_string.clone());
                if !arr.contains(&val) {
                    arr.push(val);
                }
            }
        }
    })
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;

    let response = workspaces_response();
    Ok(Json(crate::api_types::CreateWorkspaceResponse {
        workspace: workspace_item(&path, &default_workspace, &builtin),
        workspaces: response.workspaces,
        default_workspace: response.default_workspace,
    }))
}

/// POST /api/workspaces/remove -- remove a workspace path (cannot remove built-in).
pub async fn remove_workspace_handler(
    Json(body): Json<WorkspacePathBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let path = std::path::PathBuf::from(&body.path);
    let builtin = config::builtin_workspaces_dir();
    let cfg = config::ensure_loaded();
    let default_workspace = cfg.resolve_workspace("");
    if paths_equal(&path, &default_workspace) {
        return Err((
            StatusCode::BAD_REQUEST,
            "Cannot remove the default workspace".into(),
        ));
    }
    if paths_equal(&path, &builtin) {
        return Err((
            StatusCode::BAD_REQUEST,
            "Cannot remove the built-in workspace".into(),
        ));
    }

    if !cfg
        .all_workspaces()
        .iter()
        .any(|workspace| paths_equal(workspace, &path))
    {
        return Err((
            StatusCode::NOT_FOUND,
            format!("Workspace is not registered: {}", path.display()),
        ));
    }

    tokio::task::spawn_blocking(move || {
        config::remove_workspace_path(&path)?;
        common::agent_state::remove_workspace_references(&path).map_err(|error| error.to_string())
    })
    .await
    .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?
    .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error))?;

    Ok(Json(serde_json::json!({ "removed": body.path })))
}

/// PUT /api/workspaces/order -- reorder registered user workspaces.
pub async fn reorder_workspaces_handler(
    Json(body): Json<WorkspaceOrderBody>,
) -> Result<Json<crate::api_types::WorkspacesResponse>, (StatusCode, String)> {
    let cfg = config::ensure_loaded();
    let builtin = config::builtin_workspaces_dir();
    let mut seen = std::collections::HashSet::new();
    let mut ordered = Vec::new();

    for path in body.paths {
        let path = std::path::PathBuf::from(path);
        if paths_equal(&path, &builtin) || paths_equal(&path, &cfg.default_workspace) {
            continue;
        }
        if cfg
            .workspaces
            .iter()
            .any(|workspace| paths_equal(workspace, &path))
            && seen.insert(path.clone())
        {
            ordered.push(path);
        }
    }

    for workspace in &cfg.workspaces {
        if paths_equal(workspace, &builtin) || paths_equal(workspace, &cfg.default_workspace) {
            continue;
        }
        if seen.insert(workspace.clone()) {
            ordered.push(workspace.clone());
        }
    }

    config::update_settings_json_async(move |root| {
        if !root.is_object() {
            *root = serde_json::json!({});
        }
        if let Some(obj) = root.as_object_mut() {
            obj.insert(
                "workspaces".into(),
                serde_json::Value::Array(
                    ordered
                        .iter()
                        .map(|path| serde_json::Value::String(path.to_string_lossy().to_string()))
                        .collect(),
                ),
            );
        }
    })
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;

    Ok(Json(workspaces_response()))
}

/// PUT /api/workspaces/default -- set the default workspace root.
pub async fn set_default_workspace_handler(
    Json(body): Json<WorkspacePathBody>,
) -> Result<Json<crate::api_types::WorkspacesResponse>, (StatusCode, String)> {
    let path = common::workspace::normalize_workspace_cwd(std::path::PathBuf::from(&body.path));
    std::fs::create_dir_all(&path)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    let path_string = path.to_string_lossy().to_string();

    config::update_settings_json_async(move |root| {
        if !root.is_object() {
            *root = serde_json::json!({});
        }
        if let Some(obj) = root.as_object_mut() {
            obj.insert(
                "default_workspace".to_string(),
                serde_json::Value::String(path_string.clone()),
            );
            if let Some(workspaces) = obj
                .get_mut("workspaces")
                .and_then(|value| value.as_array_mut())
            {
                workspaces.retain(|value| {
                    value
                        .as_str()
                        .map(|candidate| !paths_equal(std::path::Path::new(candidate), &path))
                        .unwrap_or(true)
                });
            }
        }
    })
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;

    Ok(Json(workspaces_response()))
}

fn paths_equal(left: &std::path::Path, right: &std::path::Path) -> bool {
    left == right
        || std::fs::canonicalize(left)
            .ok()
            .zip(std::fs::canonicalize(right).ok())
            .map(|(left, right)| left == right)
            .unwrap_or(false)
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
