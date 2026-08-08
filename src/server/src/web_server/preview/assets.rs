//! Preview assets embedded in the server binary.

use axum::body::Body;
use axum::http::StatusCode;
use axum::response::Response;

pub(in crate::web_server) const MARKED_SCRIPT_ROUTE: &str = "/preview/assets/marked-15.0.12.min.js";
pub(in crate::web_server) const DOMPURIFY_SCRIPT_ROUTE: &str =
    "/preview/assets/dompurify-3.4.12.min.js";
pub(in crate::web_server) const THEME_STYLESHEET_ROUTE: &str =
    concat!("/preview/assets/theme-", env!("CARGO_PKG_VERSION"), ".css");
const MARKED_SCRIPT: &str = include_str!("vendor/marked-15.0.12.min.js");
const DOMPURIFY_SCRIPT: &str = include_str!("vendor/dompurify-3.4.12.min.js");
const THEME_STYLESHEET: &str = include_str!("../../../../shared/ui/src/theme.css");

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
        assert_eq!(
            THEME_STYLESHEET_ROUTE,
            format!("/preview/assets/theme-{}.css", env!("CARGO_PKG_VERSION"))
        );
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let css = std::str::from_utf8(&body).unwrap();
        assert!(css.contains("--primary: oklch(0.55 0.18 180)"));
        assert!(css.contains("--radius: 0.625rem"));
    }
}
