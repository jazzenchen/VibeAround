//! Preview cookie parsing plus the public Share gate and grant cookie.

use axum::body::Body;
use axum::http::{header, HeaderValue, Request, StatusCode};
use axum::response::Response;
use common::previews::PreviewEntry;

use super::assets::theme_stylesheet_href;
use super::toolbar::remaining_millis;

pub(super) const SHARE_COOKIE_PREFIX: &str = "va_preview_share_";

pub(super) fn extract_cookie<B>(req: &Request<B>, name: &str) -> Option<String> {
    req.headers()
        .get_all(header::COOKIE)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|value| value.split(';'))
        .map(str::trim)
        .find_map(|pair| {
            let (key, value) = pair.split_once('=')?;
            (key.trim() == name).then(|| value.trim().to_string())
        })
}

#[derive(Debug, Clone, Copy)]
pub(super) enum AccessGateError {
    Incorrect,
    RateLimited(u64),
}

pub(super) fn share_cookie_name(share_id: &str) -> String {
    format!("{SHARE_COOKIE_PREFIX}{share_id}")
}

fn share_cookie_path(share_id: &str) -> String {
    format!("/va/preview/s/{share_id}")
}

pub(super) fn share_grant_cookie(share_id: &str, grant: &str, entry: &PreviewEntry) -> String {
    let max_age = remaining_millis(entry)
        .map(|milliseconds| (milliseconds / 1000).max(1))
        .unwrap_or(1);
    format!(
        "{}={}; Path={}; Max-Age={}; Secure; HttpOnly; SameSite=Lax",
        share_cookie_name(share_id),
        grant,
        share_cookie_path(share_id),
        max_age
    )
}

pub(super) fn clear_share_cookie(share_id: &str) -> String {
    format!(
        "{}=; Path={}; Max-Age=0; Secure; HttpOnly; SameSite=Lax",
        share_cookie_name(share_id),
        share_cookie_path(share_id)
    )
}

pub(super) fn render_access_gate(entry: &PreviewEntry, error: Option<AccessGateError>) -> Response {
    let nonce = uuid::Uuid::new_v4().simple().to_string();
    let remaining_ms = remaining_millis(entry).unwrap_or_default();
    let code_length = common::previews::SHARE_CODE_LENGTH;
    let last_slot_index = code_length.saturating_sub(1);
    let slots = r#"<span class="slot"></span>"#.repeat(code_length);
    let (status, error_message) = match error {
        Some(AccessGateError::Incorrect) => (
            StatusCode::UNAUTHORIZED,
            "That access code is incorrect. Check the shared message and try again.".to_string(),
        ),
        Some(AccessGateError::RateLimited(seconds)) => (
            StatusCode::TOO_MANY_REQUESTS,
            format!("Too many attempts. Try again in {seconds} seconds."),
        ),
        None => (StatusCode::OK, String::new()),
    };
    let invalid = if error.is_some() { "true" } else { "false" };
    let invalid_class = if error.is_some() { " invalid" } else { "" };
    let html = format!(
        r#"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>VibeAround Preview</title>
<link rel="stylesheet" href="{theme_stylesheet_href}">
<style>
  * {{ box-sizing: border-box; }}
  body {{
    min-height: 100dvh;
    margin: 0;
    display: grid;
    place-items: center;
    padding: 24px;
    background: var(--background);
    color: var(--foreground);
    font-family: -apple-system, BlinkMacSystemFont, "SF Pro Text", "Segoe UI", ui-sans-serif, system-ui, sans-serif;
  }}
  main {{
    width: min(100%, 448px);
    display: grid;
    gap: 24px;
  }}
  .intro {{ display: flex; align-items: center; gap: 12px; }}
  .mark {{ width: 40px; height: 40px; flex: 0 0 auto; }}
  h1 {{ margin: 0 0 3px; font-size: 16px; font-weight: 600; letter-spacing: -.01em; }}
  .hint {{ margin: 0; color: var(--muted-foreground); font-size: 12px; line-height: 1.5; }}
  form {{
    padding: 24px;
    border: 1px solid var(--border);
    border-radius: var(--radius);
    background: color-mix(in oklab, var(--card) 50%, transparent);
  }}
  .otp {{ position: relative; margin: 0 auto 18px; }}
  .otp input {{
    position: absolute;
    inset: 0;
    z-index: 2;
    width: 100%;
    height: 100%;
    border: 0;
    opacity: .01;
    color: transparent;
    caret-color: transparent;
    font-size: 16px;
    cursor: text;
  }}
  .slots {{ display: grid; grid-template-columns: repeat({code_length}, 1fr); gap: 8px; }}
  .slot {{
    display: grid;
    place-items: center;
    aspect-ratio: 1;
    border: 1px solid var(--input);
    border-radius: calc(var(--radius) - 2px);
    background: var(--background);
    color: var(--foreground);
    font: 600 24px "JetBrains Mono", "SF Mono", ui-monospace, SFMono-Regular, Menlo, monospace;
  }}
  .slot.filled {{ border-color: var(--muted-foreground); background: var(--card); }}
  .otp:focus-within .slot.active {{
    border-color: var(--ring);
    box-shadow: 0 0 0 3px color-mix(in oklab, var(--ring) 25%, transparent);
  }}
  .otp.invalid .slot {{ border-color: var(--destructive); }}
  button {{
    width: 100%;
    min-height: 36px;
    border: 0;
    border-radius: calc(var(--radius) - 2px);
    background: var(--primary);
    color: var(--primary-foreground);
    font-size: 14px;
    font-weight: 600;
    cursor: pointer;
    transition: opacity 120ms ease;
  }}
  button:disabled {{ opacity: .5; cursor: not-allowed; }}
  button:focus-visible {{
    outline: 3px solid color-mix(in oklab, var(--ring) 35%, transparent);
    outline-offset: 1px;
  }}
  .error {{ min-height: 20px; margin: 0 0 12px; color: var(--destructive); font-size: 12px; }}
  .expiry {{ margin: 16px 0 0; color: var(--muted-foreground); font-size: 12px; text-align: center; }}
  @media (max-width: 480px) {{
    body {{ padding: 16px; }}
    main {{ gap: 20px; }}
    form {{ padding: 20px 18px; }}
    .slots {{ gap: 6px; }}
    .slot {{ font-size: 21px; }}
  }}
</style>
</head>
<body>
<main>
  <header class="intro">
    <img class="mark" src="/va/brand/vibearound-mark.svg" alt="">
    <div>
      <h1>Enter access code</h1>
      <p class="hint">Use the {code_length}-digit code shared by the owner.</p>
    </div>
  </header>
  <form method="post">
    <div class="otp{invalid_class}">
      <input id="access-code" name="code" type="text" inputmode="numeric"
        pattern="[0-9]{{{code_length}}}" autocomplete="one-time-code"
        enterkeyhint="go" aria-label="{code_length}-digit access code"
        aria-invalid="{invalid}" aria-describedby="access-error expiry" autofocus required>
      <div class="slots" aria-hidden="true">
        {slots}
      </div>
    </div>
    <p class="error" id="access-error" role="alert">{error_message}</p>
    <button id="submit" type="submit">Open preview</button>
  </form>
  <p class="expiry" id="expiry"></p>
</main>
<script nonce="{nonce}">
(function() {{
  var input = document.getElementById('access-code');
  var button = document.getElementById('submit');
  var slots = document.querySelectorAll('.slot');
  var expiry = Date.now() + {remaining_ms};
  var expiryEl = document.getElementById('expiry');
  function renderCode() {{
    var value = input.value.replace(/\D/g, '').slice(0, {code_length});
    if (input.value !== value) input.value = value;
    slots.forEach(function(slot, index) {{
      slot.textContent = value[index] || '';
      slot.classList.toggle('filled', index < value.length);
      slot.classList.toggle('active', index === Math.min(value.length, {last_slot_index}));
    }});
    button.disabled = value.length !== {code_length} || Date.now() >= expiry;
  }}
  function renderExpiry() {{
    var left = Math.max(0, expiry - Date.now());
    var minutes = Math.floor(left / 60000);
    var seconds = Math.floor((left % 60000) / 1000);
    expiryEl.textContent = left > 0
      ? 'Access code expires in ' + minutes + ':' + (seconds < 10 ? '0' : '') + seconds
      : 'This access code has expired.';
    if (left <= 0) {{
      input.disabled = true;
      button.disabled = true;
    }}
  }}
  input.addEventListener('input', renderCode);
  input.addEventListener('focus', renderCode);
  input.addEventListener('blur', renderCode);
  renderCode();
  renderExpiry();
  setInterval(renderExpiry, 1000);
}})();
</script>
</body>
</html>"#,
        theme_stylesheet_href = theme_stylesheet_href(),
    );

    let csp = format!(
        "default-src 'none'; script-src 'nonce-{nonce}'; script-src-attr 'none'; style-src 'self' 'unsafe-inline'; img-src 'self'; form-action 'self'; base-uri 'none'; frame-ancestors 'none'"
    );
    Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, "text/html; charset=utf-8")
        .header("Content-Security-Policy", csp)
        .header("Referrer-Policy", "no-referrer")
        .header("X-Content-Type-Options", "nosniff")
        .body(Body::from(html))
        .unwrap()
}

pub(super) fn with_retry_after(mut response: Response, seconds: u64) -> Response {
    response.headers_mut().insert(
        header::RETRY_AFTER,
        HeaderValue::from_str(&seconds.to_string()).expect("retry-after is numeric"),
    );
    response
}
