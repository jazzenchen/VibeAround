//! Owner Preview shell and iframe content dispatch.
//!
//! `/preview/u/{slug}` renders one small owner-only shell. Its native details
//! panel groups live previews by workspace. File content uses the owner-only
//! `/preview/u/{slug}/content` route. Server content loads directly from the
//! dev server's distinct `localhost:{port}` origin.

use std::path::Path;

use axum::body::Body;
use axum::http::StatusCode;
use axum::response::Response;

use common::previews::{PreviewEntry, PreviewSnapshot, PreviewTarget};

use super::assets::theme_stylesheet_href;
use super::markdown::render_md_content;
use super::toolbar::escape_html;

const OWNER_CLIENT_JS: &str = include_str!("owner_client.js");

/// Render the owner-only shell and its workspace/preview picker.
pub(super) fn render_owner_shell(
    entry: &PreviewEntry,
    selected_slug: &str,
    previews: &[PreviewSnapshot],
    server_host: &str,
) -> Response {
    let title = escape_html(&entry.title);
    let options = render_preview_options(previews, selected_slug, server_host);
    let initial_port = match entry.target {
        PreviewTarget::Server { port } => Some(port),
        PreviewTarget::File => None,
    };
    let initial_src = escape_html(&preview_src(selected_slug, initial_port, server_host));
    let frame_sources = frame_sources(previews, server_host);
    let nonce = uuid::Uuid::new_v4().simple().to_string();
    let html = format!(
        r#"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>Preview — {title}</title>
<link rel="stylesheet" href="{theme_stylesheet_href}">
<style>
  * {{ box-sizing: border-box; }}
  html, body {{ width: 100%; height: 100%; margin: 0; overflow: hidden; }}
  body {{
    background: var(--background);
    color: var(--foreground);
    font: 13px -apple-system, BlinkMacSystemFont, "SF Pro Text", "Segoe UI", ui-sans-serif, system-ui, sans-serif;
  }}
  iframe {{ width: 100%; height: 100%; border: 0; background: var(--background); }}
  .switcher {{
    position: fixed;
    top: 12px;
    left: 12px;
    z-index: 100;
    width: min(440px, calc(100vw - 24px));
    border: 1px solid var(--border);
    border-radius: var(--radius);
    background: color-mix(in oklab, var(--popover) 96%, transparent);
    box-shadow: 0 4px 14px rgba(0, 0, 0, 0.10);
    backdrop-filter: blur(12px);
  }}
  .switcher:not([open]) {{ width: auto; max-width: calc(100vw - 24px); }}
  summary {{
    display: flex;
    align-items: center;
    gap: 8px;
    min-height: 38px;
    padding: 0 12px;
    cursor: pointer;
    user-select: none;
  }}
  summary::-webkit-details-marker {{ display: none; }}
  summary::before {{ content: "▸"; color: var(--muted-foreground); }}
  details[open] summary::before {{ content: "▾"; }}
  .mark {{ width: 22px; height: 22px; flex: 0 0 auto; }}
  .brand {{ color: var(--popover-foreground); font-weight: 600; white-space: nowrap; }}
  .current {{ color: var(--muted-foreground); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }}
  .controls {{ display: flex; gap: 8px; padding: 8px; border-top: 1px solid var(--border); }}
  .picker {{ position: relative; min-width: 0; flex: 1; }}
  select, button {{
    min-height: 34px;
    border: 1px solid var(--input);
    border-radius: calc(var(--radius) - 2px);
    background: var(--background);
    color: var(--foreground);
    font: inherit;
  }}
  select {{ display: block; width: 100%; min-width: 0; padding: 0 36px 0 12px; appearance: none; cursor: pointer; }}
  .picker-icon {{
    position: absolute;
    top: 50%;
    right: 12px;
    width: 16px;
    height: 16px;
    transform: translateY(-50%);
    color: var(--muted-foreground);
    pointer-events: none;
  }}
  button {{ padding: 0 12px; cursor: pointer; }}
  button:hover {{ background: var(--accent); color: var(--accent-foreground); }}
  select:focus-visible, button:focus-visible, summary:focus-visible {{
    outline: 3px solid color-mix(in oklab, var(--ring) 30%, transparent);
    outline-offset: 1px;
  }}
  @media (max-width: 480px) {{
    .switcher {{ top: 8px; left: 8px; width: calc(100vw - 16px); }}
    .controls {{ flex-wrap: wrap; }}
    .picker {{ flex-basis: 100%; }}
    button {{ width: 100%; }}
  }}
</style>
</head>
<body>
<details class="switcher">
  <summary><img class="mark" src="/va/brand/vibearound-mark.svg" alt=""><span class="brand">VibeAround Preview</span><span class="current" id="current-preview">{title}</span></summary>
  <div class="controls">
    <div class="picker">
      <select id="preview-picker" aria-label="Workspace and preview">{options}</select>
      <svg class="picker-icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
        <path d="m6 9 6 6 6-6"></path>
      </svg>
    </div>
    <button type="button" id="refresh-preview">Refresh</button>
  </div>
</details>
<iframe id="preview-frame" title="Preview content — {title}" src="{initial_src}" referrerpolicy="no-referrer"></iframe>
<script nonce="{nonce}">
{owner_client_js}
</script>
</body>
</html>"#,
        title = title,
        options = options,
        initial_src = initial_src,
        nonce = nonce,
        owner_client_js = OWNER_CLIENT_JS,
        theme_stylesheet_href = theme_stylesheet_href(),
    );

    let csp = format!(
        "default-src 'none'; script-src 'nonce-{nonce}'; script-src-attr 'none'; style-src 'self' 'unsafe-inline'; img-src 'self'; connect-src 'self'; frame-src {frame_sources}; base-uri 'none'; form-action 'none'; frame-ancestors 'none'"
    );
    Response::builder()
        .status(StatusCode::OK)
        .header("Content-Type", "text/html; charset=utf-8")
        .header("Content-Security-Policy", csp)
        .header("Referrer-Policy", "no-referrer")
        .header("X-Content-Type-Options", "nosniff")
        .body(Body::from(html))
        .unwrap()
}

/// Render one already-authorized iframe target.
pub(super) async fn render_owner_content(
    entry: PreviewEntry,
) -> Result<Response, (StatusCode, String)> {
    match &entry.target {
        PreviewTarget::Server { .. } => Err((
            StatusCode::BAD_REQUEST,
            "Live server previews load directly from their local origin.".to_string(),
        )),
        PreviewTarget::File => render_md_content(&entry).await,
    }
}

fn render_preview_options(
    previews: &[PreviewSnapshot],
    selected_slug: &str,
    server_host: &str,
) -> String {
    let mut previews = previews.iter().collect::<Vec<_>>();
    previews.sort_by(|a, b| {
        a.workspace
            .cmp(&b.workspace)
            .then_with(|| a.created_at_ms.cmp(&b.created_at_ms))
    });

    let mut html = String::new();
    let mut open_workspace: Option<&Path> = None;
    for preview in previews {
        if open_workspace != Some(preview.workspace.as_path()) {
            if open_workspace.is_some() {
                html.push_str("</optgroup>");
            }
            let workspace = workspace_label(&preview.workspace);
            html.push_str(&format!(
                r#"<optgroup label="{}">"#,
                escape_html(&workspace)
            ));
            open_workspace = Some(&preview.workspace);
        }

        let label = match preview.port {
            Some(port) => format!("{} · :{}", preview.title, port),
            None => format!("{} · Markdown", preview.title),
        };
        let selected = if preview.slug == selected_slug {
            " selected"
        } else {
            ""
        };
        let src = preview_src(&preview.slug, preview.port, server_host);
        html.push_str(&format!(
            r#"<option value="{}" data-title="{}" data-src="{}"{}>{}</option>"#,
            escape_html(&preview.slug),
            escape_html(&preview.title),
            escape_html(&src),
            selected,
            escape_html(&label),
        ));
    }
    if open_workspace.is_some() {
        html.push_str("</optgroup>");
    }
    html
}

fn preview_src(slug: &str, port: Option<u16>, server_host: &str) -> String {
    port.map(|port| server_origin(server_host, port))
        .unwrap_or_else(|| owner_content_path(slug))
}

fn owner_content_path(slug: &str) -> String {
    format!("/va/preview/u/{slug}/content")
}

fn server_origin(host: &str, port: u16) -> String {
    format!("http://{host}:{port}/")
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

fn workspace_label(workspace: &Path) -> String {
    workspace
        .file_name()
        .and_then(|name| name.to_str())
        .map(str::to_owned)
        .unwrap_or_else(|| workspace.display().to_string())
}

#[cfg(test)]
#[path = "iframe_tests.rs"]
mod tests;
