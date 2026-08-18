//! Service discovery and metadata endpoints.

use axum::{extract::State, Json};

use crate::web_server::AppState;

/// GET /api/service/health -- tiny unauthenticated liveness check.
pub async fn health_handler() -> Json<crate::api_types::ServiceHealthResponse> {
    Json(crate::api_types::ServiceHealthResponse {
        ok: true,
        service: "vibearound-server",
        version: env!("CARGO_PKG_VERSION"),
    })
}

/// GET /api/service/info -- authenticated service metadata.
pub async fn info_handler(
    State(state): State<AppState>,
) -> Json<crate::api_types::ServiceInfoResponse> {
    Json(crate::api_types::ServiceInfoResponse {
        service: "vibearound-server",
        version: env!("CARGO_PKG_VERSION"),
        port: state.port,
        mode: "server",
        auth_mode: "token",
        data_dir: common::config::data_dir().to_string_lossy().into_owned(),
        settings_path: common::config::settings_path()
            .to_string_lossy()
            .into_owned(),
        web_dist_path: state.dist_for_fallback.to_string_lossy().into_owned(),
        host_search_available: state.host_search_available,
        replace_provider_web_search: state.replace_provider_web_search,
    })
}
