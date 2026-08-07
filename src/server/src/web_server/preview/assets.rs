//! Versioned third-party assets embedded in the server binary.

use axum::body::Body;
use axum::http::StatusCode;
use axum::response::Response;

pub(in crate::web_server) const MARKED_SCRIPT_ROUTE: &str = "/preview/assets/marked-15.0.12.min.js";
const MARKED_SCRIPT: &str = include_str!("vendor/marked-15.0.12.min.js");

pub(in crate::web_server) async fn marked_script_handler() -> Response {
    Response::builder()
        .status(StatusCode::OK)
        .header("Content-Type", "text/javascript; charset=utf-8")
        .header("Cache-Control", "public, max-age=31536000, immutable")
        .header("Cross-Origin-Resource-Policy", "same-origin")
        .header("X-Content-Type-Options", "nosniff")
        .body(Body::from(MARKED_SCRIPT))
        .unwrap()
}

#[cfg(test)]
mod tests {
    use axum::body::to_bytes;

    use super::*;

    #[tokio::test]
    async fn vendored_marked_script_is_versioned_and_immutable() {
        let response = marked_script_handler().await;
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
        assert_eq!(body.len(), 39_903);
        assert!(body
            .windows(b"marked v15.0.12".len())
            .any(|window| window == b"marked v15.0.12"));
    }
}
