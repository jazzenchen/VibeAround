//! Owner Preview SPA response and iframe target dispatch.

use axum::body::Body;
use axum::http::StatusCode;
use axum::response::Response;

use common::previews::{PreviewEntry, PreviewSnapshot, PreviewTarget};

use super::markdown::render_md_content;

pub(super) fn render_owner_app(
    html: String,
    previews: &[PreviewSnapshot],
    server_host: &str,
) -> Response {
    let csp = format!(
        "default-src 'none'; script-src 'self'; script-src-attr 'none'; style-src 'self' 'unsafe-inline'; img-src 'self' https: data: blob:; media-src 'self' https: data: blob:; font-src 'self' data:; connect-src 'self'; frame-src {}; base-uri 'none'; form-action 'none'; frame-ancestors 'none'",
        frame_sources(previews, server_host)
    );
    Response::builder()
        .status(StatusCode::OK)
        .header("Content-Type", "text/html; charset=utf-8")
        .header("Content-Security-Policy", csp)
        .header("Referrer-Policy", "no-referrer")
        .header("X-Content-Type-Options", "nosniff")
        .body(Body::from(html))
        .expect("valid owner Preview app response")
}

/// Render one already-authorized iframe target.
pub(super) async fn render_owner_content(
    entry: PreviewEntry,
    annotations_enabled: bool,
) -> Result<Response, (StatusCode, String)> {
    match &entry.target {
        PreviewTarget::Server { .. } => Err((
            StatusCode::BAD_REQUEST,
            "Live server previews load directly from their local origin.".to_string(),
        )),
        PreviewTarget::File => render_md_content(&entry, annotations_enabled).await,
    }
}

pub(super) fn preview_src(slug: &str, port: Option<u16>, server_host: &str) -> String {
    port.map(|port| format!("http://{server_host}:{port}/"))
        .unwrap_or_else(|| format!("/va/preview/u/{slug}/content"))
}

fn frame_sources(previews: &[PreviewSnapshot], server_host: &str) -> String {
    let mut ports = previews
        .iter()
        .filter_map(|preview| preview.port)
        .collect::<Vec<_>>();
    ports.sort_unstable();
    ports.dedup();

    std::iter::once("'self'".to_string())
        .chain(
            ports
                .into_iter()
                .map(|port| format!("http://{server_host}:{port}")),
        )
        .collect::<Vec<_>>()
        .join(" ")
}
