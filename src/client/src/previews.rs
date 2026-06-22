use serde::Deserialize;

use crate::http::{join_path, AuthRequirement, HttpMethod, RequestSpec, ResponseSpec};
use crate::Result;

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct PreviewSnapshot {
    pub slug: String,
    pub id: String,
    pub workspace: String,
    pub title: String,
    pub kind: PreviewKind,
    pub port: Option<u16>,
    pub share_key: Option<String>,
    pub share_expires_at_ms: Option<u64>,
    pub created_at_ms: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PreviewKind {
    Server,
    File,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct PreviewsResponse {
    pub previews: Vec<PreviewSnapshot>,
    pub tunnel_url: Option<String>,
}

pub fn list() -> RequestSpec {
    RequestSpec::new(
        HttpMethod::Get,
        "/api/previews",
        AuthRequirement::BearerToken,
    )
}

pub fn decode_list(response: ResponseSpec) -> Result<PreviewsResponse> {
    response.decode()
}

pub fn delete(slug: &str) -> RequestSpec {
    RequestSpec::new(
        HttpMethod::Delete,
        join_path("/api/previews", slug),
        AuthRequirement::BearerToken,
    )
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn delete_preview_encodes_slug() {
        let request = delete("hello/world");
        assert_eq!(request.method, HttpMethod::Delete);
        assert_eq!(request.path, "/api/previews/hello%2Fworld");
    }

    #[test]
    fn decodes_preview_response() {
        let response = ResponseSpec::json(
            200,
            json!({
                "previews": [{
                    "slug": "abc",
                    "id": "abc",
                    "workspace": "/tmp/project",
                    "title": "App",
                    "kind": "server",
                    "port": 5173,
                    "share_key": null,
                    "share_expires_at_ms": null,
                    "created_at_ms": 123
                }],
                "tunnel_url": "https://example.com"
            }),
        );
        let previews = decode_list(response).expect("decode");
        assert_eq!(previews.previews[0].kind, PreviewKind::Server);
        assert_eq!(previews.tunnel_url.as_deref(), Some("https://example.com"));
    }
}
