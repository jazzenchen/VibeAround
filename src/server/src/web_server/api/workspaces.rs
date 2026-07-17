use axum::{http::StatusCode, Json};

use common::config;

#[derive(Debug, PartialEq, Eq)]
enum RemoveWorkspaceError {
    Default,
    Builtin,
    Unregistered,
    Internal(String),
}

impl RemoveWorkspaceError {
    fn into_api_error(self, path: &std::path::Path) -> (StatusCode, String) {
        match self {
            Self::Default => (
                StatusCode::BAD_REQUEST,
                "Cannot remove the default workspace".to_string(),
            ),
            Self::Builtin => (
                StatusCode::BAD_REQUEST,
                "Cannot remove the built-in workspace".to_string(),
            ),
            Self::Unregistered => (
                StatusCode::NOT_FOUND,
                format!("Workspace is not registered: {}", path.display()),
            ),
            Self::Internal(error) => (StatusCode::INTERNAL_SERVER_ERROR, error),
        }
    }
}

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

fn workspaces_response() -> Result<crate::api_types::WorkspacesResponse, String> {
    let root = config::read_settings_json()?;
    let settings = config::workspace_settings_from_json(&root);
    let builtin = config::builtin_workspaces_dir();
    let default_workspace = settings.default_workspace;
    let mut all = vec![default_workspace.clone()];
    if !all.contains(&builtin) {
        all.push(builtin.clone());
    }
    for workspace in settings.workspaces {
        if !all.contains(&workspace) {
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
        let path_string = path.to_string_lossy().to_string();
        config::mutate_settings_json(|root| add_workspace_to_settings(root, &path_string))
            .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error))?;

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

        let path_string = path.to_string_lossy().to_string();
        config::mutate_settings_json(|root| add_workspace_to_settings(root, &path_string))
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
        let mut rejection = None;
        let result = config::mutate_settings_json(|root| {
            remove_workspace_from_settings(root, &path).map_err(|error| {
                rejection = Some(error);
                "workspace removal rejected".to_string()
            })
        });
        if let Some(error) = rejection {
            return Err(error.into_api_error(&path));
        }
        result.map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error))?;

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
        config::mutate_settings_json(|root| reorder_workspaces_in_settings(root, &requested))
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
        config::mutate_settings_json(|root| set_default_workspace_in_settings(root, &path))
            .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error))?;
        workspaces_response()
            .map(Json)
            .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error))
    })
    .await
}

fn add_workspace_to_settings(root: &mut serde_json::Value, path: &str) -> Result<(), String> {
    let obj = root
        .as_object_mut()
        .ok_or_else(|| "settings.json root must be a JSON object".to_string())?;
    let workspaces = obj
        .entry("workspaces".to_string())
        .or_insert_with(|| serde_json::json!([]));
    let workspaces = workspaces
        .as_array_mut()
        .ok_or_else(|| "settings.json workspaces must be an array".to_string())?;
    let path = std::path::Path::new(path);
    if !workspaces.iter().any(|value| {
        value
            .as_str()
            .map(|candidate| paths_equal(std::path::Path::new(candidate), path))
            .unwrap_or(false)
    }) {
        workspaces.push(serde_json::Value::String(
            path.to_string_lossy().to_string(),
        ));
    }
    Ok(())
}

fn reorder_workspaces_in_settings(
    root: &mut serde_json::Value,
    requested: &[std::path::PathBuf],
) -> Result<(), String> {
    let settings = config::workspace_settings_from_json(root);
    let builtin = config::builtin_workspaces_dir();
    let mut ordered = Vec::new();

    for requested_path in requested {
        if paths_equal(requested_path, &settings.default_workspace)
            || paths_equal(requested_path, &builtin)
        {
            continue;
        }
        if let Some(current) = settings
            .workspaces
            .iter()
            .find(|current| paths_equal(current, requested_path))
        {
            push_unique_path(&mut ordered, current.clone());
        }
    }
    for workspace in settings.workspaces {
        if !paths_equal(&workspace, &settings.default_workspace)
            && !paths_equal(&workspace, &builtin)
        {
            push_unique_path(&mut ordered, workspace);
        }
    }

    let obj = root
        .as_object_mut()
        .ok_or_else(|| "settings.json root must be a JSON object".to_string())?;
    obj.insert(
        "workspaces".to_string(),
        serde_json::Value::Array(
            ordered
                .into_iter()
                .map(|path| serde_json::Value::String(path.to_string_lossy().to_string()))
                .collect(),
        ),
    );
    Ok(())
}

fn remove_workspace_from_settings(
    root: &mut serde_json::Value,
    path: &std::path::Path,
) -> Result<(), RemoveWorkspaceError> {
    let settings = config::workspace_settings_from_json(root);
    if paths_equal(path, &settings.default_workspace) {
        return Err(RemoveWorkspaceError::Default);
    }
    if paths_equal(path, &config::builtin_workspaces_dir()) {
        return Err(RemoveWorkspaceError::Builtin);
    }
    if !settings
        .workspaces
        .iter()
        .any(|workspace| paths_equal(workspace, path))
    {
        return Err(RemoveWorkspaceError::Unregistered);
    }

    config::remove_workspace_from_settings(root, path);
    common::agent_state::remove_workspace_references_from_settings(root, path)
        .map_err(RemoveWorkspaceError::Internal)
}

fn set_default_workspace_in_settings(
    root: &mut serde_json::Value,
    path: &std::path::Path,
) -> Result<(), String> {
    let obj = root
        .as_object_mut()
        .ok_or_else(|| "settings.json root must be a JSON object".to_string())?;
    obj.insert(
        "default_workspace".to_string(),
        serde_json::Value::String(path.to_string_lossy().to_string()),
    );
    if let Some(workspaces) = obj
        .get_mut("workspaces")
        .and_then(|value| value.as_array_mut())
    {
        workspaces.retain(|value| {
            value
                .as_str()
                .map(|candidate| !paths_equal(std::path::Path::new(candidate), path))
                .unwrap_or(true)
        });
    }
    Ok(())
}

fn push_unique_path(paths: &mut Vec<std::path::PathBuf>, path: std::path::PathBuf) {
    if !paths.iter().any(|existing| paths_equal(existing, &path)) {
        paths.push(path);
    }
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

    #[test]
    fn reorder_preserves_workspace_added_after_request_snapshot() {
        let mut settings = serde_json::json!({
            "default_workspace": "/tmp/default",
            "workspaces": ["/tmp/first", "/tmp/second", "/tmp/concurrent"],
            "enabled_agents": ["codex"]
        });
        let requested = vec![
            std::path::PathBuf::from("/tmp/second"),
            std::path::PathBuf::from("/tmp/first"),
            std::path::PathBuf::from("/tmp/unknown"),
        ];

        reorder_workspaces_in_settings(&mut settings, &requested).unwrap();

        assert_eq!(
            settings["workspaces"],
            serde_json::json!(["/tmp/second", "/tmp/first", "/tmp/concurrent"])
        );
        assert_eq!(settings["enabled_agents"], serde_json::json!(["codex"]));
    }

    #[test]
    fn remove_validates_latest_settings_and_clears_launcher_reference() {
        let mut settings = serde_json::json!({
            "default_workspace": "/tmp/default",
            "workspaces": ["/tmp/removed", "/tmp/kept"],
            "launcher": {
                "terminal": "terminal",
                "agents": {
                    "codex": { "workspace": "/tmp/removed" },
                    "claude": { "workspace": "/tmp/kept" }
                }
            }
        });

        assert_eq!(
            remove_workspace_from_settings(&mut settings, std::path::Path::new("/tmp/default"))
                .unwrap_err(),
            RemoveWorkspaceError::Default
        );
        assert_eq!(
            remove_workspace_from_settings(&mut settings, std::path::Path::new("/tmp/missing"))
                .unwrap_err(),
            RemoveWorkspaceError::Unregistered
        );
        assert_eq!(
            remove_workspace_from_settings(&mut settings, &config::builtin_workspaces_dir())
                .unwrap_err(),
            RemoveWorkspaceError::Builtin
        );

        remove_workspace_from_settings(&mut settings, std::path::Path::new("/tmp/removed"))
            .unwrap();

        assert_eq!(settings["workspaces"], serde_json::json!(["/tmp/kept"]));
        assert!(settings["launcher"]["agents"].get("codex").is_none());
        assert_eq!(
            settings["launcher"]["agents"]["claude"]["workspace"],
            "/tmp/kept"
        );
        assert_eq!(settings["launcher"]["terminal"], "terminal");
    }
}
