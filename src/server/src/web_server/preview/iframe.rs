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
const OWNER_TRANSCRIPT_JS: &str = include_str!("owner_transcript.js");
const OWNER_CHAT_JS: &str = include_str!("owner_chat.js");
const OWNER_REVIEW_PROMPT_JS: &str = include_str!("owner_review_prompt.js");
const OWNER_ANNOTATIONS_JS: &str = include_str!("owner_annotations.js");
const OWNER_FRAME_BRIDGE_JS: &str = include_str!("owner_frame_bridge.js");
const OWNER_SHELL_CSS: &str = include_str!("owner_shell.css");
const OWNER_CHAT_CSS: &str = include_str!("owner_chat.css");

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
{owner_shell_css}
{owner_chat_css}
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
<button type="button" class="chat-toggle" id="preview-chat-toggle" aria-label="Preview conversation" aria-controls="preview-chat-drawer" aria-expanded="false">
  <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" aria-hidden="true"><path d="M21 15a4 4 0 0 1-4 4H8l-5 3V7a4 4 0 0 1 4-4h10a4 4 0 0 1 4 4z"></path></svg>
  <span>Chat</span><span class="review-count" id="preview-review-count" aria-hidden="true" hidden></span><span class="chat-attention" id="preview-chat-attention" hidden></span>
</button>
<aside class="chat-drawer" id="preview-chat-drawer" aria-label="Preview conversation" aria-hidden="true">
  <header class="chat-header">
    <div><strong>Preview conversation</strong><span class="chat-status" id="preview-chat-status">Connecting…</span></div>
    <div class="chat-header-actions">
      <button type="button" id="preview-chat-refresh" hidden>Refresh preview</button>
      <button type="button" class="icon-button" id="preview-chat-close" aria-label="Close conversation">×</button>
    </div>
  </header>
  <div class="chat-log" id="preview-chat-log" role="log" aria-live="polite" aria-relevant="additions text"></div>
  <div class="chat-permissions" id="preview-chat-permissions"></div>
  <form class="chat-composer" id="preview-chat-form">
    <div class="review-chips" id="preview-review-chips" aria-label="Draft review comments" hidden></div>
    <p class="review-feedback" id="preview-review-feedback" role="status" hidden></p>
    <label class="sr-only" for="preview-chat-input">Message the AI task</label>
    <textarea id="preview-chat-input" rows="1" maxlength="20000" placeholder="Ask for a change…"></textarea>
    <div class="chat-actions">
      <button type="button" class="review-tool" id="preview-select-element" hidden>Select element</button>
      <span class="chat-actions-spacer"></span>
      <button type="submit" class="primary-button round-button" id="preview-chat-action" aria-label="Send" title="Send">
        <svg id="preview-chat-send-icon" viewBox="0 0 24 24" aria-hidden="true"><path d="m5 12 7-7 7 7"></path><path d="M12 19V5"></path></svg>
        <svg id="preview-chat-stop-icon" viewBox="0 0 24 24" aria-hidden="true" hidden><rect x="7" y="7" width="10" height="10" rx="1"></rect></svg>
      </button>
    </div>
  </form>
</aside>
<form class="comment-popover" id="preview-comment-popover" hidden>
  <span class="comment-location" id="preview-comment-location"></span>
  <blockquote id="preview-comment-selection"></blockquote>
  <label class="sr-only" for="preview-comment-input">Comment on selected content</label>
  <textarea id="preview-comment-input" rows="3" maxlength="2000" placeholder="What should change?"></textarea>
  <div class="comment-actions">
    <button type="button" id="preview-comment-cancel">Cancel</button>
    <button type="submit" class="primary-button">Save</button>
  </div>
</form>
<div class="review-mode" id="preview-review-mode" role="status" hidden>Click an element to comment · Esc to cancel</div>
<iframe id="preview-frame" title="Preview content — {title}" src="{initial_src}" referrerpolicy="no-referrer"></iframe>
<script nonce="{nonce}">
{owner_client_js}
</script>
<script nonce="{nonce}">
{owner_transcript_js}
</script>
<script nonce="{nonce}">
{owner_chat_js}
</script>
<script nonce="{nonce}">
{owner_review_prompt_js}
</script>
<script nonce="{nonce}">
{owner_annotations_js}
</script>
<script nonce="{nonce}">
{owner_frame_bridge_js}
</script>
</body>
</html>"#,
        title = title,
        options = options,
        initial_src = initial_src,
        nonce = nonce,
        owner_client_js = OWNER_CLIENT_JS,
        owner_transcript_js = OWNER_TRANSCRIPT_JS,
        owner_chat_js = OWNER_CHAT_JS,
        owner_review_prompt_js = OWNER_REVIEW_PROMPT_JS,
        owner_annotations_js = OWNER_ANNOTATIONS_JS,
        owner_frame_bridge_js = OWNER_FRAME_BRIDGE_JS,
        owner_shell_css = OWNER_SHELL_CSS,
        owner_chat_css = OWNER_CHAT_CSS,
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
        let chat_available =
            common::previews::owner_conversation_thread_id(&preview.slug).is_some();
        html.push_str(&format!(
            r#"<option value="{}" data-title="{}" data-src="{}" data-chat-available="{}"{}>{}</option>"#,
            escape_html(&preview.slug),
            escape_html(&preview.title),
            escape_html(&src),
            chat_available,
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
