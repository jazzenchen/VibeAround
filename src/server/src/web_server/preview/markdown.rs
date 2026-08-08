//! Markdown-target preview: rendered document page.
//!
//! Markdown stays browser-rendered with `marked.js`. The source is carried as
//! inert JSON, and the client renderer treats raw HTML as text while allowing
//! only absolute HTTPS Markdown images.

use axum::body::Body;
use axum::http::StatusCode;
use axum::response::Response;

use common::previews::{PreviewEntry, PreviewTarget};

use super::assets::MARKED_SCRIPT_ROUTE;
use super::toolbar::{escape_html, remaining_millis, toolbar_and_timer, TOOLBAR_CSS};

const MARKDOWN_CLIENT_JS: &str = include_str!("markdown_client.js");

/// Render a Markdown file preview with a browser-side Markdown renderer.
pub(super) async fn render_md_page(entry: &PreviewEntry) -> Result<Response, (StatusCode, String)> {
    let file_path = match &entry.target {
        PreviewTarget::File => &entry.id,
        PreviewTarget::Server { .. } => {
            return Err((
                StatusCode::BAD_REQUEST,
                "This preview serves a server, not a file.".to_string(),
            ));
        }
    };

    let content = tokio::fs::read_to_string(file_path).await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to read file: {e}"),
        )
    })?;

    let remaining_ms = remaining_millis(entry);
    let title = escape_html(&entry.title);
    let nonce = uuid::Uuid::new_v4().simple().to_string();

    // Owner entries have no expiry and may show local path context. Shared
    // entries carry the transaction expiry and must not disclose workspace
    // or file paths to public viewers.
    let subtitle = if entry.expires_at.is_some() {
        String::new()
    } else {
        let ws_name = entry
            .workspace
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("");
        if let Ok(relative) = file_path.strip_prefix(&entry.workspace) {
            format!(
                "{} / {}",
                escape_html(ws_name),
                escape_html(&relative.display().to_string())
            )
        } else {
            escape_html(ws_name).to_string()
        }
    };

    let markdown_json = json_for_html_script(&content);
    let toolbar = toolbar_and_timer(&title, &subtitle, remaining_ms, "", Some(&nonce));
    let html = format!(
        r#"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>{title}</title>
<link rel="stylesheet" href="https://cdn.jsdelivr.net/npm/github-markdown-css@5/github-markdown-light.min.css">
<style>
  {toolbar_css}
  body {{ background: #fff; color: #1f2328; }}
  .markdown-body {{ max-width: 880px; margin: 0 auto; padding: 32px 24px 64px; }}
  @media (max-width: 767px) {{ .markdown-body {{ padding: 16px; }} }}
  .markdown-body pre {{ background: #f6f8fa; border-radius: 6px; padding: 16px; overflow-x: auto; }}
  .markdown-body code {{ font-family: ui-monospace, SFMono-Regular, "SF Mono", Menlo, Consolas, monospace; font-size: 85%; }}
  .markdown-body pre code {{ background: transparent; padding: 0; }}
  .markdown-body table {{ border-collapse: collapse; width: 100%; }}
  .markdown-body table th, .markdown-body table td {{ border: 1px solid #d0d7de; padding: 6px 13px; }}
  .markdown-body table tr:nth-child(2n) {{ background: #f6f8fa; }}
</style>
</head>
<body>
{toolbar}
<article class="markdown-body" id="content"></article>
<script nonce="{nonce}" id="markdown-source" type="application/json">{markdown_json}</script>
<script nonce="{nonce}" src="/va{marked_script_route}"></script>
<script nonce="{nonce}">
{markdown_client_js}
</script>
</body>
</html>"#,
        title = title,
        toolbar_css = TOOLBAR_CSS,
        toolbar = toolbar,
        nonce = nonce,
        markdown_json = markdown_json,
        markdown_client_js = MARKDOWN_CLIENT_JS,
        marked_script_route = MARKED_SCRIPT_ROUTE,
    );
    Ok(markdown_response(html, &nonce))
}

fn json_for_html_script(content: &str) -> String {
    serde_json::to_string(content)
        .expect("serializing a string to JSON cannot fail")
        .replace('&', "\\u0026")
        .replace('<', "\\u003c")
        .replace('>', "\\u003e")
}

fn markdown_response(html: String, nonce: &str) -> Response {
    let csp = format!(
        "default-src 'none'; script-src 'nonce-{nonce}'; script-src-attr 'none'; style-src 'unsafe-inline' https://cdn.jsdelivr.net; img-src https:; base-uri 'none'; form-action 'none'"
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

#[cfg(test)]
#[path = "markdown_tests.rs"]
mod tests;
