use std::path::{Path as StdPath, PathBuf};

use axum::body::Bytes;
use axum::extract::Path;
use axum::http::{HeaderMap, StatusCode};
use axum::response::Response;
use serde_json::Value;
use tokio::sync::mpsc;
use uuid::Uuid;

mod conversations;
mod events;
mod models;
mod prompt;
mod turn;

pub use models::local_agent_models_handler;

use super::{json_error, record_json_error, BridgeProtocol};
use prompt::seed_request_to_acp_prompt;

pub(super) const LOCAL_AGENT_CHANNEL_KIND: &str = "api";
const HEADER_WORKSPACE: &str = "x-vibearound-cwd";
/// Explicit conversation key for the chat/messages protocols; requests
/// carrying it share one persistent backend session per key. The Responses
/// protocol chains `previous_response_id` instead.
const HEADER_CONVERSATION: &str = "x-vibearound-conversation";

pub async fn local_agent_responses_handler(
    Path((agent_id, profile_id)): Path<(String, String)>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let agent_id = match local_agent_gate(&agent_id) {
        Ok(agent_id) => agent_id,
        Err(response) => return response,
    };
    handle_local_agent_request(
        agent_id,
        profile_id,
        BridgeProtocol::OpenAiResponses,
        headers,
        body,
    )
    .await
}

pub async fn local_agent_chat_completions_handler(
    Path((agent_id, profile_id)): Path<(String, String)>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let agent_id = match local_agent_gate(&agent_id) {
        Ok(agent_id) => agent_id,
        Err(response) => return response,
    };
    handle_local_agent_request(
        agent_id,
        profile_id,
        BridgeProtocol::OpenAiChat,
        headers,
        body,
    )
    .await
}

pub async fn local_agent_messages_handler(
    Path((agent_id, profile_id)): Path<(String, String)>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let agent_id = match local_agent_gate(&agent_id) {
        Ok(agent_id) => agent_id,
        Err(response) => return response,
    };
    handle_local_agent_request(
        agent_id,
        profile_id,
        BridgeProtocol::AnthropicMessages,
        headers,
        body,
    )
    .await
}

pub(super) fn local_agent_api_enabled() -> bool {
    common::config::ensure_loaded().local_agent_api.enabled
}

/// Service switch + per-agent opt-in, resolving the path segment to the
/// canonical agent id on the way. The service switch gates the route family
/// (503 when off, as before); each agent must additionally be opted in under
/// `local_agent_api.agents`, and one that is not answers 403 — an explicit
/// policy refusal, unlike the direct-profile 429 below.
pub(super) fn local_agent_gate(agent_id: &str) -> Result<String, Response> {
    if !local_agent_api_enabled() {
        return Err(local_agent_api_disabled_response());
    }
    let canonical = match common::resources::resolve_agent_id(agent_id) {
        Ok(canonical) => canonical,
        Err(error) => return Err(json_error(StatusCode::BAD_REQUEST, &error.to_string())),
    };
    if !common::config::ensure_loaded()
        .local_agent_api
        .agent_enabled(&canonical)
    {
        return Err(json_error(
            StatusCode::FORBIDDEN,
            &format!(
                "local agent API is not enabled for agent `{canonical}`; opt the agent in under VibeAround settings"
            ),
        ));
    }
    Ok(canonical)
}

/// The direct profile runs an agent on its own native credentials, and that
/// path is closed over the API: only managed profiles are served. Answered as
/// 429 so callers treat it as a hard limit rather than a retryable fault.
pub(super) fn direct_profile_response(profile_id: &str) -> Option<Response> {
    let profile = common::agent::launch::normalize_launch_profile_id(Some(profile_id));
    if common::agent::launch::profile_uses_vibearound_credentials(&profile) {
        return None;
    }
    Some(json_error(
        StatusCode::TOO_MANY_REQUESTS,
        "local agent API access with the direct profile is limited; use a managed profile",
    ))
}

pub(super) fn local_agent_api_disabled_response() -> Response {
    json_error(
        StatusCode::SERVICE_UNAVAILABLE,
        "local agent API service is disabled",
    )
}

async fn handle_local_agent_request(
    agent_id: String,
    profile_id: String,
    protocol: BridgeProtocol,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    if let Some(response) = direct_profile_response(&profile_id) {
        return response;
    }
    let raw = match serde_json::from_slice::<Value>(&body) {
        Ok(value) => value,
        Err(error) => {
            return json_error(
                StatusCode::BAD_REQUEST,
                &format!("invalid JSON request body: {error}"),
            );
        }
    };
    let request = match protocol.decode_agent_request(raw) {
        Ok(request) => request,
        Err(error) => return json_error(StatusCode::BAD_REQUEST, &error.to_string()),
    };
    let model_id = request
        .model
        .clone()
        .filter(|model| !model.trim().is_empty())
        .map(|model| model.trim().to_string());
    let workspace = request_workspace(&headers, &agent_id);
    let conversation_key = header_value(&headers, HEADER_CONVERSATION);
    let previous_response_id = (protocol == BridgeProtocol::OpenAiResponses)
        .then(|| {
            source_raw(&request)
                .and_then(|raw| raw.get("previous_response_id"))
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(ToOwned::to_owned)
        })
        .flatten();
    let response_id = format!("resp_{}", Uuid::new_v4().simple());

    // Resolve the conversation this request belongs to, if any. A chained
    // Responses request carries increments only, so its history cannot seed
    // a lost session; everything else can.
    let conversation = if let Some(previous) = previous_response_id {
        match conversations::registry().lookup_response(&previous) {
            conversations::ResponseLookup::Found(conversation) => Some((conversation, true)),
            conversations::ResponseLookup::NotFound => {
                return json_error(
                    StatusCode::NOT_FOUND,
                    &format!(
                        "previous response `{previous}` was not found (the daemon may have \
                         restarted); retry without previous_response_id to start a new chain"
                    ),
                );
            }
            conversations::ResponseLookup::Superseded => {
                return json_error(
                    StatusCode::CONFLICT,
                    &format!(
                        "response `{previous}` was superseded; only the latest response in a \
                         conversation can be continued"
                    ),
                );
            }
        }
    } else if let Some(key) = conversation_key.as_deref() {
        Some((
            conversations::registry().resolve_keyed(key, &agent_id, &profile_id, &workspace),
            false,
        ))
    } else if protocol == BridgeProtocol::OpenAiResponses {
        // Every Responses turn is continuable through its response id, so a
        // keyless request starts a conversation of its own.
        Some((
            conversations::registry().create_for_response(
                &agent_id,
                &profile_id,
                &workspace,
                &response_id,
            ),
            false,
        ))
    } else {
        None
    };

    let Some((conversation, chained)) = conversation else {
        // Sessionless one-shot: seed a throwaway session and answer.
        let prompt = match seed_request_to_acp_prompt(&request) {
            Ok(prompt) => prompt,
            Err(message) => return json_error(StatusCode::UNPROCESSABLE_ENTITY, &message),
        };
        let turn = turn::LocalAgentTurn {
            agent_id,
            profile_id,
            model_id,
            workspace,
            prompt,
        };
        return if request.stream {
            turn::local_agent_stream_response(turn, protocol)
        } else {
            turn::local_agent_completion_response(turn, protocol).await
        };
    };

    // Changed client instructions mean the old session no longer matches
    // what the client believes it is talking to: reseed under the same key.
    let fingerprint = conversations::instructions_fingerprint(&request.instructions);
    if conversation.instructions_changed(fingerprint) {
        conversation.reset_session().await;
    }
    conversation.set_instructions_fingerprint(fingerprint);

    let seed_prompt = if chained {
        None
    } else {
        match seed_request_to_acp_prompt(&request) {
            Ok(prompt) => Some(prompt),
            Err(message) => return json_error(StatusCode::UNPROCESSABLE_ENTITY, &message),
        }
    };
    let tail_items = prompt::tail_input_segment(&request.input);
    let tail_prompt = prompt::tail_segment_to_acp_prompt(tail_items).ok();
    if seed_prompt.is_none() && tail_prompt.is_none() {
        return json_error(
            StatusCode::UNPROCESSABLE_ENTITY,
            "request adds no new input to answer",
        );
    }

    if protocol == BridgeProtocol::OpenAiResponses {
        conversations::registry().advance_response_id(&conversation, &response_id);
    }

    let turn = turn::ConversationTurn {
        conversation,
        model_id,
        response_id,
        seed_prompt,
        tail_prompt,
    };
    let mut response = if request.stream {
        turn::conversation_stream_response(turn, protocol)
    } else {
        turn::conversation_completion_response(turn, protocol).await
    };
    if let Some(key) = conversation_key {
        if let Ok(value) = axum::http::HeaderValue::from_str(&key) {
            response.headers_mut().insert(HEADER_CONVERSATION, value);
        }
    }
    response
}

fn header_value(headers: &HeaderMap, name: &str) -> Option<String> {
    headers
        .get(name)
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

pub(super) type LaunchArgsAndEnv = (Vec<String>, Vec<(String, String)>);

pub(super) fn launch_args_and_env(
    agent_id: &str,
    profile_id: &str,
    workspace: &StdPath,
    route: &common::routing::RouteKey,
) -> Result<LaunchArgsAndEnv, String> {
    let profile_id = common::agent::launch::normalize_launch_profile_id(Some(profile_id));
    let mut env_vars = vec![
        (
            "VIBEAROUND_CHANNEL_KIND".to_string(),
            route.channel_kind.clone(),
        ),
        ("VIBEAROUND_CHAT_ID".to_string(), route.chat_id.clone()),
        ("VIBEAROUND_AGENT_KIND".to_string(), agent_id.to_string()),
        (
            "VIBEAROUND_API_REQUEST_ID".to_string(),
            route.chat_id.clone(),
        ),
    ];
    let mut extra_args = Vec::new();
    if common::agent::launch::profile_uses_vibearound_credentials(&profile_id) {
        let applied = common::agent::launch::materialize_profile_for_agent(
            &profile_id,
            agent_id,
            workspace,
            route,
        )
        .map_err(|error| format!("{error:#}"))?;
        env_vars.extend(applied.env);
        extra_args.extend(applied.command_args);
    }
    common::agent::launch::append_profile_id_env(&mut env_vars, Some(&profile_id));
    let prefs = common::agent_state::read_prefs();
    extra_args.extend(common::agent_state::resolve_agent_acp_args(
        &prefs, agent_id,
    ));
    Ok((extra_args, env_vars))
}

fn send_events(
    tx: &mpsc::UnboundedSender<turn::LocalAgentTurnEvent>,
    events: Vec<va_ai_api_bridge::UniversalEvent>,
) {
    if !events.is_empty() {
        let _ = tx.send(turn::LocalAgentTurnEvent::Events(events));
    }
}

pub(super) fn request_workspace(headers: &HeaderMap, agent_id: &str) -> PathBuf {
    headers
        .get(HEADER_WORKSPACE)
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| common::config::ensure_loaded().resolve_workspace(agent_id))
}

fn source_raw(request: &va_ai_api_bridge::UniversalRequest) -> Option<&Value> {
    request
        .source
        .as_ref()
        .and_then(|source| source.raw.as_ref())
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_client_protocol::schema::v1 as acp;
    use va_ai_api_bridge::{
        ContentBlock as UniversalContentBlock, Extensions, Role, UniversalItem, UniversalRequest,
        Usage,
    };

    #[test]
    fn direct_profile_aliases_are_rejected_with_429() {
        for profile in ["direct", "Default", "none", "OFF", "", "  "] {
            let response =
                direct_profile_response(profile).expect("direct profile alias is rejected");
            assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
        }
    }

    #[test]
    fn managed_profiles_pass_the_direct_profile_gate() {
        assert!(direct_profile_response("deepseek-8uaepyrmp4b6").is_none());
    }

    #[test]
    fn extracts_model_ids_from_acp_model_config_options() {
        let config = acp::SessionConfigOption::select(
            "model",
            "Model",
            "claude-sonnet-4-6",
            vec![
                acp::SessionConfigSelectOption::new("claude-sonnet-4-6", "Claude Sonnet"),
                acp::SessionConfigSelectOption::new("claude-opus-4-5", "Claude Opus"),
                acp::SessionConfigSelectOption::new("claude-sonnet-4-6", "Duplicate"),
            ],
        )
        .category(acp::SessionConfigOptionCategory::Model);
        let other = acp::SessionConfigOption::select(
            "permission-mode",
            "Permission mode",
            "default",
            vec![acp::SessionConfigSelectOption::new("default", "Default")],
        );

        assert_eq!(
            models::models_from_acp_config_options(&[other, config]),
            vec![
                models::LocalAgentModel {
                    id: "claude-sonnet-4-6".to_string()
                },
                models::LocalAgentModel {
                    id: "claude-opus-4-5".to_string()
                },
            ]
        );
    }

    #[test]
    fn finds_model_config_option_id_for_setting_model_id() {
        let config = acp::SessionConfigOption::select(
            "model",
            "Model",
            "claude-sonnet-4-6",
            vec![acp::SessionConfigSelectOption::new(
                "claude-sonnet-4-6",
                "Claude Sonnet",
            )],
        )
        .category(acp::SessionConfigOptionCategory::Model);
        let other = acp::SessionConfigOption::select(
            "permission-mode",
            "Permission mode",
            "default",
            vec![acp::SessionConfigSelectOption::new("default", "Default")],
        );

        assert_eq!(
            turn::model_config_option_id(Some(&[other, config])),
            Some("model".to_string())
        );
        assert_eq!(turn::model_config_option_id(None), None);
    }

    fn user_item(text: &str) -> UniversalItem {
        UniversalItem::Message {
            role: Role::User,
            id: None,
            content: vec![UniversalContentBlock::Text {
                text: text.to_string(),
            }],
            extensions: Extensions::new(),
        }
    }

    fn assistant_item(text: &str) -> UniversalItem {
        UniversalItem::Message {
            role: Role::Assistant,
            id: None,
            content: vec![UniversalContentBlock::Text {
                text: text.to_string(),
            }],
            extensions: Extensions::new(),
        }
    }

    fn tool_call_item(id: &str) -> UniversalItem {
        UniversalItem::ToolCall {
            id: id.to_string(),
            name: "read_file".to_string(),
            arguments: serde_json::json!({"path": "foo.rs"}),
            extensions: Extensions::new(),
        }
    }

    fn tool_result_item(id: &str, text: &str) -> UniversalItem {
        UniversalItem::ToolResult {
            tool_call_id: id.to_string(),
            content: vec![UniversalContentBlock::Text {
                text: text.to_string(),
            }],
            is_error: false,
            extensions: Extensions::new(),
        }
    }

    fn block_text(block: &acp::ContentBlock) -> &str {
        match block {
            acp::ContentBlock::Text(text) => &text.text,
            _ => "",
        }
    }

    #[test]
    fn tail_segment_is_the_contiguous_non_assistant_run() {
        let input = vec![
            user_item("first"),
            assistant_item("reply"),
            user_item("second"),
        ];
        let segment = prompt::tail_input_segment(&input);
        assert_eq!(segment.len(), 1);
        assert!(matches!(
            &segment[0],
            UniversalItem::Message {
                role: Role::User,
                ..
            }
        ));

        let input = vec![
            user_item("first"),
            assistant_item("reply"),
            tool_call_item("call-1"),
            tool_result_item("call-1", "file contents"),
            user_item("and now this"),
        ];
        let segment = prompt::tail_input_segment(&input);
        assert_eq!(
            segment.len(),
            2,
            "tool result and user message are both new input"
        );

        // Everything already answered: nothing new to prompt.
        let input = vec![user_item("first"), assistant_item("reply")];
        assert!(prompt::tail_input_segment(&input).is_empty());

        // No history at all: the whole input is the segment.
        let input = vec![user_item("only")];
        assert_eq!(prompt::tail_input_segment(&input).len(), 1);
    }

    #[test]
    fn lone_user_segment_prompts_as_plain_content() {
        let segment = [user_item("just this")];
        let blocks = prompt::tail_segment_to_acp_prompt(&segment).expect("prompt builds");
        assert_eq!(blocks.len(), 1);
        assert_eq!(block_text(&blocks[0]), "just this");
    }

    #[test]
    fn mixed_segment_keeps_role_labels() {
        let segment = [
            tool_result_item("call-1", "file contents"),
            user_item("continue"),
        ];
        let blocks = prompt::tail_segment_to_acp_prompt(&segment).expect("prompt builds");
        assert!(block_text(&blocks[0]).starts_with("[tool_result:call-1]"));
        assert!(blocks.iter().any(|block| block_text(block) == "[user]"));
    }

    #[test]
    fn seeding_wraps_history_in_the_bridge_envelope() {
        let request = UniversalRequest {
            instructions: vec![UniversalContentBlock::Text {
                text: "Be concise.".to_string(),
            }],
            input: vec![
                user_item("Hello"),
                assistant_item("Hi"),
                user_item("Continue"),
            ],
            ..UniversalRequest::default()
        };

        let blocks = seed_request_to_acp_prompt(&request).expect("prompt builds");
        let texts: Vec<&str> = blocks.iter().map(block_text).collect();
        assert!(texts[0].starts_with("[VibeAround local-agent bridge]"));
        assert!(texts.contains(&"Client-provided instructions:"));
        assert!(texts.contains(&"<conversation_replay>"));
        assert!(texts.contains(&"</conversation_replay>"));
        assert!(texts.last().unwrap().starts_with("End of replay."));
    }

    #[test]
    fn bare_single_user_request_skips_the_envelope() {
        let request = UniversalRequest {
            input: vec![user_item("Find recent news.")],
            ..UniversalRequest::default()
        };

        let blocks = seed_request_to_acp_prompt(&request).expect("prompt builds");
        assert_eq!(blocks.len(), 1);
        assert_eq!(block_text(&blocks[0]), "Find recent news.");
    }

    #[test]
    fn empty_seed_request_is_rejected() {
        let request = UniversalRequest::default();
        assert!(seed_request_to_acp_prompt(&request).is_err());
    }

    #[test]
    fn builds_sessionless_chat_transcript() {
        let request = UniversalRequest {
            instructions: vec![UniversalContentBlock::Text {
                text: "Be concise.".to_string(),
            }],
            input: vec![
                UniversalItem::Message {
                    role: Role::User,
                    id: None,
                    content: vec![UniversalContentBlock::Text {
                        text: "Hello".to_string(),
                    }],
                    extensions: Extensions::new(),
                },
                UniversalItem::Message {
                    role: Role::Assistant,
                    id: None,
                    content: vec![UniversalContentBlock::Text {
                        text: "Hi".to_string(),
                    }],
                    extensions: Extensions::new(),
                },
                UniversalItem::Message {
                    role: Role::User,
                    id: None,
                    content: vec![UniversalContentBlock::Text {
                        text: "Continue".to_string(),
                    }],
                    extensions: Extensions::new(),
                },
            ],
            ..UniversalRequest::default()
        };

        let transcript = prompt::universal_request_to_transcript(&request);
        assert!(transcript.contains("Instructions:\nBe concise."));
        assert!(transcript.contains("[user]\nHello"));
        assert!(transcript.contains("[assistant]\nHi"));
        assert!(transcript.contains("[user]\nContinue"));
    }

    #[test]
    fn converts_openai_responses_media_to_acp_prompt_blocks() {
        let request = BridgeProtocol::OpenAiResponses
            .decode_agent_request(serde_json::json!({
                "model": "local",
                "input": [{
                    "role": "user",
                    "content": [
                        { "type": "input_text", "text": "describe these" },
                        { "type": "input_image", "image_url": "data:image/png;base64,abc123" },
                        {
                            "type": "input_file",
                            "filename": "paper.pdf",
                            "file_data": "data:application/pdf;base64,AAAA"
                        }
                    ]
                }]
            }))
            .expect("responses request decodes");

        let prompt = seed_request_to_acp_prompt(&request).expect("prompt builds");

        assert!(prompt.iter().any(|block| {
            matches!(
                block,
                acp::ContentBlock::Image(image)
                    if image.mime_type == "image/png" && image.data == "abc123"
            )
        }));
        assert!(prompt.iter().any(|block| {
            matches!(
                block,
                acp::ContentBlock::Resource(resource)
                    if matches!(
                        &resource.resource,
                        acp::EmbeddedResourceResource::BlobResourceContents(blob)
                            if blob.mime_type.as_deref() == Some("application/pdf")
                                && blob.blob == "AAAA"
                                && blob.uri == "urn:vibearound:local-agent:file:paper-pdf"
                    )
            )
        }));
    }

    #[test]
    fn converts_openai_chat_image_url_to_acp_resource_link() {
        let request = BridgeProtocol::OpenAiChat
            .decode_agent_request(serde_json::json!({
                "model": "local",
                "messages": [{
                    "role": "user",
                    "content": [
                        { "type": "text", "text": "describe it" },
                        {
                            "type": "image_url",
                            "image_url": { "url": "https://example.test/image.png" }
                        }
                    ]
                }]
            }))
            .expect("chat request decodes");

        let prompt = seed_request_to_acp_prompt(&request).expect("prompt builds");

        assert!(prompt.iter().any(|block| {
            matches!(
                block,
                acp::ContentBlock::ResourceLink(link)
                    if link.name == "image" && link.uri == "https://example.test/image.png"
            )
        }));
    }

    #[test]
    fn converts_anthropic_document_to_acp_blob_resource() {
        let request = BridgeProtocol::AnthropicMessages
            .decode_agent_request(serde_json::json!({
                "model": "local",
                "max_tokens": 1024,
                "messages": [{
                    "role": "user",
                    "content": [
                        { "type": "text", "text": "summarize" },
                        {
                            "type": "document",
                            "title": "report.pdf",
                            "source": {
                                "type": "base64",
                                "media_type": "application/pdf",
                                "data": "BBBB"
                            }
                        }
                    ]
                }]
            }))
            .expect("anthropic request decodes");

        let prompt = seed_request_to_acp_prompt(&request).expect("prompt builds");

        assert!(prompt.iter().any(|block| {
            matches!(
                block,
                acp::ContentBlock::Resource(resource)
                    if matches!(
                        &resource.resource,
                        acp::EmbeddedResourceResource::BlobResourceContents(blob)
                            if blob.mime_type.as_deref() == Some("application/pdf")
                                && blob.blob == "BBBB"
                                && blob.uri == "urn:vibearound:local-agent:file:report-pdf"
                    )
            )
        }));
    }

    #[test]
    fn extracts_previous_response_id_from_responses_source() {
        let request = UniversalRequest {
            source: Some(va_ai_api_bridge::SourcePayload {
                protocol: va_ai_api_bridge::WireProtocol::OpenAiResponses,
                raw: Some(serde_json::json!({
                    "model": "local",
                    "previous_response_id": "resp_old",
                    "input": "continue"
                })),
            }),
            ..UniversalRequest::default()
        };

        let previous = source_raw(&request)
            .and_then(|raw| raw.get("previous_response_id"))
            .and_then(serde_json::Value::as_str);
        assert_eq!(previous, Some("resp_old"));
    }

    #[test]
    fn maps_acp_text_notification_to_universal_delta() {
        let notification = acp::SessionNotification::new(
            "session-1".to_string(),
            acp::SessionUpdate::AgentMessageChunk(acp::ContentChunk::new(acp::ContentBlock::Text(
                acp::TextContent::new("hello"),
            ))),
        );

        let events = events::acp_notification_to_events(&notification);
        assert_eq!(
            events,
            vec![va_ai_api_bridge::UniversalEvent::TextDelta {
                index: 0,
                text: "hello".to_string(),
            }]
        );
    }

    #[test]
    fn maps_prompt_response_usage_to_final_events() {
        let usage = Usage {
            input_tokens: Some(2),
            output_tokens: Some(3),
            total_tokens: Some(5),
        };
        let events = events::final_events(acp::StopReason::EndTurn, Some(usage.clone()));

        assert!(matches!(
            events.get(1),
            Some(va_ai_api_bridge::UniversalEvent::MessageDone {
                finish_reason: Some(va_ai_api_bridge::FinishReason::Stop),
                usage: Some(event_usage),
                ..
            }) if event_usage == &usage
        ));
        assert!(matches!(
            events.get(2),
            Some(va_ai_api_bridge::UniversalEvent::ResponseDone {
                usage: Some(event_usage),
                ..
            }) if event_usage == &usage
        ));
    }
}
