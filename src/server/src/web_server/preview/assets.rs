//! Preview assets embedded in the server binary.

use axum::body::Body;
use axum::http::StatusCode;
use axum::response::Response;

pub(in crate::web_server) const MARKED_SCRIPT_ROUTE: &str = "/preview/assets/marked-15.0.12.min.js";
pub(in crate::web_server) const DOMPURIFY_SCRIPT_ROUTE: &str =
    "/preview/assets/dompurify-3.4.12.min.js";
pub(in crate::web_server) const THEME_STYLESHEET_ROUTE: &str =
    concat!("/preview/assets/theme-", env!("CARGO_PKG_VERSION"), ".css");
pub(in crate::web_server) const REVIEW_BRIDGE_SCRIPT_ROUTE: &str = concat!(
    "/preview/assets/review-bridge-",
    env!("CARGO_PKG_VERSION"),
    ".js"
);
const MARKED_SCRIPT: &str = include_str!("vendor/marked-15.0.12.min.js");
const DOMPURIFY_SCRIPT: &str = include_str!("vendor/dompurify-3.4.12.min.js");
const THEME_STYLESHEET: &str = include_str!("../../../../shared/ui/src/theme.css");
const HTML2CANVAS_SCRIPT: &str = include_str!("vendor/html2canvas-pro-1.6.6.min.js");
const REVIEW_REGION_SCRIPT: &str = include_str!("review_region.js");
const REVIEW_BRIDGE_SCRIPT: &str = include_str!("review_bridge.js");

pub(in crate::web_server) async fn marked_script_handler() -> Response {
    script_response(MARKED_SCRIPT)
}

pub(in crate::web_server) async fn dompurify_script_handler() -> Response {
    script_response(DOMPURIFY_SCRIPT)
}

pub(in crate::web_server) async fn theme_stylesheet_handler() -> Response {
    Response::builder()
        .status(StatusCode::OK)
        .header("Content-Type", "text/css; charset=utf-8")
        .header("Cache-Control", "no-cache")
        .header("Cross-Origin-Resource-Policy", "same-origin")
        .header("X-Content-Type-Options", "nosniff")
        .body(Body::from(THEME_STYLESHEET))
        .unwrap()
}

pub(in crate::web_server) async fn review_bridge_script_handler() -> Response {
    let mut script = String::with_capacity(
        HTML2CANVAS_SCRIPT.len() + REVIEW_REGION_SCRIPT.len() + REVIEW_BRIDGE_SCRIPT.len() + 420,
    );
    script.push_str(
        "(()=>{const vaPreviewHtml2canvasDescriptor=Object.getOwnPropertyDescriptor(globalThis,\"html2canvas\");const module={exports:{}};const exports=module.exports;\n",
    );
    script.push_str(HTML2CANVAS_SCRIPT);
    script.push_str(
        "\nconst vaPreviewHtml2canvas=module.exports.default||module.exports.html2canvas;if(vaPreviewHtml2canvasDescriptor){Object.defineProperty(globalThis,\"html2canvas\",vaPreviewHtml2canvasDescriptor);}else{delete globalThis.html2canvas;}\n",
    );
    script.push_str(REVIEW_REGION_SCRIPT);
    script.push('\n');
    script.push_str(REVIEW_BRIDGE_SCRIPT);
    script.push_str("\n})();");
    Response::builder()
        .status(StatusCode::OK)
        .header("Content-Type", "text/javascript; charset=utf-8")
        .header("Cache-Control", "no-cache")
        .header("Access-Control-Allow-Origin", "*")
        .header("Cross-Origin-Resource-Policy", "cross-origin")
        .header("X-Content-Type-Options", "nosniff")
        .body(Body::from(script))
        .unwrap()
}

pub(super) fn theme_stylesheet_href() -> String {
    format!("/va{THEME_STYLESHEET_ROUTE}?v={}", std::process::id())
}

pub(in crate::web_server) fn review_bridge_script_href() -> String {
    format!("/va{REVIEW_BRIDGE_SCRIPT_ROUTE}?v={}", std::process::id())
}

fn script_response(script: &'static str) -> Response {
    Response::builder()
        .status(StatusCode::OK)
        .header("Content-Type", "text/javascript; charset=utf-8")
        .header("Cache-Control", "public, max-age=31536000, immutable")
        .header("Cross-Origin-Resource-Policy", "same-origin")
        .header("X-Content-Type-Options", "nosniff")
        .body(Body::from(script))
        .unwrap()
}

#[cfg(test)]
mod tests {
    use axum::body::to_bytes;

    use super::*;

    async fn assert_vendored_script(response: Response, length: usize, banner: &[u8]) {
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers().get("cache-control").unwrap(),
            "public, max-age=31536000, immutable"
        );
        assert_eq!(
            response
                .headers()
                .get("cross-origin-resource-policy")
                .unwrap(),
            "same-origin"
        );
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        assert_eq!(body.len(), length);
        assert!(body.windows(banner.len()).any(|window| window == banner));
    }

    #[tokio::test]
    async fn vendored_marked_script_is_versioned_and_immutable() {
        assert_vendored_script(marked_script_handler().await, 39_903, b"marked v15.0.12").await;
    }

    #[tokio::test]
    async fn vendored_dompurify_script_is_versioned_and_immutable() {
        assert_vendored_script(
            dompurify_script_handler().await,
            29_209,
            b"DOMPurify 3.4.12",
        )
        .await;
    }

    #[tokio::test]
    async fn preview_theme_uses_dashboard_tokens_and_is_versioned() {
        let response = theme_stylesheet_handler().await;
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers().get("content-type").unwrap(),
            "text/css; charset=utf-8"
        );
        assert_eq!(response.headers().get("cache-control").unwrap(), "no-cache");
        assert!(
            review_bridge_script_href().starts_with(&format!("/va{REVIEW_BRIDGE_SCRIPT_ROUTE}?v="))
        );
        assert_eq!(
            THEME_STYLESHEET_ROUTE,
            format!("/preview/assets/theme-{}.css", env!("CARGO_PKG_VERSION"))
        );
        assert_eq!(
            theme_stylesheet_href(),
            format!("/va{THEME_STYLESHEET_ROUTE}?v={}", std::process::id())
        );
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let css = std::str::from_utf8(&body).unwrap();
        assert!(css.contains("--primary: oklch(0.55 0.18 180)"));
        assert!(css.contains("--radius: 0.625rem"));
    }

    #[tokio::test]
    async fn review_bridge_is_versioned_and_loadable_by_local_dev_servers() {
        let response = review_bridge_script_handler().await;
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response
                .headers()
                .get("cross-origin-resource-policy")
                .unwrap(),
            "cross-origin"
        );
        assert_eq!(
            response
                .headers()
                .get("access-control-allow-origin")
                .unwrap(),
            "*"
        );
        assert_eq!(response.headers().get("cache-control").unwrap(), "no-cache");
        assert_eq!(
            REVIEW_BRIDGE_SCRIPT_ROUTE,
            format!(
                "/preview/assets/review-bridge-{}.js",
                env!("CARGO_PKG_VERSION")
            )
        );
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let script = std::str::from_utf8(&body).unwrap();
        assert!(script.contains("html2canvas-pro 1.6.6"));
        assert!(script.contains(
            "const vaPreviewHtml2canvas=module.exports.default||module.exports.html2canvas"
        ));
        assert!(script.contains("Object.defineProperty(globalThis,\"html2canvas\""));
        assert!(script.contains("createPreviewRegionPicker"));
        assert!(script.contains("region-pin"));
        assert!(script.contains("va-preview-review"));
        assert!(script.contains("documentId"));
        assert!(script.contains("data.documentId === documentId"));
        assert!(script.contains("Source line") || script.contains("startLine"));
    }
}
