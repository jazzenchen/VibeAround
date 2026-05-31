use axum::http::StatusCode;
use axum::response::Response;
use common::profiles::google_oauth;
use serde_json::{json, Map, Value};

use common::profiles::schema::ProfileDef;

use super::{json_error, upstream::send_upstream_request_with_rate_limit_retry};

pub(super) async fn bearer_token(client: &reqwest::Client) -> Result<String, Response> {
    if let Ok(token) = std::env::var("GOOGLE_CLOUD_ACCESS_TOKEN") {
        let token = token.trim();
        if !token.is_empty() {
            return Ok(token.to_string());
        }
    }

    match google_oauth::vibearound_access_token(client).await {
        Ok(token) => return Ok(token),
        Err(primary_error) => {
            let mut fallback_errors = vec![format!("{primary_error:#}")];
            for path in [
                google_oauth::google_application_credentials_path(),
                google_oauth::gemini_cli_credentials_path(),
            ]
            .into_iter()
            .flatten()
            {
                match google_oauth::access_token_from_path(client, &path).await {
                    Ok(token) => return Ok(token),
                    Err(error) => fallback_errors.push(format!("{}: {error:#}", path.display())),
                }
            }
            Err(json_error(
                StatusCode::UNAUTHORIZED,
                &format!(
                    "Google account OAuth is not connected. Choose Gemini → Google accounts in VibeAround, click Sign in with Google, then try again. Details: {}",
                    fallback_errors.join("; ")
                ),
            ))
        }
    }
}

pub(super) async fn resolve_project_id(
    client: &reqwest::Client,
    base_url: &str,
    profile: &ProfileDef,
    token: &str,
) -> Result<Option<String>, Response> {
    let configured_project = configured_project_id(profile);

    let body = json!({
        "cloudaicompanionProject": configured_project.clone(),
        "metadata": {
            "ideType": "IDE_UNSPECIFIED",
            "platform": "PLATFORM_UNSPECIFIED",
            "pluginType": "GEMINI",
            "duetProject": configured_project.clone(),
        }
    });
    let body = serde_json::to_vec(&body).map_err(|error| {
        json_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            &format!("failed to serialize Google Code Assist account check: {error}"),
        )
    })?;
    let url = format!(
        "{}/v1internal:loadCodeAssist",
        base_url.trim_end_matches('/')
    );
    let response = send_upstream_request_with_rate_limit_retry(
        client
            .post(url)
            .bearer_auth(token)
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .body(body),
    )
    .await
    .map_err(|error| {
        json_error(
            StatusCode::BAD_GATEWAY,
            &format!("failed to load Google Code Assist account: {error}"),
        )
    })?;

    if !response.status().is_success() {
        let status =
            StatusCode::from_u16(response.status().as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);
        let body = response.text().await.unwrap_or_default();
        return Err(json_error(
            status,
            &format!("Google Code Assist account check failed: {body}"),
        ));
    }

    let bytes = response.bytes().await.map_err(|error| {
        json_error(
            StatusCode::BAD_GATEWAY,
            &format!("failed to read Google Code Assist account check: {error}"),
        )
    })?;
    let raw = serde_json::from_slice::<Value>(&bytes).map_err(|error| {
        json_error(
            StatusCode::BAD_GATEWAY,
            &format!("Google Code Assist account check returned invalid JSON: {error}"),
        )
    })?;
    Ok(raw
        .get("cloudaicompanionProject")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .or(configured_project))
}

pub(super) fn wrap_generate_content_request(
    mut request: Value,
    model: &str,
    project: Option<String>,
) -> Result<Value, String> {
    sanitize_gemini_function_schemas(&mut request);
    let object = request
        .as_object_mut()
        .ok_or_else(|| "Gemini Code Assist request must be a JSON object".to_string())?;
    object.remove("model");
    object.remove("__va_model");
    object.remove("__va_stream");

    let mut out = Map::new();
    out.insert("model".to_string(), Value::String(model.to_string()));
    if let Some(project) = project.filter(|value| !value.trim().is_empty()) {
        out.insert("project".to_string(), Value::String(project));
    }
    out.insert(
        "user_prompt_id".to_string(),
        Value::String(uuid::Uuid::new_v4().to_string()),
    );
    out.insert("request".to_string(), request);
    Ok(Value::Object(out))
}

fn sanitize_gemini_function_schemas(request: &mut Value) {
    let Some(tools) = request.get_mut("tools").and_then(Value::as_array_mut) else {
        return;
    };
    for tool in tools {
        for declarations_key in ["function_declarations", "functionDeclarations"] {
            let Some(declarations) = tool.get_mut(declarations_key).and_then(Value::as_array_mut)
            else {
                continue;
            };
            for declaration in declarations {
                for schema_key in ["parameters", "response"] {
                    if let Some(schema) = declaration.get_mut(schema_key) {
                        sanitize_gemini_schema(schema);
                    }
                }
            }
        }
    }
}

fn sanitize_gemini_schema(schema: &mut Value) {
    let Value::Object(object) = schema else {
        return;
    };
    object.retain(|key, _| is_supported_gemini_schema_key(key));

    if let Some(properties) = object.get_mut("properties") {
        match properties {
            Value::Object(properties) => {
                for property_schema in properties.values_mut() {
                    sanitize_gemini_schema(property_schema);
                }
            }
            _ => {
                object.remove("properties");
            }
        }
    }

    if let Some(items) = object.get_mut("items") {
        sanitize_gemini_schema(items);
    }
}

fn is_supported_gemini_schema_key(key: &str) -> bool {
    matches!(
        key,
        "description"
            | "enum"
            | "format"
            | "items"
            | "maximum"
            | "maxItems"
            | "minimum"
            | "minItems"
            | "nullable"
            | "properties"
            | "propertyOrdering"
            | "required"
            | "type"
    )
}

pub(super) fn unwrap_generate_content_response(raw: Value) -> Result<Value, String> {
    let Value::Object(mut object) = raw else {
        return Err("Google Code Assist response must be a JSON object".to_string());
    };
    if object.contains_key("candidates") {
        return Ok(Value::Object(object));
    }
    let trace_id = object
        .get("traceId")
        .and_then(Value::as_str)
        .map(ToOwned::to_owned);
    let mut response = object
        .remove("response")
        .unwrap_or_else(|| json!({ "candidates": [] }));
    let response_object = response
        .as_object_mut()
        .ok_or_else(|| "Google Code Assist response field must be a JSON object".to_string())?;
    if let Some(trace_id) = trace_id {
        response_object
            .entry("responseId".to_string())
            .or_insert(Value::String(trace_id));
    }
    response_object
        .entry("candidates".to_string())
        .or_insert_with(|| Value::Array(Vec::new()));
    Ok(response)
}

fn configured_project_id(profile: &ProfileDef) -> Option<String> {
    profile
        .credentials
        .get("google_cloud_project")
        .cloned()
        .or_else(|| std::env::var("GOOGLE_CLOUD_PROJECT").ok())
        .or_else(|| std::env::var("GOOGLE_CLOUD_PROJECT_ID").ok())
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn wraps_gemini_request_for_code_assist() {
        let wrapped = wrap_generate_content_request(
            json!({
                "__va_model": "gemini-2.5-pro",
                "__va_stream": false,
                "contents": [{ "role": "user", "parts": [{ "text": "hi" }] }]
            }),
            "gemini-2.5-pro",
            Some("project-123".to_string()),
        )
        .expect("wrap request");

        assert_eq!(wrapped["model"], "gemini-2.5-pro");
        assert_eq!(wrapped["project"], "project-123");
        assert_eq!(wrapped["request"]["contents"][0]["role"], "user");
        assert!(wrapped["request"].get("__va_model").is_none());
    }

    #[test]
    fn unwraps_code_assist_response_to_gemini_response() {
        let response = unwrap_generate_content_response(json!({
            "traceId": "trace-1",
            "response": {
                "candidates": [{
                    "content": { "parts": [{ "text": "ok" }] },
                    "finishReason": "STOP"
                }],
                "modelVersion": "gemini-2.5-flash"
            }
        }))
        .expect("unwrap response");

        assert_eq!(response["responseId"], "trace-1");
        assert_eq!(
            response["candidates"][0]["content"]["parts"][0]["text"],
            "ok"
        );
    }

    #[test]
    fn strips_unsupported_json_schema_keywords_from_tool_parameters() {
        let wrapped = wrap_generate_content_request(
            json!({
                "__va_model": "gemini-2.5-flash",
                "contents": [{ "role": "user", "parts": [{ "text": "hi" }] }],
                "tools": [{
                    "function_declarations": [{
                        "name": "search",
                        "parameters": {
                            "$schema": "https://json-schema.org/draft/2020-12/schema",
                            "type": "object",
                            "properties": {
                                "filters": {
                                    "type": "object",
                                    "propertyNames": { "pattern": "^[a-z]+$" },
                                    "additionalProperties": { "type": "string" }
                                }
                            },
                            "required": ["filters"]
                        }
                    }]
                }]
            }),
            "gemini-2.5-flash",
            None,
        )
        .expect("wrap request");

        let params = &wrapped["request"]["tools"][0]["function_declarations"][0]["parameters"];
        assert!(params.get("$schema").is_none());
        assert_eq!(params["type"], "object");
        assert_eq!(params["required"], json!(["filters"]));
        assert!(params["properties"]["filters"]
            .get("propertyNames")
            .is_none());
        assert!(params["properties"]["filters"]
            .get("additionalProperties")
            .is_none());
    }
}
