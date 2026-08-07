//! Toolbar + shared HTML helpers for preview pages.
//!
//! Preview pages render a sticky toolbar with the title and optional controls.
//! Share pages also receive a countdown; owner pages live with the session and
//! therefore do not show one.

use axum::body::Body;
use axum::http::StatusCode;
use axum::response::Response;

use common::previews::PreviewEntry;

pub(super) fn remaining_millis(entry: &PreviewEntry) -> Option<u128> {
    entry.expires_at.map(|expires_at| {
        expires_at
            .saturating_duration_since(std::time::Instant::now())
            .as_millis()
    })
}

pub(super) fn escape_html(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

pub(super) fn html_response(html: String) -> Response {
    Response::builder()
        .status(StatusCode::OK)
        .header("Content-Type", "text/html; charset=utf-8")
        .header("Cache-Control", "no-store")
        .header("Referrer-Policy", "no-referrer")
        .body(Body::from(html))
        .unwrap()
}

/// Minimal percent-encoder for query-string values: encodes anything
/// outside the unreserved set so the URL stays well-formed.
pub(super) fn url_encode_query(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.as_bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(*b as char);
            }
            _ => out.push_str(&format!("%{:02X}", b)),
        }
    }
    out
}

/// Toolbar HTML + countdown timer script.
pub(super) fn toolbar_and_timer(
    title: &str,
    subtitle: &str,
    remaining_ms: Option<u128>,
    extra_buttons: &str,
) -> String {
    let subtitle_html = if subtitle.is_empty() {
        String::new()
    } else {
        format!(r#"<span class="subtitle">{}</span>"#, subtitle)
    };
    let timer_html = remaining_ms.map_or_else(String::new, |remaining_ms| {
        let total_seconds = remaining_ms / 1000;
        let minutes = total_seconds / 60;
        let seconds = total_seconds % 60;
        format!(
            r#"<span class="badge" id="timer">{minutes}:{seconds:02}</span>
<script>
(function() {{
  var expiry = Date.now() + {remaining_ms};
  var el = document.getElementById('timer');
  setInterval(function() {{
    var left = Math.max(0, expiry - Date.now());
    var m = Math.floor(left / 60000);
    var s = Math.floor((left % 60000) / 1000);
    el.textContent = m + ':' + (s < 10 ? '0' : '') + s;
    if (left <= 0) {{
      el.textContent = 'Expired';
      el.style.background = '#6b2020';
      el.style.color = '#e5a5a5';
    }}
  }}, 1000);
}})();
</script>"#,
        )
    });
    format!(
        r#"<div class="toolbar">
  <span class="title">{title}</span>
  {subtitle_html}
  {timer_html}
  <span class="spacer"></span>
  {extra_buttons}
</div>"#,
        title = title,
        subtitle_html = subtitle_html,
        timer_html = timer_html,
        extra_buttons = extra_buttons,
    )
}

/// Toolbar CSS shared by all preview modes.
pub(super) const TOOLBAR_CSS: &str = r#"
  * { margin: 0; padding: 0; box-sizing: border-box; }
  .toolbar {
    position: sticky;
    top: 0;
    z-index: 100;
    height: 40px;
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 0 16px;
    background: #1a1a1a;
    border-bottom: 1px solid #333;
    font-size: 13px;
    font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, sans-serif;
    color: #eee;
  }
  .toolbar .title {
    font-weight: 600;
    color: #fff;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .toolbar .subtitle {
    color: #999;
    font-size: 12px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .toolbar .badge {
    background: #2d6a4f;
    color: #b7e4c7;
    padding: 2px 8px;
    border-radius: 10px;
    font-size: 11px;
    flex-shrink: 0;
  }
  .toolbar .spacer { flex: 1; }
  .toolbar button {
    background: #333;
    color: #ccc;
    border: 1px solid #444;
    padding: 4px 12px;
    border-radius: 4px;
    cursor: pointer;
    font-size: 12px;
  }
  .toolbar button:hover { background: #444; color: #fff; }
"#;
