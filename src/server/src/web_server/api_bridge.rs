use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use common::config;
use common::profiles::headers::merged_upstream_headers;
use common::profiles::schema::ProfileDef;
use common::profiles::{catalog, connections};
use serde_json::{json, Value};
use va_ai_api_bridge::{
    DeepSeekBridgeSettings, ProviderBridgeAdapter, ProviderBridgeAdapterConfig,
};

mod completion;
mod content_policy;
mod model_mapping;
mod normalization;
mod passthrough;
mod protocol;
mod routes;
mod stream;
mod upstream;

use completion::translated_completion_response;
use content_policy::validate_request_content;
use model_mapping::{bridge_model_mapping, bridge_route_preference};
use normalization::normalize_target_request;
use passthrough::{buffered_passthrough_response, passthrough_response};
pub(super) use protocol::BridgeProtocol;
pub use routes::{
    legacy_chat_completions_handler, legacy_gemini_generate_content_handler,
    legacy_messages_handler, legacy_models_handler, legacy_responses_handler,
    local_chat_completions_handler, local_gemini_generate_content_handler, local_messages_handler,
    local_models_handler, local_responses_handler,
};
use stream::translated_stream_response;
use upstream::{apply_upstream_auth, redacted_url, upstream_endpoint, upstream_error_response};

use super::AppState;

pub(super) async fn bridge_handler(
    state: AppState,
    profile_id: String,
    route_scope: Option<String>,
    manual_scope: Option<String>,
    target_api_type: String,
    client_protocol: BridgeProtocol,
    headers: HeaderMap,
    original_request: Value,
) -> Response {
    let upstream = match upstream_endpoint(&profile_id, &target_api_type) {
        Ok(endpoint) => endpoint,
        Err((status, message)) => return json_error(status, &message),
    };
    let bridge_preference = bridge_route_preference(
        &upstream.profile,
        route_scope.as_deref(),
        client_protocol.api_type(),
        &target_api_type,
    );
    if let Some(scope) = manual_scope.as_deref() {
        if let Err(response) = validate_manual_scope(scope) {
            return response;
        }
    }
    let mut provider_adapter =
        provider_adapter_for_profile(&upstream.profile, upstream.protocol, &target_api_type);
    let manual_profile_api_key = manual_scope
        .as_ref()
        .and_then(|_| upstream.profile.credentials.get("api_key").cloned());

    let mut agent_request = original_request;

    if client_protocol == upstream.protocol {
        let requested_agent_model = wire_model(&agent_request);
        let model_mapping = bridge_model_mapping(
            &upstream.profile,
            bridge_preference.as_ref(),
            &target_api_type,
            requested_agent_model.as_deref(),
        );
        let original_agent_request = agent_request.clone();
        if let Some(mapping) = &model_mapping {
            apply_wire_model(&mut agent_request, &mapping.upstream_model);
        }
        if let Err(message) = normalize_target_request(&mut agent_request, upstream.protocol) {
            return json_error(StatusCode::UNPROCESSABLE_ENTITY, &message);
        }
        if upstream.protocol == BridgeProtocol::OpenAiResponses {
            provider_adapter.prepare_responses_request(&mut agent_request);
        } else if upstream.protocol == BridgeProtocol::OpenAiChat {
            provider_adapter.prepare_chat_request(
                client_protocol.provider_request_source(),
                &original_agent_request,
                &mut agent_request,
            );
        } else if upstream.protocol == BridgeProtocol::AnthropicMessages {
            provider_adapter.prepare_anthropic_request(&mut agent_request);
        }
        if let Ok(mut validation_request) = upstream
            .protocol
            .decode_agent_request(agent_request.clone())
        {
            if let Some(mapping) = &model_mapping {
                validation_request.model = Some(mapping.upstream_model.clone());
            }
            if let Err(message) =
                validate_request_content(&upstream.profile, &target_api_type, &validation_request)
            {
                return json_error(StatusCode::UNPROCESSABLE_ENTITY, &message);
            }
        }
        let stream = agent_request
            .get("stream")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let body = match serde_json::to_vec(&agent_request) {
            Ok(body) => body,
            Err(e) => {
                return json_error(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    &format!("failed to serialize bridge request: {e}"),
                );
            }
        };
        let upstream_headers = match merged_upstream_headers(
            &upstream.headers,
            bridge_preference
                .as_ref()
                .map(|preference| &preference.headers),
        ) {
            Ok(headers) => headers,
            Err(error) => return json_error(StatusCode::BAD_REQUEST, &error.to_string()),
        };
        let upstream_client = match upstream_http_client(&state, &upstream.profile) {
            Ok(client) => client,
            Err(message) => return json_error(StatusCode::BAD_REQUEST, &message),
        };
        let request = upstream_client
            .post(&upstream.url)
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .headers(upstream_headers)
            .body(body);
        let request = match apply_upstream_auth(
            request,
            upstream.protocol,
            upstream.auth_header,
            &headers,
            manual_profile_api_key.as_deref(),
        ) {
            Ok(request) => request,
            Err(response) => return response,
        };

        tracing::info!(
            target: "server::web_server::api_bridge",
            profile_id = %profile_id,
            route_scope = ?route_scope,
            manual_scope = ?manual_scope,
            target_api_type = %target_api_type,
            upstream = %redacted_url(&upstream.url),
            stream = stream,
            "API bridge passthrough forwarding request"
        );

        let response = match request.send().await {
            Ok(response) => response,
            Err(e) => {
                return json_error(
                    StatusCode::BAD_GATEWAY,
                    &format!("failed to reach upstream bridge endpoint: {e}"),
                );
            }
        };
        if !stream {
            return buffered_passthrough_response(response).await;
        }
        return passthrough_response(response);
    }

    let mut universal_request = match client_protocol.decode_agent_request(agent_request.clone()) {
        Ok(request) => request,
        Err(error) => return json_error(StatusCode::BAD_REQUEST, &error.to_string()),
    };
    let model_mapping = bridge_model_mapping(
        &upstream.profile,
        bridge_preference.as_ref(),
        &target_api_type,
        universal_request.model.as_deref(),
    );
    if let Some(mapping) = &model_mapping {
        universal_request.model = Some(mapping.upstream_model.clone());
    }
    let mut upstream_request = match upstream
        .protocol
        .encode_upstream_request(&universal_request)
    {
        Ok(request) => request,
        Err(error) => return json_error(StatusCode::UNPROCESSABLE_ENTITY, &error.to_string()),
    };
    if let Err(message) = normalize_target_request(&mut upstream_request, upstream.protocol) {
        return json_error(StatusCode::UNPROCESSABLE_ENTITY, &message);
    }
    if upstream.protocol == BridgeProtocol::OpenAiResponses {
        provider_adapter.prepare_responses_request(&mut upstream_request);
    } else if upstream.protocol == BridgeProtocol::OpenAiChat {
        provider_adapter.prepare_chat_request(
            client_protocol.provider_request_source(),
            &agent_request,
            &mut upstream_request,
        );
    } else if upstream.protocol == BridgeProtocol::AnthropicMessages {
        provider_adapter.prepare_anthropic_request(&mut upstream_request);
    }
    if let Ok(validation_request) = upstream
        .protocol
        .decode_agent_request(upstream_request.clone())
    {
        if let Err(message) =
            validate_request_content(&upstream.profile, &target_api_type, &validation_request)
        {
            return json_error(StatusCode::UNPROCESSABLE_ENTITY, &message);
        }
    }
    let stream = upstream_request
        .get("stream")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let body = match serde_json::to_vec(&upstream_request) {
        Ok(body) => body,
        Err(e) => {
            return json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                &format!("failed to serialize bridge request: {e}"),
            );
        }
    };

    let upstream_headers = match merged_upstream_headers(
        &upstream.headers,
        bridge_preference
            .as_ref()
            .map(|preference| &preference.headers),
    ) {
        Ok(headers) => headers,
        Err(error) => return json_error(StatusCode::BAD_REQUEST, &error.to_string()),
    };

    let upstream_client = match upstream_http_client(&state, &upstream.profile) {
        Ok(client) => client,
        Err(message) => return json_error(StatusCode::BAD_REQUEST, &message),
    };
    let request = upstream_client
        .post(&upstream.url)
        .header(reqwest::header::CONTENT_TYPE, "application/json")
        .headers(upstream_headers)
        .body(body);
    let request = match apply_upstream_auth(
        request,
        upstream.protocol,
        upstream.auth_header,
        &headers,
        manual_profile_api_key.as_deref(),
    ) {
        Ok(request) => request,
        Err(response) => return response,
    };

    tracing::info!(
        target: "server::web_server::api_bridge",
        profile_id = %profile_id,
        route_scope = ?route_scope,
        manual_scope = ?manual_scope,
        target_api_type = %target_api_type,
        upstream = %redacted_url(&upstream.url),
        bridge_model = ?model_mapping.as_ref().map(|mapping| mapping.upstream_model.as_str()),
        agent_model = ?model_mapping.as_ref().map(|mapping| mapping.agent_model.as_str()),
        stream = stream,
        "API bridge forwarding request"
    );

    let response = match request.send().await {
        Ok(response) => response,
        Err(e) => {
            return json_error(
                StatusCode::BAD_GATEWAY,
                &format!("failed to reach upstream bridge endpoint: {e}"),
            );
        }
    };

    if !response.status().is_success() {
        return upstream_error_response(response).await;
    }

    if stream {
        translated_stream_response(
            response,
            upstream.protocol,
            client_protocol,
            provider_adapter,
            model_mapping.map(|mapping| mapping.agent_model),
        )
    } else {
        translated_completion_response(
            response,
            upstream.protocol,
            client_protocol,
            &mut provider_adapter,
            model_mapping.map(|mapping| mapping.agent_model),
        )
        .await
    }
}

pub(super) async fn models_handler(
    profile_id: String,
    route_scope: Option<String>,
    manual_scope: Option<String>,
    target_api_type: String,
) -> Response {
    let upstream = match upstream_endpoint(&profile_id, &target_api_type) {
        Ok(endpoint) => endpoint,
        Err((status, message)) => return json_error(status, &message),
    };
    if let Some(scope) = manual_scope.as_deref() {
        if let Err(response) = validate_manual_scope(scope) {
            return response;
        }
    }
    let client_api_type = route_scope.as_deref().and_then(client_api_type_from_scope);
    let bridge_preference = client_api_type.and_then(|client_api_type| {
        bridge_route_preference(
            &upstream.profile,
            route_scope.as_deref(),
            client_api_type,
            &target_api_type,
        )
    });
    let models = connections::bridge_model_routes(
        &upstream.profile,
        bridge_preference.as_ref(),
        &target_api_type,
    );
    let data: Vec<_> = models
        .iter()
        .map(|model| {
            let metadata =
                bridge_model_metadata(&upstream.profile, &target_api_type, &model.upstream_model);
            let input_modalities = if metadata.image_input {
                vec!["text", "image"]
            } else {
                vec!["text"]
            };
            json!({
                "id": model.agent_model.as_str(),
                "type": "model",
                "object": "model",
                "display_name": model.agent_model.as_str(),
                "owned_by": upstream.profile.provider.as_str(),
                "created": 0,
                "created_at": null,
                "context_window": metadata.context_window,
                "max_context_window": metadata.context_window,
                "input_modalities": input_modalities,
                "capabilities": {
                    "image_input": metadata.image_input,
                    "file_input": metadata.file_input,
                }
            })
        })
        .collect();
    let first_id = data
        .first()
        .and_then(|model| model.get("id"))
        .and_then(Value::as_str)
        .map(ToOwned::to_owned);
    let last_id = data
        .last()
        .and_then(|model| model.get("id"))
        .and_then(Value::as_str)
        .map(ToOwned::to_owned);
    Json(json!({
        "object": "list",
        "data": data,
        "has_more": false,
        "first_id": first_id,
        "last_id": last_id,
    }))
    .into_response()
}

fn upstream_http_client(state: &AppState, profile: &ProfileDef) -> Result<reqwest::Client, String> {
    if !profile.use_settings_proxy {
        return Ok(state.preview_client.clone());
    }
    let cfg = config::ensure_loaded();
    if !cfg.proxy.enabled {
        return Ok(state.preview_client.clone());
    }
    proxy_http_client(&cfg.proxy)
}

fn proxy_http_client(proxy: &config::HttpProxyConfig) -> Result<reqwest::Client, String> {
    let proxy_url = proxy.http_proxy.as_deref().ok_or_else(|| {
        "Settings proxy is enabled for this profile but HTTP proxy URL is empty".to_string()
    })?;
    let mut proxy_rule = reqwest::Proxy::all(proxy_url)
        .map_err(|error| format!("invalid Settings proxy HTTP URL: {error}"))?;
    if let Some(no_proxy) = proxy.no_proxy.as_deref() {
        proxy_rule = proxy_rule.no_proxy(reqwest::NoProxy::from_string(no_proxy));
    }
    reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .proxy(proxy_rule)
        .build()
        .map_err(|error| format!("failed to build API bridge proxy client: {error}"))
}

struct BridgeModelMetadata {
    context_window: Option<u64>,
    image_input: bool,
    file_input: bool,
}

fn bridge_model_metadata(
    profile: &common::profiles::schema::ProfileDef,
    target_api_type: &str,
    upstream_model: &str,
) -> BridgeModelMetadata {
    let endpoint = catalog::get(&profile.provider).and_then(|provider| {
        let endpoint_id = profile
            .overrides
            .get(target_api_type)
            .and_then(|overrides| overrides.endpoint_id.as_deref());
        catalog::find_endpoint(provider, target_api_type, endpoint_id)
    });
    let Some(endpoint) = endpoint else {
        return BridgeModelMetadata {
            context_window: None,
            image_input: false,
            file_input: false,
        };
    };
    let model_def = catalog::find_model(endpoint, upstream_model);
    let capabilities = model_def
        .map(|model_def| endpoint.capabilities.content.merge(&model_def.capabilities))
        .unwrap_or_else(|| endpoint.capabilities.content.clone());
    BridgeModelMetadata {
        context_window: model_def.and_then(|model_def| model_def.context_window),
        image_input: capabilities.image_input,
        file_input: capabilities.file_input,
    }
}

fn provider_adapter_for_profile(
    profile: &ProfileDef,
    target_protocol: BridgeProtocol,
    target_api_type: &str,
) -> ProviderBridgeAdapter {
    let provider_id = if is_moonshot_kimi_coding(profile, target_api_type) {
        "kimi"
    } else {
        profile.provider.as_str()
    };
    ProviderBridgeAdapter::for_provider(
        provider_id,
        target_protocol.wire_protocol(),
        ProviderBridgeAdapterConfig {
            deepseek: DeepSeekBridgeSettings {
                thinking: profile.provider_settings.deepseek.thinking,
                replay_reasoning_content: profile
                    .provider_settings
                    .deepseek
                    .replay_reasoning_content,
            },
            thinking_enabled: profile_reasoning_enabled(profile, "openai-chat"),
        },
    )
}

fn is_moonshot_kimi_coding(profile: &ProfileDef, target_api_type: &str) -> bool {
    target_api_type == "anthropic"
        && profile
            .overrides
            .get("anthropic")
            .and_then(|overrides| overrides.endpoint_id.as_deref())
            == Some("kimi-coding")
}

fn profile_reasoning_enabled(profile: &ProfileDef, api_type: &str) -> Option<bool> {
    profile
        .overrides
        .get(api_type)
        .and_then(|overrides| overrides.reasoning_effort.as_deref())
        .map(reasoning_effort_enabled)
}

fn reasoning_effort_enabled(value: &str) -> bool {
    !matches!(
        value.trim().to_ascii_lowercase().as_str(),
        "off" | "none" | "disabled" | "disable" | "false"
    )
}

fn client_api_type_from_scope(scope: &str) -> Option<&str> {
    ["claude", "codex", "gemini", "opencode", "pi"]
        .iter()
        .find_map(|agent_id| scope.strip_prefix(&format!("{agent_id}-")))
}

fn validate_manual_scope(scope: &str) -> Result<(), Response> {
    if scope.is_empty() {
        return Err(json_error(
            StatusCode::BAD_REQUEST,
            "manual bridge scope must not be empty",
        ));
    }
    if scope.len() > 128 || !scope.chars().all(is_manual_scope_char) {
        return Err(json_error(
            StatusCode::BAD_REQUEST,
            "manual bridge scope must be 1-128 characters and contain only ASCII letters, digits, '.', '_' or '-'",
        ));
    }
    Ok(())
}

fn apply_wire_model(request: &mut Value, model: &str) {
    if let Some(object) = request.as_object_mut() {
        object.insert("model".to_string(), Value::String(model.to_string()));
    }
}

fn wire_model(request: &Value) -> Option<String> {
    request
        .get("model")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

fn is_manual_scope_char(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '-')
}

fn json_error(status: StatusCode, message: &str) -> Response {
    (
        status,
        Json(json!({
            "error": {
                "message": message,
                "type": "vibearound_bridge_error",
            }
        })),
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use common::config::HttpProxyConfig;

    use super::{proxy_http_client, validate_manual_scope};

    #[test]
    fn accepts_manual_bridge_scope() {
        validate_manual_scope("codex.project_1").unwrap();
    }

    #[test]
    fn rejects_invalid_manual_bridge_scope() {
        assert!(validate_manual_scope("").is_err());
        assert!(validate_manual_scope("codex/project").is_err());
        assert!(validate_manual_scope("codex project").is_err());
        assert!(validate_manual_scope(&"a".repeat(129)).is_err());
    }

    #[test]
    fn proxy_client_requires_configured_proxy_url() {
        let error = proxy_http_client(&HttpProxyConfig::default()).unwrap_err();

        assert!(error.contains("HTTP proxy URL is empty"));
    }

    #[test]
    fn proxy_client_accepts_settings_proxy_with_no_proxy() {
        proxy_http_client(&HttpProxyConfig {
            enabled: true,
            http_proxy: Some("http://127.0.0.1:7890".to_string()),
            no_proxy: Some("localhost,127.0.0.1".to_string()),
        })
        .expect("proxy client builds");
    }
}
