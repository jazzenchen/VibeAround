//! Toolbar + shared HTML helpers for preview pages.
//!
//! Standalone Markdown pages render a sticky toolbar with the title and an
//! optional countdown. The owner shell has its own floating picker.

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
    script_nonce: Option<&str>,
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
        let nonce = script_nonce
            .map(|nonce| format!(r#" nonce="{}""#, escape_html(nonce)))
            .unwrap_or_default();
        format!(
            r#"<span class="badge" id="timer">{minutes}:{seconds:02}</span>
<script{nonce}>
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
      el.classList.add('expired');
    }}
  }}, 1000);
}})();
</script>"#,
        )
    });
    format!(
        r#"<div class="toolbar">
  <img class="mark" src="/va/brand/vibearound-mark.svg" alt="">
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
    height: 44px;
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 0 14px;
    background: var(--popover);
    border-bottom: 1px solid var(--border);
    font-size: 13px;
    font-family: -apple-system, BlinkMacSystemFont, "SF Pro Text", "Segoe UI", ui-sans-serif, system-ui, sans-serif;
    color: var(--popover-foreground);
  }
  .toolbar .mark {
    width: 22px;
    height: 22px;
    flex: 0 0 auto;
  }
  .toolbar .title {
    font-weight: 600;
    color: var(--popover-foreground);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .toolbar .subtitle {
    color: var(--muted-foreground);
    font-size: 12px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .toolbar .badge {
    background: color-mix(in oklab, var(--primary) 12%, transparent);
    color: var(--primary);
    padding: 2px 8px;
    border-radius: 10px;
    font-size: 11px;
    flex-shrink: 0;
  }
  .toolbar .badge.expired {
    background: color-mix(in oklab, var(--destructive) 12%, transparent);
    color: var(--destructive);
  }
  .toolbar .spacer { flex: 1; }
  .toolbar button {
    background: var(--background);
    color: var(--foreground);
    border: 1px solid var(--border);
    padding: 4px 12px;
    border-radius: calc(var(--radius) - 2px);
    cursor: pointer;
    font-size: 12px;
  }
  .toolbar button:hover { background: var(--accent); color: var(--accent-foreground); }
  .toolbar button:focus-visible {
    outline: 3px solid color-mix(in oklab, var(--ring) 35%, transparent);
    outline-offset: 1px;
  }
"#;
