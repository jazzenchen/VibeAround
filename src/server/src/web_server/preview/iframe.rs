//! Server-target preview: iframe wrapper + cookie setup.
//!
//! When a preview session has a `Server { port }` target, we render a
//! tiny HTML shell whose `<iframe src="/">` serves dev-server content
//! through the cookie-based proxy fallback at the root path. The authorized
//! owner route is stashed in a `va_preview` cookie, so the proxy can
//! revalidate that boundary on each request.

use axum::body::Body;
use axum::http::StatusCode;
use axum::response::Response;

use common::previews::{PreviewEntry, PreviewTarget};

use super::cookie_proxy::PREVIEW_COOKIE;
use super::markdown::render_md_page;
use super::toolbar::{escape_html, remaining_millis, toolbar_and_timer, TOOLBAR_CSS};

/// Dispatch an already-authorized owner preview to its target renderer.
pub(super) async fn render_owner_preview(
    entry: PreviewEntry,
    routing_cookie: &str,
) -> Result<Response, (StatusCode, String)> {
    match entry.target {
        PreviewTarget::Server { port } => render_server_iframe(&entry, port, routing_cookie).await,
        PreviewTarget::File => render_md_page(&entry).await,
    }
}

/// Render the iframe wrapper for a Server preview; sets `va_preview` cookie
/// so the root-level cookie proxy can route requests to `localhost:{port}`.
async fn render_server_iframe(
    entry: &PreviewEntry,
    port: u16,
    routing_cookie: &str,
) -> Result<Response, (StatusCode, String)> {
    let remaining_ms = remaining_millis(entry);
    let title = escape_html(&entry.title);
    let subtitle = format!(":{}", port);

    let mut cookie = format!(
        "{}={}; Path=/; HttpOnly; SameSite=Lax",
        PREVIEW_COOKIE, routing_cookie
    );
    if let Some(milliseconds) = remaining_ms {
        cookie.push_str(&format!("; Max-Age={}", milliseconds / 1000));
    }

    let toolbar = toolbar_and_timer(
        &title,
        &subtitle,
        remaining_ms,
        r#"<button onclick="document.querySelector('iframe').src=document.querySelector('iframe').src">Refresh</button>"#,
    );
    let html = format!(
        r#"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>Preview — {title}</title>
<style>
  {toolbar_css}
  body {{ background: #111; }}
  iframe {{
    width: 100%;
    height: calc(100vh - 40px);
    border: 0;
    background: #fff;
  }}
</style>
</head>
<body>
{toolbar}
<iframe src="/"></iframe>
</body>
</html>"#,
        title = title,
        toolbar_css = TOOLBAR_CSS,
        toolbar = toolbar,
    );

    Ok(Response::builder()
        .status(StatusCode::OK)
        .header("Content-Type", "text/html; charset=utf-8")
        .header("Cache-Control", "no-store")
        .header("Referrer-Policy", "no-referrer")
        .header("Set-Cookie", cookie)
        .body(Body::from(html))
        .unwrap())
}

pub(super) fn server_not_running_page(port: u16) -> Response {
    let html = format!(
        r#"<!DOCTYPE html>
<html><head><meta charset="utf-8"><title>Server not running</title>
<style>
  body {{ font-family: -apple-system, sans-serif; display: flex; align-items: center; justify-content: center; height: 100vh; margin: 0; background: #f5f5f5; color: #333; }}
  .card {{ text-align: center; padding: 40px; background: #fff; border-radius: 12px; box-shadow: 0 2px 12px rgba(0,0,0,0.08); max-width: 400px; }}
  .card h2 {{ margin-bottom: 8px; }}
  .card p {{ color: #666; line-height: 1.5; }}
  .port {{ font-family: monospace; background: #eee; padding: 2px 6px; border-radius: 4px; }}
</style></head>
<body><div class="card">
  <h2>Server not running</h2>
  <p>No server is responding on port <span class="port">{port}</span>.<br>
  The dev server may have stopped or not started yet.</p>
</div></body></html>"#,
        port = port,
    );
    Response::builder()
        .status(StatusCode::BAD_GATEWAY)
        .header("Content-Type", "text/html; charset=utf-8")
        .body(Body::from(html))
        .unwrap()
}
