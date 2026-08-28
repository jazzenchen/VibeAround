use serde_json::{Map, Value};

use super::BridgeProtocol;

pub(super) fn normalize_target_request(request: &mut Value, protocol: BridgeProtocol) {
    match protocol {
        BridgeProtocol::AnthropicMessages => normalize_anthropic_messages_request(request),
        BridgeProtocol::OpenAiChat => normalize_openai_chat_request(request),
        BridgeProtocol::OpenAiResponses => {}
        BridgeProtocol::GeminiGenerateContent => {
            va_ai_api_bridge::translator::gemini_generate_content::strip_route_metadata(request);
        }
    }
}

fn normalize_anthropic_messages_request(request: &mut Value) {
    if let Some(object) = request.as_object_mut() {
        object
            .entry("max_tokens")
            .or_insert_with(|| Value::Number(4096_u64.into()));
    }
}

fn normalize_openai_chat_request(request: &mut Value) {
    let Some(messages) = request.get_mut("messages").and_then(Value::as_array_mut) else {
        return;
    };
    for message in messages {
        let Some(content) = message.get_mut("content").and_then(Value::as_array_mut) else {
            continue;
        };
        for part in content {
            normalize_openai_chat_part(part);
        }
    }
}

// Parts in shapes we recognize are rewritten to the Chat Completions wire form;
// anything else is forwarded untouched so the upstream judges it itself.
fn normalize_openai_chat_part(part: &mut Value) {
    let Some(object) = part.as_object_mut() else {
        return;
    };
    match object.get("type").and_then(Value::as_str) {
        Some("input_text") => {
            object.insert("type".to_string(), Value::String("text".to_string()));
            if !object.contains_key("text") {
                let text = object
                    .remove("input_text")
                    .unwrap_or_else(|| Value::String(String::new()));
                object.insert("text".to_string(), text);
            } else {
                object.remove("input_text");
            }
        }
        Some("input_image") => {
            if let Some(image_url) = chat_image_url_from_input_image(object) {
                *object = Map::from_iter([
                    ("type".to_string(), Value::String("image_url".to_string())),
                    ("image_url".to_string(), image_url),
                ]);
            }
        }
        Some("image_url") => {
            if let Some(image_url) = object
                .get("image_url")
                .and_then(|value| normalize_image_url_value(value, None))
            {
                object.insert("image_url".to_string(), image_url);
            }
        }
        _ => {}
    }
}

fn chat_image_url_from_input_image(object: &Map<String, Value>) -> Option<Value> {
    if let Some(value) = object.get("image_url") {
        return normalize_image_url_value(value, object.get("detail"));
    }
    if let Some(value) = object.get("input_image") {
        return normalize_image_source_value(value, object.get("detail"));
    }
    normalize_image_source_object(object, object.get("detail"))
}

fn normalize_image_source_value(value: &Value, detail: Option<&Value>) -> Option<Value> {
    match value {
        Value::String(url) => Some(image_url_object(url.clone(), detail.cloned())),
        Value::Object(object) => normalize_image_source_object(object, detail),
        _ => None,
    }
}

fn normalize_image_url_value(value: &Value, detail: Option<&Value>) -> Option<Value> {
    match value {
        Value::String(url) => Some(image_url_object(url.clone(), detail.cloned())),
        Value::Object(object) => {
            if let Some(url) = object.get("url").and_then(Value::as_str) {
                let mut out = object.clone();
                if !out.contains_key("detail") {
                    if let Some(detail) = detail.cloned() {
                        out.insert("detail".to_string(), detail);
                    }
                }
                out.insert("url".to_string(), Value::String(url.to_string()));
                return Some(Value::Object(out));
            }
            if let Some(value) = object.get("image_url") {
                return normalize_image_url_value(value, object.get("detail").or(detail));
            }
            normalize_image_source_object(object, detail)
        }
        _ => None,
    }
}

fn normalize_image_source_object(
    object: &Map<String, Value>,
    detail: Option<&Value>,
) -> Option<Value> {
    if let Some(value) = object.get("image_url") {
        return normalize_image_url_value(value, object.get("detail").or(detail));
    }
    if let Some(url) = object.get("url").and_then(Value::as_str) {
        return Some(image_url_object(
            url.to_string(),
            object.get("detail").or(detail).cloned(),
        ));
    }
    if let Some(data) = object.get("data").and_then(Value::as_str) {
        let url = if data.starts_with("data:") {
            data.to_string()
        } else {
            let media_type = object
                .get("media_type")
                .or_else(|| object.get("mime_type"))
                .or_else(|| object.get("mime"))
                .and_then(Value::as_str)
                .unwrap_or("image/png");
            format!("data:{media_type};base64,{data}")
        };
        return Some(image_url_object(
            url,
            object.get("detail").or(detail).cloned(),
        ));
    }
    None
}

fn image_url_object(url: String, detail: Option<Value>) -> Value {
    let mut object = Map::new();
    object.insert("url".to_string(), Value::String(url));
    if let Some(detail) = detail {
        object.insert("detail".to_string(), detail);
    }
    Value::Object(object)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn normalizes_responses_parts_in_openai_chat_requests() {
        let mut request = json!({
            "messages": [{
                "role": "user",
                "content": [
                    { "type": "input_text", "input_text": "describe" },
                    {
                        "type": "input_image",
                        "image_url": "https://example.test/image.jpg"
                    }
                ]
            }]
        });

        normalize_target_request(&mut request, BridgeProtocol::OpenAiChat);

        let content = request["messages"][0]["content"].as_array().unwrap();
        assert_eq!(content[0], json!({ "type": "text", "text": "describe" }));
        assert_eq!(
            content[1],
            json!({
                "type": "image_url",
                "image_url": { "url": "https://example.test/image.jpg" }
            })
        );
    }

    #[test]
    fn normalizes_nested_input_image_data_for_openai_chat() {
        let mut request = json!({
            "messages": [{
                "role": "user",
                "content": [{
                    "type": "input_image",
                    "input_image": {
                        "media_type": "image/jpeg",
                        "data": "abc123",
                        "detail": "low"
                    }
                }]
            }]
        });

        normalize_target_request(&mut request, BridgeProtocol::OpenAiChat);

        assert_eq!(
            request["messages"][0]["content"][0],
            json!({
                "type": "image_url",
                "image_url": {
                    "url": "data:image/jpeg;base64,abc123",
                    "detail": "low"
                }
            })
        );
    }

    #[test]
    fn forwards_unrecognized_input_image_untouched_for_openai_chat() {
        let mut request = json!({
            "messages": [{
                "role": "user",
                "content": [{ "type": "input_image", "file_id": "file-123" }]
            }]
        });

        normalize_target_request(&mut request, BridgeProtocol::OpenAiChat);

        assert_eq!(
            request["messages"][0]["content"][0],
            json!({ "type": "input_image", "file_id": "file-123" })
        );
    }
}
