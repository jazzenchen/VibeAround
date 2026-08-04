use axum::body::Bytes;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::Response;
use serde_json::Value;

use super::super::bridge_recording::RecordedPayload;
use super::super::AppState;
use super::{bridge_handler, bridge_record_metadata, record_json_error, BridgeProtocol};

pub async fn legacy_responses_handler(
    State(state): State<AppState>,
    Path((profile_id, target_api_type)): Path<(String, String)>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    handle_post_bridge_request(
        state,
        profile_id,
        None,
        None,
        target_api_type,
        BridgeProtocol::OpenAiResponses,
        headers,
        body,
        |_| Ok(()),
    )
    .await
}

pub async fn local_responses_handler(
    State(state): State<AppState>,
    Path((profile_id, scope, target_api_type)): Path<(String, String, String)>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let route_scope = scope.clone();
    handle_post_bridge_request(
        state,
        profile_id,
        Some(route_scope),
        Some(scope),
        target_api_type,
        BridgeProtocol::OpenAiResponses,
        headers,
        body,
        |_| Ok(()),
    )
    .await
}

pub async fn legacy_chat_completions_handler(
    State(state): State<AppState>,
    Path((profile_id, target_api_type)): Path<(String, String)>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    handle_post_bridge_request(
        state,
        profile_id,
        None,
        None,
        target_api_type,
        BridgeProtocol::OpenAiChat,
        headers,
        body,
        |_| Ok(()),
    )
    .await
}

pub async fn local_chat_completions_handler(
    State(state): State<AppState>,
    Path((profile_id, scope, target_api_type)): Path<(String, String, String)>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let route_scope = scope.clone();
    handle_post_bridge_request(
        state,
        profile_id,
        Some(route_scope),
        Some(scope),
        target_api_type,
        BridgeProtocol::OpenAiChat,
        headers,
        body,
        |_| Ok(()),
    )
    .await
}

pub async fn legacy_messages_handler(
    State(state): State<AppState>,
    Path((profile_id, target_api_type)): Path<(String, String)>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    handle_post_bridge_request(
        state,
        profile_id,
        None,
        None,
        target_api_type,
        BridgeProtocol::AnthropicMessages,
        headers,
        body,
        |_| Ok(()),
    )
    .await
}

pub async fn legacy_models_handler(
    State(state): State<AppState>,
    Path((profile_id, target_api_type)): Path<(String, String)>,
) -> Response {
    super::models_handler(
        profile_id,
        None,
        None,
        target_api_type,
        state.service_side.image_input.is_configured(),
    )
    .await
}

pub async fn legacy_gemini_generate_content_handler(
    State(state): State<AppState>,
    Path((profile_id, target_api_type, _version, model_action)): Path<(
        String,
        String,
        String,
        String,
    )>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    handle_post_bridge_request(
        state,
        profile_id,
        None,
        None,
        target_api_type,
        BridgeProtocol::GeminiGenerateContent,
        headers,
        body,
        move |original_request| attach_gemini_route_metadata(original_request, &model_action),
    )
    .await
}

pub async fn local_messages_handler(
    State(state): State<AppState>,
    Path((profile_id, scope, target_api_type)): Path<(String, String, String)>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let route_scope = scope.clone();
    handle_post_bridge_request(
        state,
        profile_id,
        Some(route_scope),
        Some(scope),
        target_api_type,
        BridgeProtocol::AnthropicMessages,
        headers,
        body,
        |_| Ok(()),
    )
    .await
}

pub async fn local_models_handler(
    State(state): State<AppState>,
    Path((profile_id, scope, target_api_type)): Path<(String, String, String)>,
    headers: HeaderMap,
) -> Response {
    if let Some(response) = super::validate_local_bridge_client(&state, &headers) {
        return response;
    }
    let route_scope = scope.clone();
    super::models_handler(
        profile_id,
        Some(route_scope),
        Some(scope),
        target_api_type,
        state.service_side.image_input.is_configured(),
    )
    .await
}

pub async fn local_gemini_generate_content_handler(
    State(state): State<AppState>,
    Path((profile_id, scope, target_api_type, _version, model_action)): Path<(
        String,
        String,
        String,
        String,
        String,
    )>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let route_scope = scope.clone();
    handle_post_bridge_request(
        state,
        profile_id,
        Some(route_scope),
        Some(scope),
        target_api_type,
        BridgeProtocol::GeminiGenerateContent,
        headers,
        body,
        move |original_request| attach_gemini_route_metadata(original_request, &model_action),
    )
    .await
}

#[allow(clippy::too_many_arguments)]
async fn handle_post_bridge_request<F>(
    state: AppState,
    profile_id: String,
    route_scope: Option<String>,
    manual_scope: Option<String>,
    target_api_type: String,
    client_protocol: BridgeProtocol,
    headers: HeaderMap,
    body: Bytes,
    transform_request: F,
) -> Response
where
    F: FnOnce(&mut Value) -> Result<(), String>,
{
    if manual_scope.is_some() {
        if let Some(response) = super::validate_local_bridge_client(&state, &headers) {
            return response;
        }
    }
    let request_id = uuid::Uuid::new_v4().to_string();
    let recorder_subscribers = state.bridge_recorder.subscriber_count();
    let original_request_payload =
        (recorder_subscribers > 0).then(|| RecordedPayload::from_bytes(&body));
    let record = state.bridge_recorder.begin(
        request_id.clone(),
        bridge_record_metadata(
            &profile_id,
            route_scope.as_ref(),
            manual_scope.as_ref(),
            &target_api_type,
            client_protocol,
            None,
            None,
            false,
        ),
        original_request_payload,
    );
    let mut original_request = match serde_json::from_slice::<Value>(&body) {
        Ok(value) => value,
        Err(error) => {
            return record_json_error(
                record.as_ref(),
                StatusCode::BAD_REQUEST,
                &format!("invalid JSON request body: {error}"),
            );
        }
    };
    if let Err(message) = transform_request(&mut original_request) {
        return record_json_error(record.as_ref(), StatusCode::BAD_REQUEST, &message);
    }
    bridge_handler(
        state,
        profile_id,
        route_scope,
        manual_scope,
        target_api_type,
        client_protocol,
        headers,
        request_id,
        record,
        original_request,
    )
    .await
}

fn attach_gemini_route_metadata(
    original_request: &mut Value,
    model_action: &str,
) -> Result<(), String> {
    let Some((model, action)) = parse_gemini_model_action(model_action) else {
        return Err(
            "Gemini route must end with {model}:generateContent or {model}:streamGenerateContent"
                .to_string(),
        );
    };
    va_ai_api_bridge::translator::gemini_generate_content::attach_route_metadata(
        original_request,
        model,
        action == "streamGenerateContent",
    );
    Ok(())
}

fn parse_gemini_model_action(model_action: &str) -> Option<(&str, &str)> {
    let (model, action) = model_action.rsplit_once(':')?;
    matches!(action, "generateContent" | "streamGenerateContent").then_some((model, action))
}
