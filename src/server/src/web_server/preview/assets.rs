//! Versioned third-party assets embedded in the server binary.

use axum::body::Body;
use axum::http::StatusCode;
use axum::response::Response;

pub(in crate::web_server) const MARKED_SCRIPT_ROUTE: &str = "/preview/assets/marked-15.0.12.min.js";
pub(in crate::web_server) const DOMPURIFY_SCRIPT_ROUTE: &str =
    "/preview/assets/dompurify-3.4.12.min.js";
const MARKED_SCRIPT: &str = include_str!("vendor/marked-15.0.12.min.js");
const DOMPURIFY_SCRIPT: &str = include_str!("vendor/dompurify-3.4.12.min.js");

pub(in crate::web_server) async fn marked_script_handler() -> Response {
    script_response(MARKED_SCRIPT)
}

pub(in crate::web_server) async fn dompurify_script_handler() -> Response {
    script_response(DOMPURIFY_SCRIPT)
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
}
