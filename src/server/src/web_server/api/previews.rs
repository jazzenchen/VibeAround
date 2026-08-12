use axum::{
    extract::{Path, State},
    http::{header, StatusCode},
    response::{IntoResponse, Response},
    Json,
};

use crate::web_server::AppState;

/// GET /api/previews -- list all live preview sessions and the active tunnel URL.
pub async fn list_previews_handler(State(state): State<AppState>) -> Response {
    let previews = crate::web_server::preview::active_preview_snapshots(true).await;
    let tunnel_url = state.tunnels.first_url();
    let mut response = Json(crate::api_types::PreviewsResponse {
        previews,
        tunnel_url,
    })
    .into_response();
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        "no-store".parse().expect("valid cache-control header"),
    );
    response
}

/// DELETE /api/previews/:slug -- close one preview and kill its dev-server port.
pub async fn delete_preview_handler(Path(slug): Path<String>) -> impl IntoResponse {
    if common::previews::delete_session(&slug) {
        (StatusCode::OK, format!("Preview {} closed", slug))
    } else {
        (StatusCode::NOT_FOUND, format!("Preview {} not found", slug))
    }
}
