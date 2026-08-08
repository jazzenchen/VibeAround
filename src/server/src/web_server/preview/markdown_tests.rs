use std::time::{Duration, Instant};

use axum::body::to_bytes;
use axum::http::{HeaderMap, StatusCode};
use common::previews::{PreviewEntry, PreviewTarget};

use super::{render_md_content, render_md_page};

fn unique_temp_dir() -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "vibearound-markdown-preview-{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

async fn render(markdown: &str, title: &str, expires_at: Option<Instant>) -> (HeaderMap, String) {
    let workspace = unique_temp_dir();
    let file = workspace.join("preview.md");
    std::fs::write(&file, markdown).unwrap();
    let entry = PreviewEntry {
        id: file,
        workspace,
        title: title.to_string(),
        target: PreviewTarget::File,
        created_at: Instant::now(),
        expires_at,
    };

    let response = render_md_page(&entry).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let headers = response.headers().clone();
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    (headers, String::from_utf8(body.to_vec()).unwrap())
}

async fn render_embedded(markdown: &str, title: &str) -> (HeaderMap, String) {
    let workspace = unique_temp_dir();
    let file = workspace.join("preview.md");
    std::fs::write(&file, markdown).unwrap();
    let entry = PreviewEntry {
        id: file,
        workspace,
        title: title.to_string(),
        target: PreviewTarget::File,
        created_at: Instant::now(),
        expires_at: None,
    };

    let response = render_md_content(&entry).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let headers = response.headers().clone();
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    (headers, String::from_utf8(body.to_vec()).unwrap())
}

fn assert_security_headers(headers: &HeaderMap, frame_ancestors: &str) -> String {
    assert_eq!(headers.get("referrer-policy").unwrap(), "no-referrer");
    assert_eq!(headers.get("x-content-type-options").unwrap(), "nosniff");

    let csp = headers
        .get("content-security-policy")
        .unwrap()
        .to_str()
        .unwrap();
    assert!(csp.contains("default-src 'none'"));
    assert!(csp.contains("script-src-attr 'none'"));
    assert!(csp.contains("style-src 'unsafe-inline' https://cdn.jsdelivr.net"));
    assert!(csp.contains("img-src https:"));
    assert!(csp.contains("base-uri 'none'"));
    assert!(csp.contains("form-action 'none'"));
    assert!(csp.contains(&format!("frame-ancestors {frame_ancestors}")));

    let script_source = csp
        .split(';')
        .map(str::trim)
        .find(|directive| directive.starts_with("script-src "))
        .unwrap();
    assert!(!script_source.contains("https://cdn.jsdelivr.net"));
    assert!(!script_source.contains("'unsafe-inline'"));
    script_source
        .split_whitespace()
        .find_map(|source| {
            source
                .strip_prefix("'nonce-")
                .and_then(|nonce| nonce.strip_suffix('\''))
        })
        .unwrap()
        .to_string()
}

fn assert_all_scripts_use_nonce(body: &str, nonce: &str) {
    assert_eq!(
        body.matches("<script").count(),
        body.matches(&format!(r#"<script nonce="{nonce}""#)).count(),
        "every trusted script must carry the response nonce"
    );
}

fn markdown_source(body: &str) -> (&str, String) {
    let marker = "id=\"markdown-source\" type=\"application/json\">";
    let start = body.find(marker).unwrap() + marker.len();
    let end = body[start..].find("</script>").unwrap() + start;
    let json = &body[start..end];
    let markdown = serde_json::from_str(json).unwrap();
    (json, markdown)
}

fn normalized_csp_without_frame_ancestors(headers: &HeaderMap) -> String {
    let csp = headers
        .get("content-security-policy")
        .unwrap()
        .to_str()
        .unwrap();
    csp.split(';')
        .map(str::trim)
        .filter(|directive| !directive.starts_with("frame-ancestors "))
        .collect::<Vec<_>>()
        .join("; ")
        .split_whitespace()
        .map(|part| {
            if part.starts_with("'nonce-") {
                "'nonce-<dynamic>'"
            } else {
                part
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

#[tokio::test]
async fn markdown_page_keeps_client_renderer_and_carries_source_as_inert_json() {
    let markdown = r#"# Safe heading

~~removed~~

| A | B |
|---|---|
| 1 | 2 |

- [x] complete

<script>alert('raw')</script>
"#;
    let (headers, body) = render(
        markdown,
        "</title><script>title-broke</script>",
        Some(Instant::now() + Duration::from_secs(600)),
    )
    .await;

    let nonce = assert_security_headers(&headers, "'none'");
    assert_all_scripts_use_nonce(&body, &nonce);
    assert!(body.contains("src=\"/va/preview/assets/marked-15.0.12.min.js\""));
    assert!(!body.contains("marked@15"));
    assert!(!body.contains("integrity="));
    assert!(body.contains("marked.parse(raw, { gfm: true, renderer })"));
    assert!(body.contains("renderer.html"));
    assert!(body.contains("renderer.image"));
    assert!(body.contains("linkUrl"));
    assert!(body.contains("<title>&lt;/title&gt;&lt;script&gt;title-broke&lt;/script&gt;</title>"));

    let (json, decoded) = markdown_source(&body);
    assert_eq!(decoded, markdown);
    assert!(!json.contains('<'));
    assert!(json.contains("\\u003cscript\\u003e"));
}

#[tokio::test]
async fn markdown_page_encodes_html_breakouts_before_the_browser_parses_the_page() {
    let markdown = r#"</ScRiPt><img src=x onerror=alert(1)>

</script ><iframe srcdoc="<script>alert(1)</script>"></iframe>

</article><img src=x onerror=alert(1)>

<details open ontoggle=alert(1)>toggle</details>

<style>@import url(https://evil.example/style.css)</style>

<meta http-equiv=refresh content="0;url=https://evil.example"><base href="https://evil.example">

<svg><set attributeName=href to=javascript:alert(1)></set></svg>

<math><annotation-xml><img src=x onerror=alert(1)></annotation-xml></math>

![http](http://images.example.test/x.png)
![data](data:image/png;base64,AAAA)
![relative](/x.png)
![scheme-relative](//images.example.test/x.png)
![missing-host](https:javascript)
![https](https://images.example.test/safe.png)

[javascript](javascript:alert(1))
[vbscript](vbscript:msgbox(1))
[data](data:text/html,<script>alert(1)</script>)
[entity](&#x6a;avascript:alert(1))
"#;
    let (headers, body) = render(markdown, "Breakouts", None).await;

    let nonce = assert_security_headers(&headers, "'none'");
    assert_all_scripts_use_nonce(&body, &nonce);
    let (json, decoded) = markdown_source(&body);
    assert_eq!(decoded, markdown);
    assert!(!json.contains('<'));
    assert!(!body.contains("</ScRiPt><img src=x"));
    assert!(!body.contains("</script ><iframe"));
    assert!(!body.contains("</article><img src=x"));
    assert!(json.contains("\\u003c/ScRiPt\\u003e"));
    assert!(json.contains("\\u003cmeta http-equiv"));
}

#[tokio::test]
async fn embedded_owner_and_share_markdown_share_content_security_policy() {
    let (owner_headers, owner_body) = render_embedded("owner", "Owner").await;
    let (share_headers, share_body) = render(
        "share",
        "Share",
        Some(Instant::now() + Duration::from_secs(600)),
    )
    .await;

    let owner_nonce = assert_security_headers(&owner_headers, "'self'");
    let share_nonce = assert_security_headers(&share_headers, "'none'");
    assert_all_scripts_use_nonce(&owner_body, &owner_nonce);
    assert_all_scripts_use_nonce(&share_body, &share_nonce);
    assert_eq!(
        normalized_csp_without_frame_ancestors(&owner_headers),
        normalized_csp_without_frame_ancestors(&share_headers)
    );
    assert!(!owner_body.contains("id=\"timer\""));
    assert!(!owner_body.contains("class=\"toolbar\""));
    assert!(share_body.contains("id=\"timer\""));
    assert!(!owner_body.contains("preview.md"));
    assert!(!share_body.contains("preview.md"));
}
