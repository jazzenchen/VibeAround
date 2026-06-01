#[cfg(test)]
use common::profiles::catalog::EndpointDef;
use common::profiles::catalog::{self, ContentCapabilities};
use common::profiles::schema::ProfileDef;
#[cfg(test)]
use serde_json::Value;
#[cfg(test)]
use va_ai_api_bridge::{ContentBlock, UniversalItem};
use va_ai_api_bridge::{
    sanitize_unsupported_media, MediaSanitization, ModelCapabilities, ResolvedModelSpec,
    UniversalRequest,
};

#[cfg(test)]
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
struct ContentUsage {
    image_input: bool,
    file_input: bool,
}

#[cfg(test)]
impl ContentUsage {
    fn is_empty(self) -> bool {
        !self.image_input && !self.file_input
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(super) struct ContentSanitization {
    pub(super) image_omitted: bool,
    pub(super) file_omitted: bool,
}

impl ContentSanitization {
    pub(super) fn changed(self) -> bool {
        self.image_omitted || self.file_omitted
    }
}

impl From<MediaSanitization> for ContentSanitization {
    fn from(value: MediaSanitization) -> Self {
        Self {
            image_omitted: value.image_omitted,
            file_omitted: value.file_omitted,
        }
    }
}

pub(super) fn sanitize_request_content(
    profile: &ProfileDef,
    target_api_type: &str,
    request: &mut UniversalRequest,
) -> ContentSanitization {
    let configured_model = configured_model(profile, target_api_type);
    let model = request.model.as_deref().or(configured_model.as_deref());
    let capabilities = resolve_content_capabilities(profile, target_api_type, model);
    let model_spec = resolved_model_spec(profile, model, capabilities);
    sanitize_unsupported_media(request, &model_spec).into()
}

fn resolved_model_spec(
    profile: &ProfileDef,
    model: Option<&str>,
    capabilities: ContentCapabilities,
) -> ResolvedModelSpec {
    ResolvedModelSpec {
        provider_label: Some(
            catalog::get(&profile.provider)
                .map(|provider| provider.label.clone())
                .unwrap_or_else(|| profile.provider.clone()),
        ),
        model: model.unwrap_or("selected model").to_string(),
        capabilities: ModelCapabilities {
            vision: capabilities.image_input,
            files: capabilities.file_input,
            input_modalities: input_modalities(capabilities),
            ..ModelCapabilities::default()
        },
        extensions: Default::default(),
    }
}

fn input_modalities(capabilities: ContentCapabilities) -> Vec<String> {
    let mut modalities = vec!["text".to_string()];
    if capabilities.image_input {
        modalities.push("image".to_string());
    }
    if capabilities.file_input {
        modalities.push("file".to_string());
    }
    modalities
}

#[cfg(test)]
pub(super) fn validate_request_content(
    profile: &ProfileDef,
    target_api_type: &str,
    request: &UniversalRequest,
) -> Result<(), String> {
    let usage = request_content_usage(request);
    if usage.is_empty() {
        return Ok(());
    }

    let configured_model = configured_model(profile, target_api_type);
    let model = request.model.as_deref().or(configured_model.as_deref());
    let capabilities = resolve_content_capabilities(profile, target_api_type, model);

    let mut missing = ContentUsage::default();
    let mut unsupported = Vec::new();
    if usage.image_input && !capabilities.image_input {
        missing.image_input = true;
        unsupported.push("image");
    }
    if usage.file_input && !capabilities.file_input {
        missing.file_input = true;
        unsupported.push("file");
    }
    if unsupported.is_empty() {
        return Ok(());
    }

    let provider_label = catalog::get(&profile.provider)
        .map(|provider| provider.label.as_str())
        .unwrap_or(profile.provider.as_str());
    let model_label = model.unwrap_or("selected model");
    let alternatives = compatible_models(profile, target_api_type, model, missing);
    let alternative_hint = if alternatives.is_empty() {
        String::new()
    } else {
        format!(
            " Compatible models for {} input on this endpoint: {}.",
            content_usage_label(missing),
            alternatives.join(", ")
        )
    };
    Err(format!(
        "{provider_label} model '{model_label}' does not support {} input. Switch to a model that supports it, or remove the attachment.{alternative_hint}",
        unsupported.join(" or ")
    ))
}

fn resolve_content_capabilities(
    profile: &ProfileDef,
    target_api_type: &str,
    model: Option<&str>,
) -> ContentCapabilities {
    let overrides = profile.overrides.get(target_api_type);
    if let Some(capabilities) = overrides.and_then(|overrides| overrides.capabilities.clone()) {
        return capabilities;
    }

    let Some(provider) = catalog::get(&profile.provider) else {
        return ContentCapabilities::default();
    };
    let endpoint_id = selected_endpoint_id(profile, target_api_type);
    let Some(endpoint) = catalog::find_endpoint(provider, target_api_type, endpoint_id) else {
        return ContentCapabilities::default();
    };

    let mut capabilities = endpoint.capabilities.content.clone();
    let model = model
        .and_then(clean_model_id)
        .or_else(|| configured_model(profile, target_api_type))
        .or_else(|| {
            endpoint
                .models
                .first()
                .and_then(|model| clean_model_id(&model.id))
        });
    if let Some(model) = model {
        if let Some(model_def) = catalog::find_model(endpoint, &model) {
            capabilities = capabilities.merge(&model_def.capabilities);
        }
    }
    capabilities
}

#[cfg(test)]
fn compatible_models(
    profile: &ProfileDef,
    target_api_type: &str,
    selected_model: Option<&str>,
    required: ContentUsage,
) -> Vec<String> {
    let Some(provider) = catalog::get(&profile.provider) else {
        return Vec::new();
    };
    let Some(endpoint) = catalog::find_endpoint(
        provider,
        target_api_type,
        selected_endpoint_id(profile, target_api_type),
    ) else {
        return Vec::new();
    };
    let selected_model = selected_model.and_then(clean_model_id);

    endpoint
        .models
        .iter()
        .filter_map(|model| {
            let id = clean_model_id(&model.id)?;
            if selected_model
                .as_deref()
                .map(|selected| catalog::model_matches(model, selected))
                .unwrap_or(false)
            {
                return None;
            }
            if !supports_required_content(endpoint, &model.capabilities, required) {
                return None;
            }
            Some(id)
        })
        .collect()
}

#[cfg(test)]
fn supports_required_content(
    endpoint: &EndpointDef,
    model_capabilities: &ContentCapabilities,
    required: ContentUsage,
) -> bool {
    let capabilities = endpoint.capabilities.content.merge(model_capabilities);
    (!required.image_input || capabilities.image_input)
        && (!required.file_input || capabilities.file_input)
}

fn selected_endpoint_id<'a>(profile: &'a ProfileDef, target_api_type: &str) -> Option<&'a str> {
    profile
        .overrides
        .get(target_api_type)
        .and_then(|overrides| overrides.endpoint_id.as_deref())
}

#[cfg(test)]
fn content_usage_label(usage: ContentUsage) -> &'static str {
    match (usage.image_input, usage.file_input) {
        (true, true) => "image and file",
        (true, false) => "image",
        (false, true) => "file",
        (false, false) => "attachment",
    }
}

fn configured_model(profile: &ProfileDef, target_api_type: &str) -> Option<String> {
    profile
        .overrides
        .get(target_api_type)
        .and_then(|overrides| overrides.model.as_deref())
        .and_then(clean_model_id)
}

fn clean_model_id(value: &str) -> Option<String> {
    let value = value.trim();
    (!value.is_empty()).then(|| value.to_string())
}

#[cfg(test)]
fn request_content_usage(request: &UniversalRequest) -> ContentUsage {
    let mut usage = ContentUsage::default();
    collect_blocks_usage(&request.instructions, &mut usage);
    for item in &request.input {
        collect_item_usage(item, &mut usage);
    }
    usage
}

#[cfg(test)]
fn collect_item_usage(item: &UniversalItem, usage: &mut ContentUsage) {
    match item {
        UniversalItem::Message { content, .. } | UniversalItem::ToolResult { content, .. } => {
            collect_blocks_usage(content, usage);
        }
        UniversalItem::Unknown { raw } => collect_value_usage(raw, usage),
        UniversalItem::ToolCall { .. } | UniversalItem::Reasoning { .. } => {}
    }
}

#[cfg(test)]
fn collect_blocks_usage(blocks: &[ContentBlock], usage: &mut ContentUsage) {
    for block in blocks {
        collect_block_usage(block, usage);
    }
}

#[cfg(test)]
fn collect_block_usage(block: &ContentBlock, usage: &mut ContentUsage) {
    match block {
        ContentBlock::Image { .. } => usage.image_input = true,
        ContentBlock::File { .. } => usage.file_input = true,
        ContentBlock::ToolResult { content, .. } => collect_blocks_usage(content, usage),
        ContentBlock::Unknown { raw } => collect_value_usage(raw, usage),
        ContentBlock::Text { .. }
        | ContentBlock::ToolCall { .. }
        | ContentBlock::Reasoning { .. } => {}
    }
}

#[cfg(test)]
fn collect_value_usage(value: &Value, usage: &mut ContentUsage) {
    match value {
        Value::Array(values) => {
            for value in values {
                collect_value_usage(value, usage);
            }
        }
        Value::Object(object) => {
            let mut typed_image = false;
            let mut typed_file = false;
            if let Some(kind) = object.get("type").and_then(Value::as_str) {
                match kind {
                    "image" | "input_image" | "image_url" => {
                        typed_image = true;
                        usage.image_input = true;
                    }
                    "document" | "file" | "input_file" => {
                        typed_file = true;
                        usage.file_input = true;
                    }
                    _ => {}
                }
            }
            if object.contains_key("image_url") {
                usage.image_input = true;
            }
            if !typed_image
                && (typed_file
                    || object.contains_key("file_data")
                    || object.contains_key("fileData")
                    || object.contains_key("file_id")
                    || object.contains_key("file_url")
                    || object.contains_key("fileUrl")
                    || object.contains_key("filename"))
            {
                usage.file_input = true;
            }
            for value in object.values() {
                collect_value_usage(value, usage);
            }
        }
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => {}
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use common::profiles::schema::{ApiTypeOverrides, AuthMode, ProviderSettings};
    use serde_json::json;
    use va_ai_api_bridge::{AnthropicMessagesTranslator, OpenAiChatTranslator, Role, WireTranslator};

    use super::*;

    fn profile(provider: &str, model: &str) -> ProfileDef {
        ProfileDef {
            id: format!("{provider}-test"),
            label: provider.to_string(),
            provider: provider.to_string(),
            auth_mode: AuthMode::ApiKey,
            api_types: vec!["openai-chat".to_string()],
            credentials: BTreeMap::new(),
            overrides: [(
                "openai-chat".to_string(),
                ApiTypeOverrides {
                    model: Some(model.to_string()),
                    ..Default::default()
                },
            )]
            .into_iter()
            .collect(),
            use_settings_proxy: false,
            provider_settings: ProviderSettings::default(),
        }
    }

    fn image_request(model: &str) -> UniversalRequest {
        UniversalRequest {
            model: Some(model.to_string()),
            input: vec![UniversalItem::Message {
                role: Role::User,
                id: None,
                content: vec![ContentBlock::Image {
                    media_type: Some("image/png".to_string()),
                    url: Some("https://example.test/a.png".to_string()),
                    data: None,
                    extensions: BTreeMap::new(),
                }],
                extensions: BTreeMap::new(),
            }],
            ..UniversalRequest::default()
        }
    }

    #[test]
    fn rejects_image_for_text_only_model() {
        let error = validate_request_content(
            &profile("deepseek", "deepseek-v4-pro"),
            "openai-chat",
            &image_request("deepseek-v4-pro"),
        )
        .unwrap_err();

        assert!(error.contains("DeepSeek model 'deepseek-v4-pro'"));
        assert!(error.contains("image input"));
    }

    #[test]
    fn allows_catalog_image_model() {
        validate_request_content(
            &profile("dashscope", "qwen3.6-plus"),
            "openai-chat",
            &image_request("qwen3.6-plus"),
        )
        .unwrap();
    }

    #[test]
    fn suggests_compatible_models_from_same_endpoint() {
        let error = validate_request_content(
            &profile("dashscope", "glm-5"),
            "openai-chat",
            &image_request("glm-5"),
        )
        .unwrap_err();

        assert!(error.contains("Alibaba DashScope model 'glm-5'"));
        assert!(error.contains("Compatible models for image input on this endpoint"));
        assert!(error.contains("qwen3.6-plus"));
        assert!(error.contains("qwen3.5-plus"));
        assert!(error.contains("kimi-k2.5"));
    }

    #[test]
    fn custom_override_controls_image_input() {
        let mut profile = profile("custom", "my-vision-model");
        let error =
            validate_request_content(&profile, "openai-chat", &image_request("my-vision-model"))
                .unwrap_err();
        assert!(error.contains("image input"));

        let overrides = profile.overrides.get_mut("openai-chat").unwrap();
        overrides.capabilities = Some(ContentCapabilities {
            image_input: true,
            file_input: false,
        });

        validate_request_content(&profile, "openai-chat", &image_request("my-vision-model"))
            .unwrap();
    }

    #[test]
    fn detects_unknown_file_parts() {
        let request = UniversalRequest {
            input: vec![UniversalItem::Unknown {
                raw: json!({
                    "role": "user",
                    "content": [
                        { "type": "input_file", "file_url": "https://example.test/a.pdf" }
                    ]
                }),
            }],
            ..UniversalRequest::default()
        };

        let error = validate_request_content(
            &profile("deepseek", "deepseek-v4-pro"),
            "openai-chat",
            &request,
        )
        .unwrap_err();

        assert!(error.contains("file input"));
    }

    #[test]
    fn detects_openrouter_file_data_alias() {
        let request = UniversalRequest {
            input: vec![UniversalItem::Unknown {
                raw: json!({
                    "role": "user",
                    "content": [
                        { "fileData": "https://example.test/a.pdf" }
                    ]
                }),
            }],
            ..UniversalRequest::default()
        };

        let error = validate_request_content(
            &profile("deepseek", "deepseek-v4-pro"),
            "openai-chat",
            &request,
        )
        .unwrap_err();

        assert!(error.contains("file input"));
    }

    #[test]
    fn sanitizes_image_for_text_only_model() {
        let mut request = image_request("deepseek-v4-pro");

        let result = sanitize_request_content(
            &profile("deepseek", "deepseek-v4-pro"),
            "openai-chat",
            &mut request,
        );

        assert!(result.changed());
        assert!(result.image_omitted);
        assert!(!request_content_usage(&request).image_input);
        let UniversalItem::Message { content, .. } = &request.input[0] else {
            panic!("expected message");
        };
        let ContentBlock::Text { text } = &content[0] else {
            panic!("expected placeholder text");
        };
        assert!(text.contains("Image attachment omitted"));
        assert!(text.contains("DeepSeek deepseek-v4-pro"));
        assert!(text.contains("Do not infer image contents"));
    }

    #[test]
    fn leaves_supported_image_model_unchanged() {
        let mut request = image_request("qwen3.6-plus");

        let result = sanitize_request_content(
            &profile("dashscope", "qwen3.6-plus"),
            "openai-chat",
            &mut request,
        );

        assert!(!result.changed());
        assert!(request_content_usage(&request).image_input);
    }

    #[test]
    fn sanitizes_file_inside_tool_result() {
        let mut request = UniversalRequest {
            model: Some("deepseek-v4-pro".to_string()),
            input: vec![UniversalItem::ToolResult {
                tool_call_id: "call_1".to_string(),
                content: vec![ContentBlock::File {
                    media_type: Some("application/pdf".to_string()),
                    filename: Some("paper.pdf".to_string()),
                    url: Some("https://example.test/paper.pdf".to_string()),
                    data: None,
                    extensions: BTreeMap::new(),
                }],
                is_error: false,
                extensions: BTreeMap::new(),
            }],
            ..UniversalRequest::default()
        };

        let result = sanitize_request_content(
            &profile("deepseek", "deepseek-v4-pro"),
            "openai-chat",
            &mut request,
        );

        assert!(result.file_omitted);
        assert!(!request_content_usage(&request).file_input);
        let UniversalItem::ToolResult { content, .. } = &request.input[0] else {
            panic!("expected tool result");
        };
        let ContentBlock::Text { text } = &content[0] else {
            panic!("expected placeholder text");
        };
        assert!(text.contains("File attachment omitted"));
        assert!(text.contains("Do not infer file contents"));
    }

    #[test]
    fn sanitizes_unknown_media_payloads() {
        let mut request = UniversalRequest {
            input: vec![UniversalItem::Unknown {
                raw: json!({
                    "role": "user",
                    "content": [
                        { "type": "input_image", "image_url": "https://example.test/a.png" }
                    ]
                }),
            }],
            ..UniversalRequest::default()
        };

        let result = sanitize_request_content(
            &profile("deepseek", "deepseek-v4-pro"),
            "openai-chat",
            &mut request,
        );

        assert!(result.image_omitted);
        assert!(!request_content_usage(&request).image_input);
        let UniversalItem::Message { content, .. } = &request.input[0] else {
            panic!("expected unknown item to become placeholder message");
        };
        assert!(matches!(content[0], ContentBlock::Text { .. }));
    }

    #[test]
    fn claude_image_history_to_text_only_chat_does_not_forward_image() {
        let raw_anthropic = json!({
            "model": "deepseek-v4-pro",
            "max_tokens": 1024,
            "messages": [{
                "role": "user",
                "content": [
                    {
                        "type": "image",
                        "source": {
                            "type": "base64",
                            "media_type": "image/png",
                            "data": "iVBORw0KGgo="
                        }
                    },
                    { "type": "text", "text": "What is this?" }
                ]
            }]
        });
        let mut request = AnthropicMessagesTranslator
            .decode_request(raw_anthropic)
            .unwrap();

        let result = sanitize_request_content(
            &profile("deepseek", "deepseek-v4-pro"),
            "openai-chat",
            &mut request,
        );
        let upstream = OpenAiChatTranslator.encode_request(&request).unwrap();
        let upstream_text = upstream.to_string();

        assert!(result.image_omitted);
        assert!(!upstream_text.contains("image_url"));
        assert!(!upstream_text.contains("iVBORw0KGgo="));
        assert!(upstream_text.contains("Image attachment omitted"));
        assert!(upstream_text.contains("What is this?"));
    }
}
