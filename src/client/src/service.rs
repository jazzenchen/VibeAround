use serde::Deserialize;

use crate::http::{AuthRequirement, HttpMethod, RequestSpec, ResponseSpec};
use crate::Result;

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct ServiceHealthResponse {
    pub ok: bool,
    pub service: String,
    pub version: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct ServiceInfoResponse {
    pub service: String,
    pub version: String,
    pub port: u16,
    pub mode: String,
    pub auth_mode: String,
    pub data_dir: String,
    pub settings_path: String,
    pub web_dist_path: String,
    pub host_search_available: bool,
    pub replace_provider_web_search: bool,
}

pub fn health() -> RequestSpec {
    RequestSpec::new(
        HttpMethod::Get,
        "/api/service/health",
        AuthRequirement::None,
    )
}

pub fn decode_health(response: ResponseSpec) -> Result<ServiceHealthResponse> {
    response.decode()
}

pub fn info() -> RequestSpec {
    RequestSpec::new(
        HttpMethod::Get,
        "/api/service/info",
        AuthRequirement::BearerToken,
    )
}

pub fn decode_info(response: ResponseSpec) -> Result<ServiceInfoResponse> {
    response.decode()
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn health_request_is_public_liveness_probe() {
        let request = health();
        assert_eq!(request.method, HttpMethod::Get);
        assert_eq!(request.path, "/api/service/health");
        assert_eq!(request.auth, AuthRequirement::None);
        assert!(request.body.is_none());
    }

    #[test]
    fn decodes_service_health() {
        let response = ResponseSpec::json(
            200,
            json!({
                "ok": true,
                "service": "vibearound-server",
                "version": "0.7.11",
            }),
        );

        let health = decode_health(response).expect("decode health");

        assert!(health.ok);
    }

    #[test]
    fn decodes_service_info() {
        let response = ResponseSpec::json(
            200,
            json!({
                "service": "vibearound-server",
                "version": "0.7.12",
                "port": 12358,
                "mode": "server",
                "auth_mode": "token",
                "data_dir": "/tmp/va",
                "settings_path": "/tmp/va/settings.json",
                "web_dist_path": "/tmp/va/web",
                "host_search_available": true,
                "replace_provider_web_search": false
            }),
        );

        let info = decode_info(response).expect("decode info");
        assert_eq!(info.port, 12358);
        assert!(info.host_search_available);
    }
}
