use std::sync::Arc;

use async_trait::async_trait;
use common::search::{SearchError, SearchToolRuntime, WebSearchRequest, WebSearchResponse};
use serde_json::{json, Map, Value};
use va_ai_api_bridge::{
    ContentBlock, ServerToolDeclaration, ServerToolKind, ToolChoice, UniversalItem,
    UniversalRequest, UniversalResponse, UniversalTool,
};

use super::super::bridge_recording::ActiveBridgeRecord;

pub(super) const WEB_SEARCH_FALLBACK_TOOL_NAME: &str = "vibearound_web_search";
pub(super) const MAX_WEB_SEARCH_FALLBACK_ROUNDS: usize = 4;

#[derive(Debug, Clone)]
pub(super) struct WebSearchFallback {
    pub(super) original_stream: bool,
    default_request: WebSearchRequest,
}

#[async_trait]
pub(super) trait WebSearchProvider {
    async fn search(&self, request: WebSearchRequest) -> Result<WebSearchResponse, SearchError>;
}

#[derive(Clone, Default)]
pub(super) struct HostWebSearchProvider {
    runtime: Option<Arc<SearchToolRuntime>>,
}

impl HostWebSearchProvider {
    pub(super) fn new(runtime: Option<Arc<SearchToolRuntime>>) -> Self {
        Self { runtime }
    }
}

#[async_trait]
impl WebSearchProvider for HostWebSearchProvider {
    async fn search(&self, request: WebSearchRequest) -> Result<WebSearchResponse, SearchError> {
        let runtime = self
            .runtime
            .as_ref()
            .ok_or_else(|| SearchError::new("search provider runtime is not running"))?;
        runtime.search(request).await.map_err(|error| {
            tracing::warn!(
                target: "server::web_server::api_bridge",
                error = %error,
                "supervised va-search-tool failed"
            );
            error
        })
    }
}

#[derive(Debug, Clone, PartialEq)]
struct HostToolCall {
    id: String,
    arguments: Value,
}

pub(super) fn request_needs_web_search_fallback(request: &UniversalRequest) -> bool {
    request
        .server_tools
        .iter()
        .any(|tool| is_web_search_kind(tool.kind))
}

pub(super) fn prepare_web_search_fallback(
    request: &mut UniversalRequest,
) -> Option<WebSearchFallback> {
    let declaration = request
        .server_tools
        .iter()
        .find(|tool| is_web_search_kind(tool.kind))?;
    let original_stream = request.stream;
    let default_request = request_defaults_from_declaration(declaration);

    if !request
        .tools
        .iter()
        .any(|tool| tool.name == WEB_SEARCH_FALLBACK_TOOL_NAME)
    {
        request.tools.push(web_search_fallback_tool());
    }

    if matches!(
        request.tool_choice,
        Some(ToolChoice::ServerTool {
            kind: ServerToolKind::WebSearch | ServerToolKind::XSearch
        })
    ) {
        request.tool_choice = Some(ToolChoice::Tool {
            name: WEB_SEARCH_FALLBACK_TOOL_NAME.to_string(),
        });
    }

    request.stream = false;

    Some(WebSearchFallback {
        original_stream,
        default_request,
    })
}

pub(super) async fn append_web_search_results(
    request: &mut UniversalRequest,
    response: UniversalResponse,
    fallback: &WebSearchFallback,
    provider: &impl WebSearchProvider,
    record: Option<&ActiveBridgeRecord>,
    round: usize,
) -> Result<bool, String> {
    let (calls, has_other_tool_calls) = collect_host_tool_calls(&response);
    if calls.is_empty() {
        return Ok(false);
    }
    if has_other_tool_calls {
        return Err(
            "web search fallback cannot be mixed with client tool calls in the same model turn"
                .to_string(),
        );
    }

    request.input.extend(response.output);
    for call in calls {
        let tool_call_id = call.id.clone();
        let search_request = search_request_from_tool_arguments(&fallback.default_request, &call);
        let (content, is_error, trace) = match provider.search(search_request.clone()).await {
            Ok(response) => {
                let content = search_tool_result_content(&response, &search_request);
                (
                    content,
                    false,
                    json!({
                        "round": round,
                        "toolCallId": tool_call_id,
                        "request": search_request,
                        "response": response
                    }),
                )
            }
            Err(error) => {
                let error = error.to_string();
                let trace_error = error.clone();
                (
                    json!({
                        "provider": "vibearound",
                        "error": error
                    })
                    .to_string(),
                    true,
                    json!({
                        "round": round,
                        "toolCallId": tool_call_id,
                        "request": search_request,
                        "error": trace_error
                    }),
                )
            }
        };
        if let Some(record) = record {
            record.search(&trace);
        }
        request.input.push(UniversalItem::ToolResult {
            tool_call_id: call.id,
            content: vec![ContentBlock::Text { text: content }],
            is_error,
            extensions: Default::default(),
        });
    }
    request.tool_choice = Some(ToolChoice::Auto);
    Ok(true)
}

fn search_tool_result_content(response: &WebSearchResponse, request: &WebSearchRequest) -> String {
    let snippet_limit = snippet_char_limit(request.search_context_size.as_deref());
    let results = response
        .results
        .iter()
        .enumerate()
        .map(|(index, result)| {
            let mut value = Map::new();
            value.insert("index".to_string(), json!(index + 1));
            insert_non_empty(&mut value, "title", &result.title, usize::MAX);
            insert_non_empty(&mut value, "url", &result.url, usize::MAX);
            insert_non_empty(&mut value, "source", &result.source, usize::MAX);
            insert_non_empty(&mut value, "snippet", &result.snippet, snippet_limit);
            insert_non_empty(&mut value, "content", &result.content, snippet_limit);
            if let Some(score) = result.score {
                value.insert("score".to_string(), json!(score));
            }
            if let Some(published_date) = &result.published_date {
                insert_non_empty(&mut value, "publishedDate", published_date, usize::MAX);
            }
            Value::Object(value)
        })
        .collect::<Vec<_>>();

    json!({
        "provider": response.provider,
        "query": response.query,
        "results": results,
        "citations": response.citations,
    })
    .to_string()
}

fn snippet_char_limit(search_context_size: Option<&str>) -> usize {
    match search_context_size {
        Some("low") => 320,
        Some("high") => 1_600,
        _ => 800,
    }
}

fn insert_non_empty(object: &mut Map<String, Value>, key: &str, value: &str, limit: usize) {
    if let Some(text) = compact_text(value, limit) {
        object.insert(key.to_string(), Value::String(text));
    }
}

fn compact_text(value: &str, limit: usize) -> Option<String> {
    let text = value.split_whitespace().collect::<Vec<_>>().join(" ");
    if text.is_empty() {
        return None;
    }
    if text.chars().count() <= limit {
        return Some(text);
    }
    let mut clipped = text
        .chars()
        .take(limit.saturating_sub(3))
        .collect::<String>();
    clipped.push_str("...");
    Some(clipped)
}

fn collect_host_tool_calls(response: &UniversalResponse) -> (Vec<HostToolCall>, bool) {
    let mut calls = Vec::new();
    let mut has_other_tool_calls = false;
    for item in &response.output {
        match item {
            UniversalItem::ToolCall {
                id,
                name,
                arguments,
                ..
            } if name == WEB_SEARCH_FALLBACK_TOOL_NAME => calls.push(HostToolCall {
                id: id.clone(),
                arguments: arguments.clone(),
            }),
            UniversalItem::ToolCall { .. } => has_other_tool_calls = true,
            UniversalItem::Message { content, .. } => {
                for block in content {
                    match block {
                        ContentBlock::ToolCall {
                            id,
                            name,
                            arguments,
                            ..
                        } if name == WEB_SEARCH_FALLBACK_TOOL_NAME => calls.push(HostToolCall {
                            id: id.clone(),
                            arguments: arguments.clone(),
                        }),
                        ContentBlock::ToolCall { .. } => has_other_tool_calls = true,
                        _ => {}
                    }
                }
            }
            _ => {}
        }
    }
    (calls, has_other_tool_calls)
}

fn request_defaults_from_declaration(declaration: &ServerToolDeclaration) -> WebSearchRequest {
    let config = declaration.config.as_object();
    let include_domains = string_array_field(&declaration.config, "include_domains")
        .or_else(|| string_array_field(&declaration.config, "allowed_domains"))
        .unwrap_or_default();
    let exclude_domains =
        string_array_field(&declaration.config, "exclude_domains").unwrap_or_default();
    let max_results = config
        .and_then(|config| {
            config
                .get("max_results")
                .or_else(|| config.get("maxResults"))
                .or_else(|| config.get("num_results"))
        })
        .and_then(Value::as_u64)
        .map(|value| value as usize);
    let search_context_size = config
        .and_then(|config| {
            config
                .get("search_context_size")
                .or_else(|| config.get("searchContextSize"))
        })
        .and_then(Value::as_str)
        .map(ToString::to_string);

    WebSearchRequest {
        query: String::new(),
        max_results,
        include_domains,
        exclude_domains,
        search_context_size,
        providers: Vec::new(),
    }
}

fn search_request_from_tool_arguments(
    defaults: &WebSearchRequest,
    call: &HostToolCall,
) -> WebSearchRequest {
    let mut request = defaults.clone();
    let object = call.arguments.as_object();
    if let Some(query) = object
        .and_then(|object| object.get("query"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|query| !query.is_empty())
    {
        request.query = query.to_string();
    }
    if let Some(max_results) = object
        .and_then(|object| {
            object
                .get("max_results")
                .or_else(|| object.get("maxResults"))
                .or_else(|| object.get("num_results"))
        })
        .and_then(Value::as_u64)
    {
        request.max_results = Some(max_results as usize);
    }
    if let Some(include_domains) = string_array_field(&call.arguments, "include_domains")
        .or_else(|| string_array_field(&call.arguments, "allowed_domains"))
    {
        request.include_domains = include_domains;
    }
    if let Some(exclude_domains) = string_array_field(&call.arguments, "exclude_domains") {
        request.exclude_domains = exclude_domains;
    }
    if let Some(search_context_size) = object
        .and_then(|object| {
            object
                .get("search_context_size")
                .or_else(|| object.get("searchContextSize"))
        })
        .and_then(Value::as_str)
    {
        request.search_context_size = Some(search_context_size.to_string());
    }
    request
}

fn string_array_field(value: &Value, key: &str) -> Option<Vec<String>> {
    let values = value.as_object()?.get(key)?.as_array()?;
    Some(
        values
            .iter()
            .filter_map(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToString::to_string)
            .collect(),
    )
}

fn web_search_fallback_tool() -> UniversalTool {
    UniversalTool {
        name: WEB_SEARCH_FALLBACK_TOOL_NAME.to_string(),
        description: Some(
            "Search the web through the VibeAround host when the upstream model provider does not support native server-side web search."
                .to_string(),
        ),
        input_schema: Some(json!({
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "The web search query."
                },
                "max_results": {
                    "type": "integer",
                    "minimum": 1,
                    "maximum": 20,
                    "description": "Maximum number of normalized search results to return per enabled source."
                },
                "include_domains": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Optional domains to prefer or restrict to."
                },
                "exclude_domains": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Optional domains to exclude."
                },
                "search_context_size": {
                    "type": "string",
                    "enum": ["low", "medium", "high"],
                    "description": "Provider-style context size hint."
                }
            },
            "required": ["query"],
            "additionalProperties": false
        })),
        strict: Some(false),
        extensions: Default::default(),
    }
}

fn is_web_search_kind(kind: ServerToolKind) -> bool {
    matches!(kind, ServerToolKind::WebSearch | ServerToolKind::XSearch)
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use va_ai_api_bridge::{ServerToolDeclaration, WireProtocol};

    use super::*;

    #[test]
    fn injects_fallback_tool_for_server_web_search() {
        let mut request = UniversalRequest {
            server_tools: vec![ServerToolDeclaration {
                kind: ServerToolKind::WebSearch,
                wire_type: "web_search".to_string(),
                source_protocol: WireProtocol::OpenAiResponses,
                name: None,
                config: json!({
                    "search_context_size": "low",
                    "allowed_domains": ["example.com"]
                }),
                raw: json!({"type": "web_search"}),
                extensions: Default::default(),
            }],
            tool_choice: Some(ToolChoice::ServerTool {
                kind: ServerToolKind::WebSearch,
            }),
            stream: true,
            ..UniversalRequest::default()
        };

        let fallback = prepare_web_search_fallback(&mut request).expect("fallback");

        assert!(fallback.original_stream);
        assert!(!request.stream);
        assert!(request
            .tools
            .iter()
            .any(|tool| tool.name == WEB_SEARCH_FALLBACK_TOOL_NAME));
        assert_eq!(
            request.tool_choice,
            Some(ToolChoice::Tool {
                name: WEB_SEARCH_FALLBACK_TOOL_NAME.to_string()
            })
        );
        assert_eq!(
            fallback.default_request.include_domains,
            vec!["example.com".to_string()]
        );
    }

    #[tokio::test]
    async fn appends_tool_results_for_host_search_calls() {
        let fallback = WebSearchFallback {
            original_stream: false,
            default_request: WebSearchRequest::default(),
        };
        let mut request = UniversalRequest::default();
        let response = UniversalResponse {
            output: vec![UniversalItem::ToolCall {
                id: "call_search".to_string(),
                name: WEB_SEARCH_FALLBACK_TOOL_NAME.to_string(),
                arguments: json!({ "query": "vaaab web search" }),
                extensions: Default::default(),
            }],
            ..UniversalResponse::default()
        };

        let appended =
            append_web_search_results(&mut request, response, &fallback, &TestProvider, None, 1)
                .await
                .expect("append results");

        assert!(appended);
        assert_eq!(request.input.len(), 2);
        assert!(matches!(
            request.input.last(),
            Some(UniversalItem::ToolResult {
                is_error: false,
                ..
            })
        ));
    }

    struct TestProvider;

    #[async_trait]
    impl WebSearchProvider for TestProvider {
        async fn search(
            &self,
            request: WebSearchRequest,
        ) -> Result<WebSearchResponse, SearchError> {
            Ok(WebSearchResponse {
                provider: "test".to_string(),
                query: request.query,
                results: Vec::new(),
                citations: Vec::new(),
            })
        }
    }

    #[test]
    fn formats_compact_search_results_with_content() {
        let request = WebSearchRequest {
            query: "today news".to_string(),
            search_context_size: Some("low".to_string()),
            ..WebSearchRequest::default()
        };
        let response = WebSearchResponse {
            provider: "test".to_string(),
            query: request.query.clone(),
            citations: vec!["https://example.com/news".to_string()],
            results: vec![common::search::WebSearchResult {
                title: "Example News".to_string(),
                url: "https://example.com/news".to_string(),
                snippet: "A concise snippet that is safe to pass through.".to_string(),
                content: "Raw provider content is still forwarded for model grounding.".to_string(),
                score: Some(0.8),
                published_date: Some("2026-06-17".to_string()),
                source: "exa".to_string(),
            }],
        };

        let content = search_tool_result_content(&response, &request);
        let payload: Value = serde_json::from_str(&content).expect("json payload");

        assert_eq!(payload["results"][0]["title"], "Example News");
        assert_eq!(
            payload["results"][0]["snippet"],
            "A concise snippet that is safe to pass through."
        );
        assert_eq!(
            payload["results"][0]["content"],
            "Raw provider content is still forwarded for model grounding."
        );
    }
}
