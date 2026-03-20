//! Preview & raw static file serving for project workspaces.
//!
//! - GET /preview/:project_id — HTML page with iframe
//! - GET /raw/:project_id — serve index.html (or first .html) from workspace
//! - GET /raw/:project_id/*path — serve arbitrary file (traversal-safe)

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::Response,
};
use axum::body::Body;

use super::AppState;

/// GET /preview/:project_id — HTML page with iframe pointing to /raw/:project_id/
pub async fn preview_page_handler(
    State(state): State<AppState>,
    Path(project_id): Path<String>,
) -> Result<Response, (StatusCode, String)> {
    let p = state.working_dir.join("workspaces").join(&project_id);
    if !p.exists() {
        return Err((StatusCode::NOT_FOUND, format!("Project not found: {}", project_id)));
    }
    let iframe_src = format!("/raw/{}", project_id);
    let html = format!(
        r#"<!DOCTYPE html><html><head><meta charset="utf-8"><title>Preview</title></head>
<body style="margin:0;overflow:hidden"><iframe src="{}" style="width:100%;height:100vh;border:0"></iframe></body></html>"#,
        iframe_src.replace('"', "&quot;")
    );
    Ok(Response::builder()
        .status(StatusCode::OK)
        .header("Content-Type", "text/html; charset=utf-8")
        .body(Body::from(html))
        .unwrap())
}

/// Shared implementation for raw file serving.
async fn raw_impl(
    state: AppState,
    project_id: String,
    path: Option<String>,
) -> Result<Response, (StatusCode, String)> {
    let base = state.working_dir.join("workspaces").join(&project_id);
    if !base.exists() {
        return Err((StatusCode::NOT_FOUND, format!("Project not found: {}", project_id)));
    }
    let base = base
        .canonicalize()
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    let workspaces_root = state.working_dir.join("workspaces");
    let workspaces_root = workspaces_root
        .canonicalize()
        .unwrap_or_else(|_| state.working_dir.join("workspaces"));
    if !base.starts_with(&workspaces_root) {
        return Err((StatusCode::FORBIDDEN, "Invalid project path".into()));
    }
    let sub = path.as_deref().unwrap_or("").trim_start_matches('/');
    let requested = if sub.is_empty() {
        let index = base.join("index.html");
        if index.exists() {
            index
        } else {
            // No index.html: serve first .html file in directory (e.g. todo.html, todolist.html)
            let mut first_html: Option<std::path::PathBuf> = None;
            if let Ok(entries) = std::fs::read_dir(&base) {
                for e in entries.filter_map(|e| e.ok()) {
                    let p = e.path();
                    if p.is_file()
                        && p.file_name().and_then(|n| n.to_str()).map_or(false, |n| n.to_lowercase().ends_with(".html"))
                    {
                        first_html = Some(p);
                        break;
                    }
                }
            }
            first_html.unwrap_or_else(|| base.join("index.html"))
        }
    } else {
        let p = std::path::Path::new(sub);
        if p.components().any(|c| matches!(c, std::path::Component::ParentDir)) {
            return Err((StatusCode::BAD_REQUEST, "Path traversal not allowed".into()));
        }
        base.join(p)
    };
    let requested = requested
        .canonicalize()
        .map_err(|_| (StatusCode::NOT_FOUND, "Not found".to_string()))?;
    if !requested.starts_with(&base) {
        return Err((StatusCode::FORBIDDEN, "Path outside workspace".into()));
    }
    if !requested.is_file() {
        return Err((StatusCode::NOT_FOUND, "Not found".to_string()));
    }
    let content = tokio::fs::read(&requested).await.map_err(|e| {
        (StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
    })?;
    let mime = mime_guess::from_path(&requested).first_raw().unwrap_or("application/octet-stream");
    Ok(Response::builder()
        .status(StatusCode::OK)
        .header("Content-Type", mime)
        .body(Body::from(content))
        .unwrap())
}

/// GET /raw/:project_id — serve index.html from project workspace.
pub async fn raw_root_handler(
    State(state): State<AppState>,
    Path(project_id): Path<String>,
) -> Result<Response, (StatusCode, String)> {
    raw_impl(state, project_id, None).await
}

/// GET /raw/:project_id/*path — serve static file from project workspace (directory traversal safe).
pub async fn raw_path_handler(
    State(state): State<AppState>,
    Path((project_id, path)): Path<(String, String)>,
) -> Result<Response, (StatusCode, String)> {
    raw_impl(state, project_id, Some(path)).await
}
