//! Pi profile launch rendering.
//!
//! Pi accepts custom providers through extensions. VibeAround writes a
//! profile-local extension per launch so provider/base URL choices stay scoped
//! to the selected profile instead of mutating the user's global Pi config.

use std::collections::BTreeMap;

use anyhow::{bail, Context};
use serde_json::{json, Map, Value};

use super::bridge_url;
use super::catalog::ContentCapabilities;
use super::render::{validate_rel_path, RenderedProfile, RenderedSettingsFile};
use crate::config;

pub(super) struct PiProviderLaunchConfig<'a> {
    pub profile_id: &'a str,
    pub provider_id: String,
    pub provider_label: &'a str,
    pub api_type: &'a str,
    pub api_key: String,
    pub base_url: String,
    pub model: String,
    pub model_context_window: Option<u64>,
    pub model_capabilities: ContentCapabilities,
    pub reasoning: bool,
    pub headers: BTreeMap<String, String>,
    pub auth_header: bool,
    pub file_stem: String,
}

pub(super) fn render_pi_provider(
    config: PiProviderLaunchConfig<'_>,
) -> anyhow::Result<RenderedProfile> {
    if config.api_key.trim().is_empty() {
        bail!(
            "profile '{}' has no api_key credential for Pi launch",
            config.profile_id
        );
    }
    if config.base_url.trim().is_empty() {
        bail!(
            "profile '{}' has no base URL configured for Pi launch",
            config.profile_id
        );
    }
    if config.model.trim().is_empty() {
        bail!(
            "profile '{}' has no model configured for Pi launch",
            config.profile_id
        );
    }

    let rel_path = format!("pi-provider-{}.mjs", slug(&config.file_stem));
    validate_rel_path(&rel_path)?;
    let extension_path = super::runtime::profile_state_dir(config.profile_id).join(&rel_path);
    let extension_path = extension_path.to_string_lossy().into_owned();

    Ok(RenderedProfile {
        env: vec![("VIBEAROUND_PI_API_KEY".to_string(), config.api_key.clone())],
        settings_files: vec![RenderedSettingsFile {
            rel_path,
            contents: pi_extension_contents(&config)?,
        }],
        command_args: vec![
            "--extension".to_string(),
            extension_path,
            "--provider".to_string(),
            config.provider_id,
            "--model".to_string(),
            config.model,
        ],
        config_env: None,
    })
}

/// Render the same provider definition Pi would receive, but as the flat
/// model config VibeAround Agent reads from its environment.
pub(super) fn render_va_agent_provider(
    config: PiProviderLaunchConfig<'_>,
) -> anyhow::Result<RenderedProfile> {
    if config.api_key.trim().is_empty() {
        bail!(
            "profile '{}' has no api_key credential for VibeAround Agent launch",
            config.profile_id
        );
    }
    if config.base_url.trim().is_empty() {
        bail!(
            "profile '{}' has no base URL configured for VibeAround Agent launch",
            config.profile_id
        );
    }
    if config.model.trim().is_empty() {
        bail!(
            "profile '{}' has no model configured for VibeAround Agent launch",
            config.profile_id
        );
    }

    let context_window = config.model_context_window.unwrap_or(128_000);
    let mut model_config = Map::new();
    model_config.insert("provider".to_string(), json!(config.provider_id));
    model_config.insert("api".to_string(), json!(pi_api_for(config.api_type)?));
    model_config.insert("baseUrl".to_string(), json!(config.base_url));
    model_config.insert("model".to_string(), json!(config.model));
    model_config.insert("contextWindow".to_string(), json!(context_window));
    model_config.insert("maxTokens".to_string(), json!(context_window.min(16_384)));
    // Pi's provider registration types accept text and image inputs.
    let inputs: Vec<&str> = model_inputs(&config.model_capabilities)
        .into_iter()
        .filter(|input| *input != "file")
        .collect();
    model_config.insert("input".to_string(), json!(inputs));
    if config.reasoning {
        model_config.insert("reasoning".to_string(), json!(true));
    }
    if !config.headers.is_empty() {
        model_config.insert("headers".to_string(), json!(config.headers));
    }
    if config.auth_header {
        model_config.insert("authHeader".to_string(), json!(true));
    }
    let model_config = serde_json::to_string(&Value::Object(model_config))
        .context("serialize VibeAround Agent model config")?;

    Ok(RenderedProfile {
        env: vec![
            ("VIBEAROUND_MODEL_CONFIG".to_string(), model_config),
            ("VIBEAROUND_MODEL_API_KEY".to_string(), config.api_key),
        ],
        settings_files: Vec::new(),
        command_args: Vec::new(),
        config_env: None,
    })
}

pub(super) fn provider_id(profile_id: &str, api_type: &str) -> String {
    format!("vibearound-{}-{}", slug(profile_id), slug(api_type))
}

pub(super) fn bridge_base_url(
    profile_id: &str,
    scope: &str,
    target_api_type: &str,
    client_api_type: &str,
) -> String {
    let suffix = if client_api_type == "anthropic" {
        ""
    } else {
        "/v1"
    };
    bridge_url::local_api_base_with_suffix(
        config::DEFAULT_PORT,
        profile_id,
        scope,
        target_api_type,
        suffix,
    )
}

fn pi_api_for(api_type: &str) -> anyhow::Result<&'static str> {
    match api_type {
        "anthropic" => Ok("anthropic-messages"),
        "openai-chat" => Ok("openai-completions"),
        "openai-responses" => Ok("openai-responses"),
        other => bail!("Pi launch is not wired for api kind '{}'", other),
    }
}

fn pi_extension_contents(config: &PiProviderLaunchConfig<'_>) -> anyhow::Result<String> {
    let context_window = config.model_context_window.unwrap_or(128_000);
    let mut model = Map::new();
    model.insert("id".to_string(), json!(config.model));
    model.insert("name".to_string(), json!(config.model));
    model.insert(
        "input".to_string(),
        json!(model_inputs(&config.model_capabilities)),
    );
    model.insert("contextWindow".to_string(), json!(context_window));
    model.insert("maxTokens".to_string(), json!(context_window.min(16_384)));
    model.insert(
        "cost".to_string(),
        json!({
            "input": 0,
            "output": 0,
            "cacheRead": 0,
            "cacheWrite": 0
        }),
    );
    if config.reasoning {
        model.insert("reasoning".to_string(), json!(true));
    }

    let mut provider = Map::new();
    provider.insert("name".to_string(), json!(config.provider_label));
    provider.insert("baseUrl".to_string(), json!(config.base_url));
    // Pi resolves config values via $VAR/${VAR} templates; a bare string is
    // treated as a literal key, not an env var name.
    provider.insert("apiKey".to_string(), json!("$VIBEAROUND_PI_API_KEY"));
    provider.insert("api".to_string(), json!(pi_api_for(config.api_type)?));
    provider.insert(
        "models".to_string(),
        Value::Array(vec![Value::Object(model)]),
    );
    if !config.headers.is_empty() {
        provider.insert("headers".to_string(), json!(config.headers));
    }
    if config.auth_header {
        provider.insert("authHeader".to_string(), json!(true));
    }

    let provider_id = serde_json::to_string(&config.provider_id)
        .context("serialize Pi provider id for extension")?;
    let provider_json = serde_json::to_string_pretty(&Value::Object(provider))
        .context("serialize Pi provider config for extension")?;
    Ok(format!(
        "export default function (pi) {{\n  pi.registerProvider({provider_id}, {provider_json});\n}}\n"
    ))
}

fn model_inputs(capabilities: &ContentCapabilities) -> Vec<&'static str> {
    let mut inputs = vec!["text"];
    if capabilities.image_input {
        inputs.push("image");
    }
    if capabilities.file_input {
        inputs.push("file");
    }
    inputs
}

fn slug(input: &str) -> String {
    let mut out = String::with_capacity(input.len().min(96));
    for ch in input.chars().take(96) {
        if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
            out.push(ch);
        } else {
            out.push('_');
        }
    }
    if out.is_empty() {
        out.push_str("profile");
    }
    out
}
