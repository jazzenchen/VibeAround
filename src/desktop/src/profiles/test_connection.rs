use std::time::Duration;

use common::config;
use common::profiles::catalog::{self, EndpointDef, ProviderCatalog};
use common::profiles::endpoint_url::join_protocol_endpoint;
use common::profiles::headers::merged_upstream_headers;
use common::profiles::schema::{AuthMode, ProfileDef};
use reqwest::header::{AUTHORIZATION, CONTENT_TYPE};
use serde::Serialize;
use serde_json::{json, Value};

use super::store::ProfileDraft;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProfileConnectionTestResult {
    pub tested_api_types: Vec<String>,
}

pub async fn test_connection(draft: ProfileDraft) -> Result<ProfileConnectionTestResult, String> {
    let profile = draft.into_profile("__test__".to_string());
    if profile.auth_mode != AuthMode::ApiKey {
        return Err("Connection test currently supports API key profiles.".to_string());
    }
    if profile.api_types.is_empty() {
        return Err("Pick at least one API type.".to_string());
    }

    let provider = catalog::get(&profile.provider)
        .ok_or_else(|| format!("unknown provider '{}'", profile.provider))?;
    let client = test_http_client(&profile)?;
    let mut tested_api_types = Vec::new();

    for api_type in &profile.api_types {
        test_api_type(&client, &profile, provider, api_type)
            .await
            .map_err(|error| format!("{api_type}: {error}"))?;
        tested_api_types.push(api_type.clone());
    }

    Ok(ProfileConnectionTestResult { tested_api_types })
}

async fn test_api_type(
    client: &reqwest::Client,
    profile: &ProfileDef,
    provider: &ProviderCatalog,
    api_type: &str,
) -> Result<(), String> {
    let endpoint = selected_endpoint(profile, provider, api_type)?;
    let base_url = profile
        .overrides
        .get(api_type)
        .and_then(|overrides| overrides.base_url.as_deref())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(endpoint.default_base_url.trim())
        .trim_end_matches('/');
    if base_url.is_empty() {
        return Err("Base URL is required.".to_string());
    }
    let model = selected_model(profile, endpoint, api_type)?;
    let payload = test_payload(api_type, &model)?;
    let url = test_url(api_type, base_url, endpoint, &model)?;
    let api_key = profile
        .credentials
        .get("api_key")
        .map(String::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "API key is required.".to_string())?;

    let headers =
        merged_upstream_headers(&endpoint.headers, None).map_err(|error| error.to_string())?;
    let request = apply_auth(
        client
            .post(url)
            .header(CONTENT_TYPE, "application/json")
            .headers(headers)
            .json(&payload),
        api_type,
        endpoint,
        api_key,
    );

    let response = request.send().await.map_err(|error| error.to_string())?;
    let status = response.status();
    let body = response.text().await.unwrap_or_default();
    if !status.is_success() {
        return Err(format!("HTTP {status}: {}", compact_error_body(&body)));
    }
    if body.trim().is_empty() {
        return Err("Provider returned an empty response.".to_string());
    }
    Ok(())
}

fn selected_endpoint<'a>(
    profile: &ProfileDef,
    provider: &'a ProviderCatalog,
    api_type: &str,
) -> Result<&'a EndpointDef, String> {
    let endpoint_id = profile
        .overrides
        .get(api_type)
        .and_then(|overrides| overrides.endpoint_id.as_deref());
    catalog::find_endpoint(provider, api_type, endpoint_id).ok_or_else(|| {
        let suffix = endpoint_id
            .map(|id| format!(" endpoint_id '{id}'"))
            .unwrap_or_default();
        format!(
            "provider '{}' has no endpoint for api kind '{}'{}",
            provider.id, api_type, suffix
        )
    })
}

fn selected_model(
    profile: &ProfileDef,
    endpoint: &EndpointDef,
    api_type: &str,
) -> Result<String, String> {
    let requested = profile
        .overrides
        .get(api_type)
        .and_then(|overrides| overrides.model.as_deref())
        .map(str::trim)
        .filter(|model| !model.is_empty())
        .map(ToString::to_string)
        .or_else(|| endpoint.models.first().map(|model| model.id.clone()))
        .ok_or_else(|| "Model is required.".to_string())?;
    Ok(catalog::canonical_model_id(endpoint, &requested).unwrap_or(requested))
}

fn test_payload(api_type: &str, model: &str) -> Result<Value, String> {
    let prompt = "hello. Reply with only: world";
    match api_type {
        "anthropic" => Ok(json!({
            "model": model,
            "max_tokens": 8,
            "messages": [{ "role": "user", "content": prompt }],
            "stream": false
        })),
        "openai-chat" => Ok(json!({
            "model": model,
            "messages": [{ "role": "user", "content": prompt }],
            "max_tokens": 8,
            "stream": false
        })),
        "openai-responses" => Ok(json!({
            "model": model,
            "input": prompt,
            "max_output_tokens": 8,
            "stream": false
        })),
        "gemini" => Ok(json!({
            "contents": [{
                "role": "user",
                "parts": [{ "text": prompt }]
            }],
            "generationConfig": {
                "maxOutputTokens": 8
            },
            "__va_model": model
        })),
        _ => Err(format!("unsupported API kind '{api_type}'")),
    }
}

fn test_url(
    api_type: &str,
    base_url: &str,
    endpoint: &EndpointDef,
    model: &str,
) -> Result<String, String> {
    match api_type {
        "anthropic" => Ok(join_protocol_endpoint(
            base_url,
            "messages",
            endpoint.append_v1_path,
        )),
        "openai-chat" => Ok(join_protocol_endpoint(
            base_url,
            "chat/completions",
            endpoint.append_v1_path,
        )),
        "openai-responses" => Ok(join_protocol_endpoint(
            base_url,
            "responses",
            endpoint.append_v1_path,
        )),
        "gemini" => Ok(join_gemini_endpoint(base_url, model)),
        _ => Err(format!("unsupported API kind '{api_type}'")),
    }
}

fn apply_auth(
    request: reqwest::RequestBuilder,
    api_type: &str,
    endpoint: &EndpointDef,
    api_key: &str,
) -> reqwest::RequestBuilder {
    let mut request = if is_openai_family(api_type) || endpoint.auth_header {
        request.header(AUTHORIZATION, format!("Bearer {api_key}"))
    } else if api_type == "gemini" {
        request.header("x-goog-api-key", api_key)
    } else {
        request.header("x-api-key", api_key)
    };

    if api_type == "anthropic" {
        request = request.header("anthropic-version", "2023-06-01");
    }
    request
}

fn is_openai_family(api_type: &str) -> bool {
    matches!(api_type, "openai-chat" | "openai-responses")
}

fn join_gemini_endpoint(base_url: &str, model: &str) -> String {
    let base_url = if gemini_base_url_has_version(base_url) {
        base_url.to_string()
    } else {
        format!("{base_url}/v1beta")
    };
    format!("{base_url}/models/{model}:generateContent")
}

fn gemini_base_url_has_version(base_url: &str) -> bool {
    let last = base_url.rsplit('/').next().unwrap_or_default();
    matches!(last, "v1" | "v1beta" | "v1alpha")
}

fn test_http_client(profile: &ProfileDef) -> Result<reqwest::Client, String> {
    let mut builder = reqwest::Client::builder()
        .timeout(Duration::from_secs(45))
        .redirect(reqwest::redirect::Policy::none());
    if profile.use_settings_proxy {
        let cfg = config::ensure_loaded();
        if cfg.proxy.enabled {
            let proxy_url = cfg.proxy.http_proxy.as_deref().ok_or_else(|| {
                "Settings proxy is enabled for this profile but HTTP proxy URL is empty".to_string()
            })?;
            let mut proxy_rule = reqwest::Proxy::all(proxy_url)
                .map_err(|error| format!("invalid Settings proxy HTTP URL: {error}"))?;
            if let Some(no_proxy) = cfg.proxy.no_proxy.as_deref() {
                proxy_rule = proxy_rule.no_proxy(reqwest::NoProxy::from_string(no_proxy));
            }
            builder = builder.proxy(proxy_rule);
        }
    }
    builder
        .build()
        .map_err(|error| format!("failed to build profile test HTTP client: {error}"))
}

fn compact_error_body(body: &str) -> String {
    let compact = body.split_whitespace().collect::<Vec<_>>().join(" ");
    if compact.chars().count() <= 360 {
        return compact;
    }
    compact.chars().take(360).collect::<String>() + "..."
}
