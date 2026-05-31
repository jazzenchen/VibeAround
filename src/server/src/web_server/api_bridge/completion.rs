use std::time::{SystemTime, UNIX_EPOCH};

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::{json, Value};
use va_ai_api_bridge::{
    ContentBlock, FinishReason, ProviderBridgeAdapter, UniversalEvent, UniversalItem,
    UniversalResponse, Usage,
};

use super::{json_error, BridgeProtocol};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum UpstreamResponseTransform {
    Identity,
    GoogleCodeAssist,
}

pub(super) async fn translated_completion_response(
    upstream: reqwest::Response,
    upstream_protocol: BridgeProtocol,
    agent_protocol: BridgeProtocol,
    provider_adapter: &mut ProviderBridgeAdapter,
    agent_model: Option<String>,
    transform: UpstreamResponseTransform,
) -> Response {
    let bytes = match upstream.bytes().await {
        Ok(bytes) => bytes,
        Err(e) => {
            return json_error(
                StatusCode::BAD_GATEWAY,
                &format!("failed to read upstream response: {e}"),
            );
        }
    };
    let raw = match serde_json::from_slice::<Value>(&bytes) {
        Ok(value) => value,
        Err(e) => {
            return json_error(
                StatusCode::BAD_GATEWAY,
                &format!("upstream returned invalid JSON: {e}"),
            );
        }
    };
    let mut raw = match transform_upstream_response(raw, transform) {
        Ok(raw) => raw,
        Err(message) => return json_error(StatusCode::BAD_GATEWAY, &message),
    };
    if upstream_protocol == BridgeProtocol::OpenAiChat {
        provider_adapter.normalize_chat_response(&mut raw);
    }
    let mut events = match upstream_protocol.decode_upstream_response(raw) {
        Ok(events) => events,
        Err(error) => return json_error(StatusCode::BAD_GATEWAY, &error.to_string()),
    };
    provider_adapter.transform_upstream_events(&mut events);
    apply_agent_model(&mut events, agent_model.as_deref());
    let body = match agent_protocol {
        BridgeProtocol::OpenAiResponses => events_to_openai_response(&events),
        BridgeProtocol::OpenAiChat => events_to_openai_chat_response(&events),
        BridgeProtocol::AnthropicMessages => events_to_anthropic_response(&events),
        BridgeProtocol::GeminiGenerateContent => {
            va_ai_api_bridge::translator::gemini_generate_content::encode_response(&events)
        }
    };
    Json(body).into_response()
}

pub(super) fn transform_upstream_response(
    raw: Value,
    transform: UpstreamResponseTransform,
) -> Result<Value, String> {
    match transform {
        UpstreamResponseTransform::Identity => Ok(raw),
        UpstreamResponseTransform::GoogleCodeAssist => {
            super::google_code_assist::unwrap_generate_content_response(raw)
        }
    }
}

fn apply_agent_model(events: &mut [UniversalEvent], agent_model: Option<&str>) {
    let Some(agent_model) = agent_model else {
        return;
    };
    for event in events {
        if let UniversalEvent::ResponseStart { model, .. } = event {
            *model = Some(agent_model.to_string());
        }
    }
}

#[derive(Default)]
struct ResponseParts {
    id: Option<String>,
    message_id: Option<String>,
    model: Option<String>,
    text: String,
    tool_calls: Vec<ToolCallParts>,
    reasoning_content: String,
    usage: Option<Usage>,
    finish_reason: Option<FinishReason>,
}

#[derive(Default)]
struct ToolCallParts {
    id: String,
    name: Option<String>,
    arguments: String,
}

fn collect_response_parts(events: &[UniversalEvent]) -> ResponseParts {
    collect_response_parts_from_universal(&UniversalResponse::from_events(events))
}

fn collect_response_parts_from_universal(response: &UniversalResponse) -> ResponseParts {
    let mut parts = ResponseParts::default();
    parts.id = response.id.clone();
    parts.model = response.model.clone();
    parts.usage = response.usage.clone();
    parts.finish_reason = response.finish_reason;

    for item in &response.output {
        match item {
            UniversalItem::Message { id, content, .. } => {
                if parts.message_id.is_none() {
                    parts.message_id = id.clone();
                }
                collect_content_blocks(&mut parts, content);
            }
            UniversalItem::ToolCall {
                id,
                name,
                arguments,
                ..
            } => parts.tool_calls.push(ToolCallParts {
                id: id.clone(),
                name: Some(name.clone()),
                arguments: stringify_json(arguments),
            }),
            UniversalItem::Reasoning { text, .. } => {
                if let Some(text) = text {
                    parts.reasoning_content.push_str(text);
                }
            }
            UniversalItem::ToolResult { .. } | UniversalItem::Unknown { .. } => {}
        }
    }
    parts
}

fn collect_content_blocks(parts: &mut ResponseParts, content: &[ContentBlock]) {
    for block in content {
        match block {
            ContentBlock::Text { text } => parts.text.push_str(text),
            ContentBlock::Reasoning {
                text: Some(text), ..
            } => parts.reasoning_content.push_str(text),
            ContentBlock::ToolCall {
                id,
                name,
                arguments,
                ..
            } => parts.tool_calls.push(ToolCallParts {
                id: id.clone(),
                name: Some(name.clone()),
                arguments: stringify_json(arguments),
            }),
            _ => {}
        }
    }
}

fn stringify_json(value: &Value) -> String {
    match value {
        Value::String(value) => value.clone(),
        Value::Null => String::new(),
        value => serde_json::to_string(value).unwrap_or_default(),
    }
}

fn events_to_openai_response(events: &[UniversalEvent]) -> Value {
    let parts = collect_response_parts(events);
    let id = parts.id.unwrap_or_else(|| "resp_bridge".to_string());
    let mut output = Vec::new();
    if !parts.reasoning_content.is_empty() {
        output.push(json!({
            "type": "reasoning",
            "id": "rs_bridge",
            "content": [{
                "type": "reasoning_text",
                "text": parts.reasoning_content,
            }]
        }));
    }
    if !parts.text.is_empty() || parts.tool_calls.is_empty() {
        output.push(json!({
            "type": "message",
            "id": parts.message_id.unwrap_or_else(|| "msg_bridge".to_string()),
            "status": "completed",
            "role": "assistant",
            "content": [{
                "type": "output_text",
                "text": parts.text,
                "annotations": []
            }]
        }));
    }
    for tool_call in &parts.tool_calls {
        output.push(json!({
            "type": "function_call",
            "id": response_function_call_id(tool_call),
            "call_id": tool_call_id(tool_call),
            "name": tool_call_name(tool_call),
            "arguments": tool_call.arguments,
            "status": "completed"
        }));
    }
    json!({
        "id": id,
        "object": "response",
        "created_at": unix_timestamp(),
        "status": "completed",
        "model": parts.model,
        "output": output,
        "usage": usage_to_openai_responses(parts.usage.as_ref())
    })
}

fn events_to_openai_chat_response(events: &[UniversalEvent]) -> Value {
    let parts = collect_response_parts(events);
    let has_tool_calls = !parts.tool_calls.is_empty();
    let mut message = serde_json::Map::new();
    message.insert("role".to_string(), json!("assistant"));
    if has_tool_calls {
        message.insert("content".to_string(), Value::Null);
        message.insert(
            "tool_calls".to_string(),
            Value::Array(
                parts
                    .tool_calls
                    .iter()
                    .map(|tool_call| {
                        json!({
                            "id": tool_call_id(tool_call),
                            "type": "function",
                            "function": {
                                "name": tool_call_name(tool_call),
                                "arguments": tool_call.arguments
                            }
                        })
                    })
                    .collect(),
            ),
        );
    } else {
        message.insert("content".to_string(), Value::String(parts.text));
    }
    if !parts.reasoning_content.is_empty() {
        message.insert(
            "reasoning_content".to_string(),
            Value::String(parts.reasoning_content),
        );
    }
    json!({
        "id": parts.id.unwrap_or_else(|| "chatcmpl_bridge".to_string()),
        "object": "chat.completion",
        "created": unix_timestamp(),
        "model": parts.model,
        "choices": [{
            "index": 0,
            "message": Value::Object(message),
            "finish_reason": if has_tool_calls { json!("tool_calls") } else { finish_to_openai(parts.finish_reason) }
        }],
        "usage": usage_to_openai_chat(parts.usage.as_ref())
    })
}

fn events_to_anthropic_response(events: &[UniversalEvent]) -> Value {
    let parts = collect_response_parts(events);
    let has_tool_calls = !parts.tool_calls.is_empty();
    let mut content = Vec::new();
    if !parts.text.is_empty() || !has_tool_calls {
        content.push(json!({
            "type": "text",
            "text": parts.text
        }));
    }
    for tool_call in &parts.tool_calls {
        content.push(json!({
            "type": "tool_use",
            "id": tool_call_id(tool_call),
            "name": tool_call_name(tool_call),
            "input": tool_call_arguments_value(tool_call),
        }));
    }
    json!({
        "id": parts.message_id.or(parts.id).unwrap_or_else(|| "msg_bridge".to_string()),
        "type": "message",
        "role": "assistant",
        "model": parts.model,
        "content": content,
        "stop_reason": if has_tool_calls { json!("tool_use") } else { finish_to_anthropic(parts.finish_reason) },
        "stop_sequence": Value::Null,
        "usage": usage_to_anthropic(parts.usage.as_ref())
    })
}

fn tool_call_id(tool_call: &ToolCallParts) -> String {
    if tool_call.id.is_empty() {
        "call_bridge".to_string()
    } else {
        tool_call.id.clone()
    }
}

fn response_function_call_id(tool_call: &ToolCallParts) -> String {
    let id = tool_call_id(tool_call);
    if id.starts_with("fc_") {
        id
    } else {
        format!("fc_{id}")
    }
}

fn tool_call_name(tool_call: &ToolCallParts) -> String {
    tool_call
        .name
        .clone()
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| "function".to_string())
}

fn tool_call_arguments_value(tool_call: &ToolCallParts) -> Value {
    serde_json::from_str(&tool_call.arguments)
        .unwrap_or_else(|_| Value::String(tool_call.arguments.clone()))
}

fn usage_to_openai_responses(usage: Option<&Usage>) -> Value {
    match usage {
        Some(usage) => json!({
            "input_tokens": usage.input_tokens,
            "output_tokens": usage.output_tokens,
            "total_tokens": usage.total_tokens
        }),
        None => Value::Null,
    }
}

fn usage_to_openai_chat(usage: Option<&Usage>) -> Value {
    match usage {
        Some(usage) => json!({
            "prompt_tokens": usage.input_tokens,
            "completion_tokens": usage.output_tokens,
            "total_tokens": usage.total_tokens
        }),
        None => Value::Null,
    }
}

fn usage_to_anthropic(usage: Option<&Usage>) -> Value {
    match usage {
        Some(usage) => json!({
            "input_tokens": usage.input_tokens.unwrap_or(0),
            "output_tokens": usage.output_tokens.unwrap_or(0)
        }),
        None => json!({
            "input_tokens": 0,
            "output_tokens": 0
        }),
    }
}

fn finish_to_openai(reason: Option<FinishReason>) -> Value {
    match reason {
        Some(FinishReason::Stop) => json!("stop"),
        Some(FinishReason::Length) => json!("length"),
        Some(FinishReason::ToolCall) => json!("tool_calls"),
        Some(FinishReason::ContentFilter) => json!("content_filter"),
        Some(FinishReason::Error) => json!("error"),
        Some(FinishReason::Unknown) | None => json!("stop"),
    }
}

fn finish_to_anthropic(reason: Option<FinishReason>) -> Value {
    match reason {
        Some(FinishReason::Stop) => json!("end_turn"),
        Some(FinishReason::Length) => json!("max_tokens"),
        Some(FinishReason::ToolCall) => json!("tool_use"),
        Some(FinishReason::ContentFilter) => json!("content_filter"),
        Some(FinishReason::Error) => json!("error"),
        Some(FinishReason::Unknown) | None => json!("end_turn"),
    }
}

fn unix_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}
