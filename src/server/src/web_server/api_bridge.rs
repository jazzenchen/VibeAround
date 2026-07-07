use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use common::config;
use common::profiles::catalog::ContentCapabilities;
use common::profiles::headers::merged_upstream_headers;
use common::profiles::schema::ProfileDef;
use common::profiles::{catalog, connections};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use va_ai_api_bridge::{
    DeepSeekBridgeSettings, ProviderBridgeAdapter, ProviderBridgeAdapterConfig, UniversalRequest,
    UniversalResponse,
};

mod completion;
mod content_policy;
mod google_code_assist;
mod local_agent;
mod model_mapping;
mod normalization;
mod passthrough;
mod protocol;
mod routes;
mod server_tools;
mod stream;
mod upstream;

use completion::{
    decode_completion_response, translated_completion_events_response,
    translated_completion_response, UpstreamResponseTransform,
};
use content_policy::{sanitize_request_content_with_capabilities, ContentSanitization};
pub use local_agent::{
    local_agent_chat_completions_handler, local_agent_messages_handler, local_agent_models_handler,
    local_agent_responses_handler,
};
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
use stream::{translated_events_stream_response, translated_stream_response};
use upstream::{
    apply_upstream_auth, redacted_url, request_stream, send_upstream_request_with_rate_limit_retry,
    upstream_endpoint, upstream_error_response, upstream_error_response_with_hint,
    RateLimitRetryContext, ResolvedUpstreamRoute,
};

use super::bridge_recording::{ActiveBridgeRecord, BridgeRecordMetadata};
use super::AppState;

pub(super) async fn bridge_handler(
    state: AppState,
    profile_id: String,
    route_scope: Option<String>,
    manual_scope: Option<String>,
    target_api_type: String,
    client_protocol: BridgeProtocol,
    headers: HeaderMap,
    request_id: String,
    record: Option<ActiveBridgeRecord>,
    original_request: Value,
) -> Response {
    let upstream = match upstream_endpoint(&profile_id, &target_api_type) {
        Ok(endpoint) => endpoint,
        Err((status, message)) => return record_json_error(record.as_ref(), status, &message),
    };
    let bridge_preference = bridge_route_preference(
        &upstream.profile,
        route_scope.as_deref(),
        client_protocol.api_type(),
        &target_api_type,
    );
    if let Some(scope) = manual_scope.as_deref() {
        if let Err((status, message)) = validate_manual_scope(scope) {
            return record_json_error(record.as_ref(), status, &message);
        }
    }
    let mut provider_adapter =
        provider_adapter_for_profile(&upstream.profile, upstream.protocol, &target_api_type);
    let manual_profile_api_key = manual_scope
        .as_ref()
        .and_then(|_| upstream.profile.credentials.get("api_key").cloned());

    let mut agent_request = original_request;

    let requested_agent_model = wire_model(&agent_request);
    let model_mapping = bridge_model_mapping(
        &upstream.profile,
        bridge_preference.as_ref(),
        &target_api_type,
        requested_agent_model.as_deref(),
    );
    let upstream_model_for_capabilities = model_mapping
        .as_ref()
        .map(|mapping| mapping.upstream_model.as_str())
        .or(requested_agent_model.as_deref());
    let target_model_capabilities = resolved_bridge_model_capabilities(
        &upstream.profile,
        &target_api_type,
        upstream_model_for_capabilities,
        model_mapping.as_ref().map(|mapping| &mapping.capabilities),
    );
    let requested_web_search_same_protocol = client_protocol == upstream.protocol
        && !upstream.is_google_code_assist()
        && client_protocol
            .decode_agent_request(agent_request.clone())
            .map(|request| server_tools::request_needs_web_search_fallback(&request))
            .unwrap_or(false);
    let same_protocol_needs_web_search_fallback = requested_web_search_same_protocol
        && state.host_search_available
        && (state.replace_provider_web_search || !target_model_capabilities.web_search);
    let same_protocol_needs_web_search_discard = requested_web_search_same_protocol
        && !state.host_search_available
        && !target_model_capabilities.web_search;

    if same_protocol_needs_web_search_discard {
        tracing::info!(
            target: "server::web_server::api_bridge",
            request_id = %request_id,
            profile_id = %profile_id,
            target_api_type = %target_api_type,
            "API bridge will translate request to discard unsupported provider web search"
        );
    }

    if client_protocol == upstream.protocol
        && !upstream.is_google_code_assist()
        && !same_protocol_needs_web_search_fallback
        && !same_protocol_needs_web_search_discard
    {
        let original_agent_request = agent_request.clone();
        if let Some(mapping) = &model_mapping {
            apply_wire_model(&mut agent_request, &mapping.upstream_model);
        }
        if let Ok(mut content_request) = upstream
            .protocol
            .decode_agent_request(agent_request.clone())
        {
            let sanitization = sanitize_request_content_with_capabilities(
                &upstream.profile,
                &target_api_type,
                &mut content_request,
                model_mapping.as_ref().map(|mapping| &mapping.capabilities),
            );
            log_content_sanitization(
                &profile_id,
                &target_api_type,
                client_protocol,
                upstream.protocol,
                sanitization,
            );
            if sanitization.changed() {
                agent_request = match upstream.protocol.encode_upstream_request(&content_request) {
                    Ok(request) => request,
                    Err(error) => {
                        return record_json_error(
                            record.as_ref(),
                            StatusCode::UNPROCESSABLE_ENTITY,
                            &error.to_string(),
                        );
                    }
                };
            }
        }
        let gemini_route = if upstream.protocol == BridgeProtocol::GeminiGenerateContent {
            match resolve_upstream_route(&upstream, &agent_request) {
                Ok(route) => Some(route),
                Err((status, message)) => {
                    return record_json_error(record.as_ref(), status, &message);
                }
            }
        } else {
            None
        };
        if let Err(message) = normalize_target_request(&mut agent_request, upstream.protocol) {
            return record_json_error(record.as_ref(), StatusCode::UNPROCESSABLE_ENTITY, &message);
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
            provider_adapter.prepare_anthropic_request(
                client_protocol.provider_request_source(),
                &mut agent_request,
            );
        }
        let route = match gemini_route {
            Some(route) => route,
            None => match resolve_upstream_route(&upstream, &agent_request) {
                Ok(route) => route,
                Err((status, message)) => {
                    return record_json_error(record.as_ref(), status, &message);
                }
            },
        };
        let request = match build_upstream_request(
            &state,
            &upstream,
            &route,
            agent_request,
            bridge_preference.as_ref(),
            &headers,
            manual_profile_api_key.as_deref(),
            record.as_ref(),
            bridge_record_metadata(
                &profile_id,
                route_scope.as_ref(),
                manual_scope.as_ref(),
                &target_api_type,
                client_protocol,
                Some(upstream.protocol),
                Some(&route),
                true,
            ),
        )
        .await
        {
            Ok(request) => request,
            Err(response) => return response,
        };

        tracing::info!(
            target: "server::web_server::api_bridge",
            request_id = %request_id,
            profile_id = %profile_id,
            route_scope = ?route_scope,
            manual_scope = ?manual_scope,
            target_api_type = %target_api_type,
            upstream = %redacted_url(&route.url),
            stream = route.stream,
            "API bridge passthrough forwarding request"
        );

        let retry_context = retry_context(
            &request_id,
            &profile_id,
            route_scope.as_ref(),
            &target_api_type,
            client_protocol,
            &route,
        );
        let response = match send_upstream_request_with_rate_limit_retry(
            request,
            Some(&retry_context),
        )
        .await
        {
            Ok(response) => response,
            Err(e) => {
                return record_json_error(
                    record.as_ref(),
                    StatusCode::BAD_GATEWAY,
                    &format!("failed to reach upstream bridge endpoint: {e}"),
                );
            }
        };
        if !route.stream {
            return buffered_passthrough_response(response, record.as_ref()).await;
        }
        return passthrough_response(response, record.clone());
    }

    let mut universal_request = match client_protocol.decode_agent_request(agent_request.clone()) {
        Ok(request) => request,
        Err(error) => {
            return record_json_error(record.as_ref(), StatusCode::BAD_REQUEST, &error.to_string());
        }
    };
    if let Some(mapping) = &model_mapping {
        universal_request.model = Some(mapping.upstream_model.clone());
    }
    let sanitization = sanitize_request_content_with_capabilities(
        &upstream.profile,
        &target_api_type,
        &mut universal_request,
        model_mapping.as_ref().map(|mapping| &mapping.capabilities),
    );
    log_content_sanitization(
        &profile_id,
        &target_api_type,
        client_protocol,
        upstream.protocol,
        sanitization,
    );
    let request_needs_web_search =
        server_tools::request_needs_web_search_fallback(&universal_request);
    let can_use_native_web_search = target_model_capabilities.web_search
        && protocol_encodes_native_web_search(upstream.protocol);
    let should_use_host_web_search = request_needs_web_search
        && state.host_search_available
        && (state.replace_provider_web_search || !can_use_native_web_search);
    let web_search_fallback = if should_use_host_web_search {
        server_tools::prepare_web_search_fallback(&mut universal_request)
    } else {
        if request_needs_web_search
            && !can_use_native_web_search
            && server_tools::discard_web_search_server_tools(&mut universal_request)
        {
            tracing::info!(
                target: "server::web_server::api_bridge",
                request_id = %request_id,
                profile_id = %profile_id,
                target_api_type = %target_api_type,
                host_search_available = state.host_search_available,
                native_web_search = target_model_capabilities.web_search,
                "API bridge discarded provider web search server tool"
            );
        }
        if request_needs_web_search && can_use_native_web_search {
            tracing::info!(
                target: "server::web_server::api_bridge",
                request_id = %request_id,
                profile_id = %profile_id,
                target_api_type = %target_api_type,
                upstream_protocol = ?upstream.protocol,
                "API bridge preserved native provider web search server tool"
            );
        }
        None
    };
    if let Some(fallback) = &web_search_fallback {
        tracing::info!(
            target: "server::web_server::api_bridge",
            request_id = %request_id,
            profile_id = %profile_id,
            target_api_type = %target_api_type,
            client_protocol = ?client_protocol,
            upstream_protocol = ?upstream.protocol,
            original_stream = fallback.original_stream,
            "API bridge injected host web search fallback tool"
        );
    }
    let mut upstream_request = match upstream
        .protocol
        .encode_upstream_request(&universal_request)
    {
        Ok(request) => request,
        Err(error) => {
            return record_json_error(
                record.as_ref(),
                StatusCode::UNPROCESSABLE_ENTITY,
                &error.to_string(),
            );
        }
    };
    let gemini_route = if upstream.protocol == BridgeProtocol::GeminiGenerateContent {
        match resolve_upstream_route(&upstream, &upstream_request) {
            Ok(route) => Some(route),
            Err((status, message)) => {
                return record_json_error(record.as_ref(), status, &message);
            }
        }
    } else {
        None
    };
    if let Err(message) = normalize_target_request(&mut upstream_request, upstream.protocol) {
        return record_json_error(record.as_ref(), StatusCode::UNPROCESSABLE_ENTITY, &message);
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
        provider_adapter.prepare_anthropic_request(
            client_protocol.provider_request_source(),
            &mut upstream_request,
        );
    }
    let route = match gemini_route {
        Some(route) => route,
        None => match resolve_upstream_route(&upstream, &upstream_request) {
            Ok(route) => route,
            Err((status, message)) => {
                return record_json_error(record.as_ref(), status, &message);
            }
        },
    };
    let request = match build_upstream_request(
        &state,
        &upstream,
        &route,
        upstream_request,
        bridge_preference.as_ref(),
        &headers,
        manual_profile_api_key.as_deref(),
        record.as_ref(),
        bridge_record_metadata(
            &profile_id,
            route_scope.as_ref(),
            manual_scope.as_ref(),
            &target_api_type,
            client_protocol,
            Some(upstream.protocol),
            Some(&route),
            false,
        ),
    )
    .await
    {
        Ok(request) => request,
        Err(response) => return response,
    };

    tracing::info!(
        target: "server::web_server::api_bridge",
        request_id = %request_id,
        profile_id = %profile_id,
        route_scope = ?route_scope,
        manual_scope = ?manual_scope,
        target_api_type = %target_api_type,
        upstream = %redacted_url(&route.url),
        bridge_model = ?model_mapping.as_ref().map(|mapping| mapping.upstream_model.as_str()),
        agent_model = ?model_mapping.as_ref().map(|mapping| mapping.agent_model.as_str()),
        stream = route.stream,
        "API bridge forwarding request"
    );

    let retry_context = retry_context(
        &request_id,
        &profile_id,
        route_scope.as_ref(),
        &target_api_type,
        client_protocol,
        &route,
    );
    let response =
        match send_upstream_request_with_rate_limit_retry(request, Some(&retry_context)).await {
            Ok(response) => response,
            Err(e) => {
                return record_json_error(
                    record.as_ref(),
                    StatusCode::BAD_GATEWAY,
                    &format!("failed to reach upstream bridge endpoint: {e}"),
                );
            }
        };

    if !response.status().is_success() {
        return upstream_error_response(response, record.as_ref()).await;
    }

    let response_transform = if upstream.is_google_code_assist() {
        UpstreamResponseTransform::GoogleCodeAssist
    } else {
        UpstreamResponseTransform::Identity
    };

    if let Some(fallback) = web_search_fallback {
        return translated_web_search_fallback_response(
            &state,
            &upstream,
            bridge_preference.as_ref(),
            &headers,
            manual_profile_api_key.as_deref(),
            record.as_ref(),
            &request_id,
            &profile_id,
            route_scope.as_ref(),
            manual_scope.as_ref(),
            &target_api_type,
            client_protocol,
            &mut provider_adapter,
            universal_request,
            fallback,
            response,
            response_transform,
            model_mapping.map(|mapping| mapping.agent_model),
            &agent_request,
        )
        .await;
    }

    if route.stream {
        translated_stream_response(
            response,
            upstream.protocol,
            client_protocol,
            provider_adapter,
            model_mapping.map(|mapping| mapping.agent_model),
            response_transform,
            record.clone(),
        )
    } else {
        translated_completion_response(
            response,
            upstream.protocol,
            client_protocol,
            &mut provider_adapter,
            model_mapping.map(|mapping| mapping.agent_model),
            response_transform,
            record.as_ref(),
        )
        .await
    }
}

#[allow(clippy::too_many_arguments)]
async fn translated_web_search_fallback_response(
    state: &AppState,
    upstream: &upstream::UpstreamEndpoint,
    bridge_preference: Option<&common::agent_state::ProfileBridgePreference>,
    headers: &HeaderMap,
    manual_profile_api_key: Option<&str>,
    record: Option<&ActiveBridgeRecord>,
    request_id: &str,
    profile_id: &str,
    route_scope: Option<&String>,
    manual_scope: Option<&String>,
    target_api_type: &str,
    client_protocol: BridgeProtocol,
    provider_adapter: &mut ProviderBridgeAdapter,
    mut universal_request: UniversalRequest,
    fallback: server_tools::WebSearchFallback,
    first_response: reqwest::Response,
    response_transform: UpstreamResponseTransform,
    agent_model: Option<String>,
    original_agent_request: &Value,
) -> Response {
    let provider = server_tools::HostWebSearchProvider::new(state.search_runtime.clone());
    let mut response = first_response;

    for round in 0..server_tools::MAX_WEB_SEARCH_FALLBACK_ROUNDS {
        let events = match decode_completion_response(
            response,
            upstream.protocol,
            provider_adapter,
            response_transform,
            record,
        )
        .await
        {
            Ok(events) => events,
            Err(response) => return response,
        };

        let upstream_response = UniversalResponse::from_events(&events);
        let should_continue = match server_tools::append_web_search_results(
            &mut universal_request,
            upstream_response,
            &fallback,
            &provider,
            record,
            round + 1,
        )
        .await
        {
            Ok(value) => value,
            Err(message) => {
                return record_json_error(record, StatusCode::BAD_GATEWAY, &message);
            }
        };

        if !should_continue {
            if fallback.original_stream {
                return translated_events_stream_response(
                    events,
                    client_protocol,
                    agent_model,
                    record,
                );
            }
            return translated_completion_events_response(
                events,
                client_protocol,
                agent_model,
                record,
            );
        }

        tracing::info!(
            target: "server::web_server::api_bridge",
            request_id = %request_id,
            profile_id = %profile_id,
            target_api_type = %target_api_type,
            round = round + 1,
            "API bridge consumed host web search fallback tool call"
        );

        if round + 1 >= server_tools::MAX_WEB_SEARCH_FALLBACK_ROUNDS {
            return record_json_error(
                record,
                StatusCode::BAD_GATEWAY,
                "web search fallback exceeded the maximum internal tool-call rounds",
            );
        }

        let (next_body, next_route) = match encode_fallback_upstream_request(
            upstream,
            client_protocol,
            original_agent_request,
            provider_adapter,
            &universal_request,
            record,
        ) {
            Ok(value) => value,
            Err(response) => return response,
        };
        let request = match build_upstream_request(
            state,
            upstream,
            &next_route,
            next_body,
            bridge_preference,
            headers,
            manual_profile_api_key,
            record,
            bridge_record_metadata(
                profile_id,
                route_scope,
                manual_scope,
                target_api_type,
                client_protocol,
                Some(upstream.protocol),
                Some(&next_route),
                false,
            ),
        )
        .await
        {
            Ok(request) => request,
            Err(response) => return response,
        };
        let retry_context = retry_context(
            request_id,
            profile_id,
            route_scope,
            target_api_type,
            client_protocol,
            &next_route,
        );
        response = match send_upstream_request_with_rate_limit_retry(request, Some(&retry_context))
            .await
        {
            Ok(response) => response,
            Err(e) => {
                return record_json_error(
                    record,
                    StatusCode::BAD_GATEWAY,
                    &format!("failed to reach upstream bridge endpoint: {e}"),
                );
            }
        };
        if !response.status().is_success() {
            return upstream_error_response_with_hint(
                response,
                record,
                Some(
                    "Web search fallback failed after sending search results to the upstream model",
                ),
            )
            .await;
        }
    }

    record_json_error(
        record,
        StatusCode::BAD_GATEWAY,
        "web search fallback stopped unexpectedly",
    )
}

fn encode_fallback_upstream_request(
    upstream: &upstream::UpstreamEndpoint,
    client_protocol: BridgeProtocol,
    original_agent_request: &Value,
    provider_adapter: &mut ProviderBridgeAdapter,
    universal_request: &UniversalRequest,
    record: Option<&ActiveBridgeRecord>,
) -> Result<(Value, ResolvedUpstreamRoute), Response> {
    let mut upstream_request = upstream
        .protocol
        .encode_upstream_request(universal_request)
        .map_err(|error| {
            record_json_error(record, StatusCode::UNPROCESSABLE_ENTITY, &error.to_string())
        })?;
    let gemini_route = if upstream.protocol == BridgeProtocol::GeminiGenerateContent {
        match resolve_upstream_route(upstream, &upstream_request) {
            Ok(route) => Some(route),
            Err((status, message)) => return Err(record_json_error(record, status, &message)),
        }
    } else {
        None
    };
    if let Err(message) = normalize_target_request(&mut upstream_request, upstream.protocol) {
        return Err(record_json_error(
            record,
            StatusCode::UNPROCESSABLE_ENTITY,
            &message,
        ));
    }
    if upstream.protocol == BridgeProtocol::OpenAiResponses {
        provider_adapter.prepare_responses_request(&mut upstream_request);
    } else if upstream.protocol == BridgeProtocol::OpenAiChat {
        provider_adapter.prepare_chat_request(
            client_protocol.provider_request_source(),
            original_agent_request,
            &mut upstream_request,
        );
    } else if upstream.protocol == BridgeProtocol::AnthropicMessages {
        provider_adapter.prepare_anthropic_request(
            client_protocol.provider_request_source(),
            &mut upstream_request,
        );
    }
    let route = match gemini_route {
        Some(route) => route,
        None => match resolve_upstream_route(upstream, &upstream_request) {
            Ok(route) => route,
            Err((status, message)) => return Err(record_json_error(record, status, &message)),
        },
    };
    Ok((upstream_request, route))
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
        if let Err((status, message)) = validate_manual_scope(scope) {
            return json_error(status, &message);
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
            let metadata = bridge_model_metadata(&upstream.profile, &target_api_type, model);
            let mut input_modalities = vec!["text"];
            if metadata.image_input {
                input_modalities.push("image");
            }
            if metadata.file_input {
                input_modalities.push("file");
            }
            let context_window = metadata.context_window;
            json!({
                "id": model.agent_model.as_str(),
                "model": model.agent_model.as_str(),
                "type": "model",
                "object": "model",
                "display_name": model.agent_model.as_str(),
                "displayName": model.agent_model.as_str(),
                "owned_by": upstream.profile.provider.as_str(),
                "created": 0,
                "created_at": null,
                "context_window": context_window,
                "contextWindow": context_window,
                "max_context_window": context_window,
                "maxContextWindow": context_window,
                "input_modalities": input_modalities.clone(),
                "inputModalities": input_modalities,
                "capabilities": {
                    "image_input": metadata.image_input,
                    "file_input": metadata.file_input,
                    "web_search": metadata.web_search,
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

fn retry_context(
    request_id: &str,
    profile_id: &str,
    route_scope: Option<&String>,
    target_api_type: &str,
    client_protocol: BridgeProtocol,
    route: &ResolvedUpstreamRoute,
) -> RateLimitRetryContext {
    RateLimitRetryContext {
        request_id: request_id.to_string(),
        profile_id: profile_id.to_string(),
        route_scope: route_scope.cloned(),
        target_api_type: target_api_type.to_string(),
        client_protocol,
        upstream: redacted_url(&route.url),
        stream: route.stream,
        model: route.model.clone(),
    }
}

fn bridge_record_metadata(
    profile_id: &str,
    route_scope: Option<&String>,
    manual_scope: Option<&String>,
    target_api_type: &str,
    client_protocol: BridgeProtocol,
    upstream_protocol: Option<BridgeProtocol>,
    route: Option<&ResolvedUpstreamRoute>,
    passthrough: bool,
) -> BridgeRecordMetadata {
    BridgeRecordMetadata {
        profile_id: profile_id.to_string(),
        route_scope: route_scope.cloned(),
        manual_scope: manual_scope.cloned(),
        target_api_type: target_api_type.to_string(),
        client_protocol: client_protocol.api_type().to_string(),
        upstream_protocol: upstream_protocol.map(|protocol| protocol.api_type().to_string()),
        upstream_url: route.map(|route| redacted_url(&route.url)),
        stream: route.map(|route| route.stream),
        model: route.and_then(|route| route.model.clone()),
        passthrough,
    }
}

fn protocol_encodes_native_web_search(protocol: BridgeProtocol) -> bool {
    matches!(protocol, BridgeProtocol::AnthropicMessages)
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

async fn build_upstream_request(
    state: &AppState,
    upstream: &upstream::UpstreamEndpoint,
    route: &ResolvedUpstreamRoute,
    request_body: Value,
    bridge_preference: Option<&common::agent_state::ProfileBridgePreference>,
    headers: &HeaderMap,
    manual_profile_api_key: Option<&str>,
    record: Option<&ActiveBridgeRecord>,
    record_metadata: BridgeRecordMetadata,
) -> Result<reqwest::RequestBuilder, Response> {
    let bridge_headers = bridge_preference
        .map(|preference| render_bridge_headers(&preference.headers, &upstream.profile));
    let bridge_managed_auth = bridge_headers
        .as_ref()
        .is_some_and(|headers| has_auth_header(headers));
    let upstream_headers = match merged_upstream_headers(&upstream.headers, bridge_headers.as_ref())
    {
        Ok(headers) => headers,
        Err(error) => {
            return Err(record_json_error(
                record,
                StatusCode::BAD_REQUEST,
                &error.to_string(),
            ));
        }
    };
    let upstream_client = match upstream_http_client(state, &upstream.profile) {
        Ok(client) => client,
        Err(message) => return Err(record_json_error(record, StatusCode::BAD_REQUEST, &message)),
    };

    let (request_body, bearer_token) = if upstream.is_google_code_assist() {
        let token = google_code_assist::bearer_token(&upstream_client).await?;
        let project = google_code_assist::resolve_project_id(
            &upstream_client,
            &upstream.base_url,
            &upstream.profile,
            &token,
        )
        .await?;
        let model = route.model.as_deref().ok_or_else(|| {
            record_json_error(
                record,
                StatusCode::UNPROCESSABLE_ENTITY,
                "Gemini Code Assist upstream request is missing model metadata",
            )
        })?;
        let request_body =
            google_code_assist::wrap_generate_content_request(request_body, model, project)
                .map_err(|message| {
                    record_json_error(record, StatusCode::UNPROCESSABLE_ENTITY, &message)
                })?;
        (request_body, Some(token))
    } else {
        (request_body, None)
    };
    if let Some(record) = record {
        record.bridge_request(record_metadata, &request_body);
    }
    let body = serde_json::to_vec(&request_body).map_err(|error| {
        record_json_error(
            record,
            StatusCode::INTERNAL_SERVER_ERROR,
            &format!("failed to serialize bridge request: {error}"),
        )
    })?;

    let request = upstream_client
        .post(&route.url)
        .header(reqwest::header::CONTENT_TYPE, "application/json")
        .headers(upstream_headers)
        .body(body);

    if let Some(token) = bearer_token {
        return Ok(request.bearer_auth(token));
    }

    apply_upstream_auth(
        request,
        upstream.protocol,
        upstream.auth_header,
        upstream.managed_auth || bridge_managed_auth,
        headers,
        manual_profile_api_key,
    )
}

fn render_bridge_headers(
    headers: &BTreeMap<String, String>,
    profile: &ProfileDef,
) -> BTreeMap<String, String> {
    headers
        .iter()
        .map(|(name, value)| {
            (
                name.clone(),
                render_profile_secret_placeholders(value, profile),
            )
        })
        .collect()
}

fn render_profile_secret_placeholders(value: &str, profile: &ProfileDef) -> String {
    let api_key = profile
        .credentials
        .get("api_key")
        .map(String::as_str)
        .unwrap_or_default();
    value.replace("$apiKey", api_key)
}

fn has_auth_header(headers: &BTreeMap<String, String>) -> bool {
    headers.keys().any(|name| {
        let name = name.trim();
        name.eq_ignore_ascii_case("authorization")
            || name.eq_ignore_ascii_case("x-api-key")
            || name.eq_ignore_ascii_case("x-goog-api-key")
    })
}

fn resolve_upstream_route(
    upstream: &upstream::UpstreamEndpoint,
    request: &Value,
) -> Result<ResolvedUpstreamRoute, (StatusCode, String)> {
    let stream = request_stream(upstream.protocol, request);
    let model = upstream
        .route_model(request)
        .map_err(|message| (StatusCode::UNPROCESSABLE_ENTITY, message))?;
    let url = upstream
        .request_url(request)
        .map_err(|message| (StatusCode::UNPROCESSABLE_ENTITY, message))?;
    Ok(ResolvedUpstreamRoute { stream, url, model })
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
    web_search: bool,
}

fn resolved_bridge_model_capabilities(
    profile: &common::profiles::schema::ProfileDef,
    target_api_type: &str,
    upstream_model: Option<&str>,
    capability_overrides: Option<&ContentCapabilities>,
) -> ContentCapabilities {
    let overrides = profile.overrides.get(target_api_type);
    let mut capabilities = if let Some(capabilities) =
        overrides.and_then(|overrides| overrides.capabilities.clone())
    {
        capabilities
    } else {
        let Some(provider) = catalog::get(&profile.provider) else {
            return capability_overrides.cloned().unwrap_or_default();
        };
        let endpoint_id = overrides.and_then(|overrides| overrides.endpoint_id.as_deref());
        let Some(endpoint) = catalog::find_endpoint(provider, target_api_type, endpoint_id) else {
            return capability_overrides.cloned().unwrap_or_default();
        };
        let mut capabilities = endpoint.capabilities.content.clone();
        let model = upstream_model
            .or_else(|| {
                overrides
                    .and_then(|overrides| overrides.model.as_deref())
                    .map(str::trim)
                    .filter(|model| !model.is_empty())
            })
            .or_else(|| endpoint.models.first().map(|model| model.id.as_str()));
        if let Some(model) = model.and_then(|model| catalog::find_model(endpoint, model)) {
            capabilities = capabilities.merge(&model.capabilities);
        }
        capabilities
    };

    if let Some(overrides) = capability_overrides {
        capabilities = capabilities.merge(overrides);
    }
    capabilities
}

fn bridge_model_metadata(
    profile: &common::profiles::schema::ProfileDef,
    target_api_type: &str,
    model: &connections::ProfileBridgeModelRoute,
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
            web_search: false,
        };
    };
    let model_def = catalog::find_model(endpoint, &model.upstream_model);
    let capabilities = resolved_bridge_model_capabilities(
        profile,
        target_api_type,
        Some(&model.upstream_model),
        Some(&model.capabilities),
    );
    BridgeModelMetadata {
        context_window: model_def.and_then(|model_def| model_def.context_window),
        image_input: capabilities.image_input,
        file_input: capabilities.file_input,
        web_search: capabilities.web_search,
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

fn log_content_sanitization(
    profile_id: &str,
    target_api_type: &str,
    client_protocol: BridgeProtocol,
    upstream_protocol: BridgeProtocol,
    sanitization: ContentSanitization,
) {
    if !sanitization.changed() {
        return;
    }
    tracing::warn!(
        target: "server::web_server::api_bridge",
        profile_id = %profile_id,
        target_api_type = %target_api_type,
        client_protocol = ?client_protocol,
        upstream_protocol = ?upstream_protocol,
        image_omitted = sanitization.image_omitted,
        file_omitted = sanitization.file_omitted,
        "API bridge omitted unsupported request content before upstream forwarding"
    );
}

fn client_api_type_from_scope(scope: &str) -> Option<&str> {
    [
        "claude-desktop",
        "codex-desktop",
        "claude",
        "codex",
        "gemini",
        "opencode",
        "pi",
    ]
    .iter()
    .find_map(|agent_id| scope.strip_prefix(&format!("{agent_id}-")))
}

fn validate_manual_scope(scope: &str) -> Result<(), (StatusCode, String)> {
    if scope.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            "manual bridge scope must not be empty".to_string(),
        ));
    }
    if scope.len() > 128 || !scope.chars().all(is_manual_scope_char) {
        return Err((
            StatusCode::BAD_REQUEST,
            "manual bridge scope must be 1-128 characters and contain only ASCII letters, digits, '.', '_' or '-'".to_string(),
        ));
    }
    Ok(())
}

fn apply_wire_model(request: &mut Value, model: &str) {
    if let Some(object) = request.as_object_mut() {
        if object.contains_key("__va_model") {
            object.insert("__va_model".to_string(), Value::String(model.to_string()));
        }
        if object.contains_key("model") || !object.contains_key("__va_model") {
            object.insert("model".to_string(), Value::String(model.to_string()));
        }
    }
}

fn wire_model(request: &Value) -> Option<String> {
    request
        .get("model")
        .or_else(|| request.get("__va_model"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

fn is_manual_scope_char(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '-')
}

pub(super) fn record_json_error(
    record: Option<&ActiveBridgeRecord>,
    status: StatusCode,
    message: &str,
) -> Response {
    let body = json_error_body(message);
    if let Some(record) = record {
        record.bridge_json_response(status, &body);
    }
    (status, Json(body)).into_response()
}

fn json_error(status: StatusCode, message: &str) -> Response {
    (status, Json(json_error_body(message))).into_response()
}

fn json_error_body(message: &str) -> Value {
    json!({
        "error": {
            "message": message,
            "type": "vibearound_bridge_error",
        }
    })
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use common::config::HttpProxyConfig;
    use common::profiles::schema::{AuthMode, ProfileDef};

    use super::{
        client_api_type_from_scope, has_auth_header, proxy_http_client, render_bridge_headers,
        validate_manual_scope,
    };

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
    fn client_api_type_parser_accepts_desktop_scopes() {
        assert_eq!(
            client_api_type_from_scope("codex-desktop-openai-responses"),
            Some("openai-responses")
        );
        assert_eq!(
            client_api_type_from_scope("claude-desktop-anthropic"),
            Some("anthropic")
        );
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

    #[test]
    fn bridge_headers_render_profile_secret_placeholders() {
        let mut credentials = BTreeMap::new();
        credentials.insert("api_key".to_string(), "key-123".to_string());
        let profile = ProfileDef {
            id: "custom-test".to_string(),
            label: "Custom Test".to_string(),
            provider: "custom".to_string(),
            auth_mode: AuthMode::ApiKey,
            api_types: vec!["anthropic".to_string()],
            credentials,
            overrides: BTreeMap::new(),
            api_configs: BTreeMap::new(),
            use_settings_proxy: false,
            provider_settings: Default::default(),
            connections: BTreeMap::new(),
        };
        let headers = [
            ("Authorization".to_string(), "Bearer $apiKey".to_string()),
            ("X-Api-Key".to_string(), "$apiKey".to_string()),
        ]
        .into_iter()
        .collect();

        let rendered = render_bridge_headers(&headers, &profile);

        assert_eq!(
            rendered.get("Authorization").map(String::as_str),
            Some("Bearer key-123")
        );
        assert_eq!(
            rendered.get("X-Api-Key").map(String::as_str),
            Some("key-123")
        );
        assert!(has_auth_header(&rendered));
    }
}
