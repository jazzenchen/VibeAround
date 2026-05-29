use axum::body::Body;
use axum::http::{header, HeaderMap as InboundHeaderMap, StatusCode};
use axum::response::Response;
use serde_json::json;
use std::collections::BTreeMap;

use common::profiles::schema::ProfileDef;
use common::profiles::{catalog, normalize_legacy_profile_and_persist, schema};

use super::{json_error, BridgeProtocol};

pub(super) struct UpstreamEndpoint {
    pub(super) url: String,
    pub(super) protocol: BridgeProtocol,
    pub(super) profile: ProfileDef,
    pub(super) headers: BTreeMap<String, String>,
    pub(super) auth_header: bool,
}

pub(super) fn upstream_endpoint(
    profile_id: &str,
    target_api_type: &str,
) -> Result<UpstreamEndpoint, (StatusCode, String)> {
    let protocol = BridgeProtocol::from_api_type(target_api_type).ok_or_else(|| {
        (
            StatusCode::BAD_REQUEST,
            format!("unsupported bridge target api kind '{target_api_type}'"),
        )
    })?;
    let profile = schema::load(profile_id)
        .map(normalize_legacy_profile_and_persist)
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                format!("profile '{profile_id}' not found"),
            )
        })?;
    if !profile
        .api_types
        .iter()
        .any(|api_type| api_type == target_api_type)
    {
        return Err((
            StatusCode::BAD_REQUEST,
            format!(
                "profile '{}' does not expose api kind '{}'",
                profile.id, target_api_type
            ),
        ));
    }
    let provider = catalog::get(&profile.provider).ok_or_else(|| {
        (
            StatusCode::BAD_REQUEST,
            format!("unknown provider '{}'", profile.provider),
        )
    })?;
    let endpoint_id = profile
        .overrides
        .get(target_api_type)
        .and_then(|overrides| overrides.endpoint_id.as_deref());
    let endpoint =
        catalog::find_endpoint(provider, target_api_type, endpoint_id).ok_or_else(|| {
            let suffix = endpoint_id
                .map(|id| format!(" endpoint_id '{id}'"))
                .unwrap_or_default();
            (
                StatusCode::BAD_REQUEST,
                format!(
                    "provider '{}' does not expose api kind '{}'{}",
                    profile.provider, target_api_type, suffix
                ),
            )
        })?;
    let base_url = profile
        .overrides
        .get(target_api_type)
        .and_then(|overrides| overrides.base_url.clone())
        .unwrap_or_else(|| endpoint.default_base_url.clone());
    let base_url = base_url.trim_end_matches('/');
    if base_url.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            format!(
                "profile '{}' has no base URL for api kind '{}'",
                profile.id, target_api_type
            ),
        ));
    }
    if protocol == BridgeProtocol::GeminiGenerateContent {
        return Err((
            StatusCode::BAD_REQUEST,
            concat!(
                "Gemini GenerateContent is supported as a client protocol only; ",
                "choose an OpenAI-compatible Gemini endpoint as the bridge target"
            )
            .to_string(),
        ));
    }

    let url = match protocol {
        BridgeProtocol::OpenAiResponses => {
            join_protocol_endpoint(base_url, "responses", endpoint.append_v1_path)
        }
        BridgeProtocol::OpenAiChat => {
            join_protocol_endpoint(base_url, "chat/completions", endpoint.append_v1_path)
        }
        BridgeProtocol::AnthropicMessages => {
            join_protocol_endpoint(base_url, "messages", endpoint.append_v1_path)
        }
        BridgeProtocol::GeminiGenerateContent => unreachable!("Gemini target is rejected above"),
    };
    Ok(UpstreamEndpoint {
        url,
        protocol,
        profile,
        headers: endpoint.headers.clone(),
        auth_header: endpoint.auth_header,
    })
}

fn join_protocol_endpoint(base_url: &str, endpoint: &str, append_v1_path: bool) -> String {
    if !append_v1_path || base_url.ends_with("/v1") {
        format!("{base_url}/{endpoint}")
    } else {
        format!("{base_url}/v1/{endpoint}")
    }
}

pub(super) fn apply_upstream_auth(
    request: reqwest::RequestBuilder,
    protocol: BridgeProtocol,
    auth_header: bool,
    headers: &InboundHeaderMap,
    profile_api_key: Option<&str>,
) -> Result<reqwest::RequestBuilder, Response> {
    let profile_api_key = profile_api_key
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let api_key = profile_api_key
        .map(ToString::to_string)
        .or_else(|| inbound_api_key(headers));
    if protocol.is_openai_family() || auth_header {
        let auth = match profile_api_key {
            Some(key) => Some(format!("Bearer {key}")),
            None => authorization_header(headers)
                .or_else(|| api_key.as_ref().map(|key| format!("Bearer {key}"))),
        };
        let Some(auth) = auth else {
            return Err(json_error(
                StatusCode::UNAUTHORIZED,
                "missing Authorization or x-api-key header",
            ));
        };
        let request = request.header(reqwest::header::AUTHORIZATION, auth);
        if protocol == BridgeProtocol::AnthropicMessages {
            let anthropic_version = headers
                .get("anthropic-version")
                .and_then(|value| value.to_str().ok())
                .unwrap_or("2023-06-01");
            return Ok(request.header("anthropic-version", anthropic_version));
        }
        return Ok(request);
    }

    let Some(api_key) = api_key else {
        return Err(json_error(
            StatusCode::UNAUTHORIZED,
            "missing x-api-key or Authorization header",
        ));
    };
    let mut request = if protocol == BridgeProtocol::GeminiGenerateContent {
        request.header("x-goog-api-key", api_key)
    } else {
        request.header("x-api-key", api_key)
    };
    if profile_api_key.is_none() {
        if let Some(auth) = authorization_header(headers) {
            request = request.header(reqwest::header::AUTHORIZATION, auth);
        }
    }
    let anthropic_version = headers
        .get("anthropic-version")
        .and_then(|value| value.to_str().ok())
        .unwrap_or("2023-06-01");
    Ok(request.header("anthropic-version", anthropic_version))
}

fn authorization_header(headers: &InboundHeaderMap) -> Option<String> {
    headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .map(ToString::to_string)
}

fn inbound_api_key(headers: &InboundHeaderMap) -> Option<String> {
    headers
        .get("x-api-key")
        .or_else(|| headers.get("x-goog-api-key"))
        .and_then(|value| value.to_str().ok())
        .filter(|value| !value.trim().is_empty())
        .map(ToString::to_string)
        .or_else(|| {
            authorization_header(headers).and_then(|auth| {
                auth.strip_prefix("Bearer ")
                    .or_else(|| auth.strip_prefix("bearer "))
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(ToString::to_string)
            })
        })
}

pub(super) async fn upstream_error_response(upstream: reqwest::Response) -> Response {
    let status =
        StatusCode::from_u16(upstream.status().as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);
    let content_type = upstream
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .map(ToString::to_string)
        .unwrap_or_else(|| "application/json".to_string());
    let body = match upstream.bytes().await {
        Ok(bytes) => Body::from(bytes),
        Err(e) => Body::from(
            json!({ "error": { "message": format!("failed to read upstream error body: {e}") } })
                .to_string(),
        ),
    };
    Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, content_type)
        .body(body)
        .unwrap_or_else(|_| json_error(StatusCode::BAD_GATEWAY, "upstream request failed"))
}

pub(super) fn redacted_url(url: &str) -> String {
    match reqwest::Url::parse(url) {
        Ok(mut parsed) => {
            parsed.set_query(None);
            parsed.to_string()
        }
        Err(_) => url.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use axum::http::{header, HeaderMap, HeaderValue};

    use super::{apply_upstream_auth, join_protocol_endpoint, BridgeProtocol};

    #[test]
    fn joins_default_v1_for_host_root_endpoints() {
        assert_eq!(
            join_protocol_endpoint("https://api.example.com", "chat/completions", true),
            "https://api.example.com/v1/chat/completions"
        );
    }

    #[test]
    fn joins_provider_specific_api_roots_without_v1_append() {
        assert_eq!(
            join_protocol_endpoint(
                "https://generativelanguage.googleapis.com/v1beta/openai",
                "chat/completions",
                false,
            ),
            "https://generativelanguage.googleapis.com/v1beta/openai/chat/completions"
        );
        assert_eq!(
            join_protocol_endpoint(
                "https://ark.cn-beijing.volces.com/api/v3",
                "responses",
                false,
            ),
            "https://ark.cn-beijing.volces.com/api/v3/responses"
        );
    }

    #[test]
    fn profile_key_overrides_openai_inbound_auth() {
        let mut headers = HeaderMap::new();
        headers.insert(
            header::AUTHORIZATION,
            HeaderValue::from_static("Bearer dummy-key"),
        );

        let request = apply_upstream_auth(
            reqwest::Client::new().post("http://127.0.0.1/v1/chat/completions"),
            BridgeProtocol::OpenAiChat,
            false,
            &headers,
            Some("sk-profile"),
        )
        .unwrap()
        .build()
        .unwrap();

        assert_eq!(
            request
                .headers()
                .get(reqwest::header::AUTHORIZATION)
                .and_then(|value| value.to_str().ok()),
            Some("Bearer sk-profile")
        );
    }

    #[test]
    fn profile_key_overrides_anthropic_key_without_forwarding_dummy_auth() {
        let mut headers = HeaderMap::new();
        headers.insert("x-api-key", HeaderValue::from_static("dummy-key"));
        headers.insert(
            header::AUTHORIZATION,
            HeaderValue::from_static("Bearer dummy-key"),
        );

        let request = apply_upstream_auth(
            reqwest::Client::new().post("http://127.0.0.1/v1/messages"),
            BridgeProtocol::AnthropicMessages,
            false,
            &headers,
            Some("sk-profile"),
        )
        .unwrap()
        .build()
        .unwrap();

        assert_eq!(
            request
                .headers()
                .get("x-api-key")
                .and_then(|value| value.to_str().ok()),
            Some("sk-profile")
        );
        assert!(request
            .headers()
            .get(reqwest::header::AUTHORIZATION)
            .is_none());
    }

    #[test]
    fn anthropic_auth_header_endpoint_uses_bearer_auth() {
        let mut headers = HeaderMap::new();
        headers.insert("x-api-key", HeaderValue::from_static("dummy-key"));

        let request = apply_upstream_auth(
            reqwest::Client::new().post("http://127.0.0.1/v1/messages"),
            BridgeProtocol::AnthropicMessages,
            true,
            &headers,
            Some("sk-profile"),
        )
        .unwrap()
        .build()
        .unwrap();

        assert_eq!(
            request
                .headers()
                .get(reqwest::header::AUTHORIZATION)
                .and_then(|value| value.to_str().ok()),
            Some("Bearer sk-profile")
        );
        assert!(request.headers().get("x-api-key").is_none());
        assert_eq!(
            request
                .headers()
                .get("anthropic-version")
                .and_then(|value| value.to_str().ok()),
            Some("2023-06-01")
        );
    }
}
