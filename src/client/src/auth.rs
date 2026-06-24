use serde::Deserialize;

use crate::http::{path_with_query, AuthRequirement, HttpMethod, RequestSpec, ResponseSpec};
use crate::Result;

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct AuthFile {
    pub port: u16,
    pub token: String,
}

impl AuthFile {
    pub fn base_url(&self) -> String {
        format!("http://127.0.0.1:{}/va", self.port)
    }

    pub fn bearer_header_value(&self) -> String {
        bearer_header_value(&self.token)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct PairStartResponse {
    pub code: String,
    pub sid: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum PairStatusResponse {
    Pending,
    Expired,
    Verified { token: String },
}

pub fn parse_auth_file(body: &str) -> Result<AuthFile> {
    serde_json::from_str(body).map_err(crate::ClientError::Decode)
}

pub fn bearer_header_value(token: &str) -> String {
    format!("Bearer {}", token.trim())
}

pub fn pair_start() -> RequestSpec {
    RequestSpec::new(HttpMethod::Post, "/api/pair/start", AuthRequirement::None)
}

pub fn decode_pair_start(response: ResponseSpec) -> Result<PairStartResponse> {
    response.decode()
}

pub fn pair_status(sid: &str) -> RequestSpec {
    RequestSpec::new(
        HttpMethod::Get,
        path_with_query("/api/pair/status", &[("sid", Some(sid.to_string()))]),
        AuthRequirement::None,
    )
}

pub fn decode_pair_status(response: ResponseSpec) -> Result<PairStatusResponse> {
    response.decode()
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn parses_auth_file_without_reading_filesystem() {
        let auth = parse_auth_file(r#"{ "port": 12358, "token": "abc" }"#).expect("auth");
        assert_eq!(auth.base_url(), "http://127.0.0.1:12358/va");
        assert_eq!(auth.bearer_header_value(), "Bearer abc");
    }

    #[test]
    fn pair_status_request_is_public_and_query_encoded() {
        let request = pair_status("session/1");
        assert_eq!(request.method, HttpMethod::Get);
        assert_eq!(request.auth, AuthRequirement::None);
        assert_eq!(request.path, "/api/pair/status?sid=session%2F1");
    }

    #[test]
    fn decodes_verified_pair_status() {
        let status = decode_pair_status(ResponseSpec::json(
            200,
            json!({ "status": "verified", "token": "secret" }),
        ))
        .expect("status");
        assert_eq!(
            status,
            PairStatusResponse::Verified {
                token: "secret".into()
            }
        );
    }
}
