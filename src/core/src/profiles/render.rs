//! Render orchestrator — resolve a profile against a provider API kind and
//! CLI launch target, then produce the env vars + optional settings files
//! the launcher will hand to the spawned terminal.
//!
//! The mustache-lite engine is intentionally tiny: it supports `{{name}}`
//! substitution against a flat string context and nothing else (no pipes,
//! no conditionals, no escaping). Catalog templates that need richer logic
//! should pre-shape the data instead. Empty resolved env values are
//! dropped so a missing-but-not-required field doesn't end up exporting
//! `KEY=""`.

use std::collections::BTreeMap;

use anyhow::{anyhow, bail};

use super::catalog::{
    self, AuthModeDef, ContentCapabilities, EndpointDef, ProviderCatalog, RenderRules,
    SettingsFileTemplate,
};
use super::codex_metadata::{self, CodexModelCatalogSpec};
use super::schema::{ApiTypeOverrides, AuthMode, ProfileDef};

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct RenderedProfile {
    pub env: Vec<(String, String)>,
    pub settings_files: Vec<RenderedSettingsFile>,
    pub command_args: Vec<String>,
    /// Which env var should point at profile-local rendered config once
    /// the launcher materializes any settings files. We avoid overriding
    /// agent home dirs such as CODEX_HOME or CLAUDE_CONFIG_DIR so those CLIs
    /// keep loading the user's own sessions, plugins, and skills.
    pub config_env: Option<ConfigEnvTarget>,
}

#[derive(Debug, Clone)]
pub struct RenderedSettingsFile {
    pub rel_path: String,
    pub contents: String,
}

#[derive(Debug, Clone)]
pub enum ConfigEnvTarget {
    Directory(&'static str),
    File {
        env: &'static str,
        rel_path: &'static str,
    },
}

// ---------------------------------------------------------------------------
// Public entry
// ---------------------------------------------------------------------------

pub fn render(
    profile: &ProfileDef,
    api_type: &str,
    launch_target: &str,
    catalog: &ProviderCatalog,
) -> anyhow::Result<RenderedProfile> {
    let endpoint = pick_endpoint(profile, catalog, api_type)?;
    let auth = pick_auth_mode(endpoint, &profile.auth_mode)?;
    let context = build_context(profile, api_type, endpoint, catalog);
    if launch_target == "pi" {
        return render_pi_profile(profile, api_type, endpoint, catalog, &context);
    }

    let opencode_rules;
    let render_rules = if launch_target == "opencode" {
        opencode_rules = opencode_render_rules(api_type)?;
        &opencode_rules
    } else {
        auth.render
            .as_ref()
            .ok_or_else(|| {
                anyhow!(
                    "auth mode '{}' has no render rules (only oauth flows skip rendering, which v1 doesn't expose)",
                    auth.mode
                )
            })?
    };

    // Env vars — drop entries whose substituted value is empty so we don't
    // end up exporting blank keys (e.g. `ANTHROPIC_MODEL=""` when the user
    // didn't pick a model override).
    let mut env: Vec<(String, String)> = Vec::new();
    for (k, tmpl) in &render_rules.env {
        if !is_valid_env_key(k) {
            bail!("invalid env key in render rules: '{}'", k);
        }
        let v = substitute(tmpl, &context);
        if !v.is_empty() {
            env.push((k.clone(), v));
        }
    }
    if launch_target == "claude" && api_type == "anthropic" {
        normalize_claude_env(&mut env, &context);
    }

    // Settings files — substitute against the same context, validate each path.
    let mut settings_files: Vec<RenderedSettingsFile> = Vec::new();
    for sf in &render_rules.settings_files {
        validate_rel_path(&sf.rel_path)?;
        settings_files.push(RenderedSettingsFile {
            rel_path: sf.rel_path.clone(),
            contents: substitute(&sf.template, &context),
        });
    }

    let mut command_args = command_args_for(launch_target, &context);
    if let Some(metadata) = selected_model_metadata(&context, endpoint) {
        add_codex_model_catalog(
            profile,
            launch_target,
            &context,
            &metadata,
            &mut settings_files,
            &mut command_args,
        )?;
    }

    let config_env = config_env_for_rendered_files(launch_target, &settings_files);

    Ok(RenderedProfile {
        env,
        settings_files,
        command_args,
        config_env,
    })
}

// ---------------------------------------------------------------------------
// Lookups
// ---------------------------------------------------------------------------

fn pick_endpoint<'a>(
    profile: &ProfileDef,
    catalog: &'a ProviderCatalog,
    api_type: &str,
) -> anyhow::Result<&'a EndpointDef> {
    let endpoint_id = profile
        .overrides
        .get(api_type)
        .and_then(|overrides| overrides.endpoint_id.as_deref());
    catalog::find_endpoint(catalog, api_type, endpoint_id).ok_or_else(|| {
        let suffix = endpoint_id
            .map(|id| format!(" endpoint_id '{id}'"))
            .unwrap_or_default();
        anyhow!(
            "provider '{}' has no endpoint for api_type '{}'{}",
            catalog.id,
            api_type,
            suffix
        )
    })
}

fn pick_auth_mode<'a>(
    endpoint: &'a EndpointDef,
    auth_mode: &AuthMode,
) -> anyhow::Result<&'a AuthModeDef> {
    let needle = match auth_mode {
        AuthMode::ApiKey => "api_key",
        AuthMode::OauthViaCli => "oauth_via_cli",
    };
    endpoint
        .auth_modes
        .iter()
        .find(|a| a.mode == needle)
        .ok_or_else(|| {
            anyhow!(
                "endpoint '{}' has no auth mode '{}'",
                endpoint.api_type,
                needle
            )
        })
}

fn config_env_for(launch_target: &str) -> Option<ConfigEnvTarget> {
    match launch_target {
        "opencode" => Some(ConfigEnvTarget::File {
            env: "OPENCODE_CONFIG",
            rel_path: "opencode.json",
        }),
        _ => None,
    }
}

fn config_env_for_rendered_files(
    launch_target: &str,
    settings_files: &[RenderedSettingsFile],
) -> Option<ConfigEnvTarget> {
    if settings_files
        .iter()
        .all(|settings_file| settings_file.rel_path.starts_with("codex-model-catalog-"))
    {
        return None;
    }
    config_env_for(launch_target)
}

fn opencode_render_rules(api_type: &str) -> anyhow::Result<RenderRules> {
    match api_type {
        "openai-responses" => Ok(RenderRules {
            env: [(
                "VIBEAROUND_OPENCODE_API_KEY".to_string(),
                "{{api_key}}".to_string(),
            )]
            .into_iter()
            .collect(),
            settings_files: vec![SettingsFileTemplate {
                rel_path: "opencode.json".to_string(),
                template: "{\n  \"$schema\": \"https://opencode.ai/config.json\",\n  \"model\": \"{{provider_id}}/{{model|json}}\",\n  \"provider\": {\n    \"{{provider_id}}\": {\n      \"npm\": \"@ai-sdk/openai\",\n      \"name\": \"{{provider_label|json}}\",\n      \"options\": {\n        \"baseURL\": \"{{base_url|json}}\",\n        \"apiKey\": \"{env:VIBEAROUND_OPENCODE_API_KEY}\",\n        \"setCacheKey\": true\n      },\n      \"models\": {\n        \"{{model|json}}\": { \"name\": \"{{model|json}}\" }\n      }\n    }\n  }\n}\n".to_string(),
            }],
        }),
        "openai-chat" => Ok(RenderRules {
            env: [(
                "VIBEAROUND_OPENCODE_API_KEY".to_string(),
                "{{api_key}}".to_string(),
            )]
            .into_iter()
            .collect(),
            settings_files: vec![SettingsFileTemplate {
                rel_path: "opencode.json".to_string(),
                template: "{\n  \"$schema\": \"https://opencode.ai/config.json\",\n  \"model\": \"{{provider_id}}/{{model|json}}\",\n  \"provider\": {\n    \"{{provider_id}}\": {\n      \"npm\": \"@ai-sdk/openai-compatible\",\n      \"name\": \"{{provider_label|json}}\",\n      \"options\": {\n        \"baseURL\": \"{{base_url|json}}\",\n        \"apiKey\": \"{env:VIBEAROUND_OPENCODE_API_KEY}\",\n        \"setCacheKey\": true\n      },\n      \"models\": {\n        \"{{model|json}}\": { \"name\": \"{{model|json}}\" }\n      }\n    }\n  }\n}\n".to_string(),
            }],
        }),
        "anthropic" => Ok(RenderRules {
            env: [(
                "VIBEAROUND_OPENCODE_API_KEY".to_string(),
                "{{api_key}}".to_string(),
            )]
            .into_iter()
            .collect(),
            settings_files: vec![SettingsFileTemplate {
                rel_path: "opencode.json".to_string(),
                template: "{\n  \"$schema\": \"https://opencode.ai/config.json\",\n  \"model\": \"{{provider_id}}/{{model|json}}\",\n  \"provider\": {\n    \"{{provider_id}}\": {\n      \"npm\": \"@ai-sdk/anthropic\",\n      \"name\": \"{{provider_label|json}}\",\n      \"options\": {\n        \"baseURL\": \"{{base_url|json}}\",\n        \"apiKey\": \"{env:VIBEAROUND_OPENCODE_API_KEY}\"\n      },\n      \"models\": {\n        \"{{model|json}}\": { \"name\": \"{{model|json}}\" }\n      }\n    }\n  }\n}\n".to_string(),
            }],
        }),
        other => bail!("opencode launch is not wired for api kind '{}'", other),
    }
}

fn render_pi_profile(
    profile: &ProfileDef,
    api_type: &str,
    endpoint: &EndpointDef,
    catalog: &ProviderCatalog,
    context: &BTreeMap<String, String>,
) -> anyhow::Result<RenderedProfile> {
    let model = context
        .get("model")
        .map(String::as_str)
        .filter(|value| !value.is_empty());
    let model_def = model.and_then(|model| catalog::find_model(endpoint, model));
    let model_capabilities = model_def
        .map(|model_def| endpoint.capabilities.content.merge(&model_def.capabilities))
        .unwrap_or_else(|| endpoint.capabilities.content.clone());
    let provider_id = super::pi_launch::provider_id(&profile.id, api_type);
    super::pi_launch::render_pi_provider(super::pi_launch::PiProviderLaunchConfig {
        profile_id: &profile.id,
        provider_id: provider_id.clone(),
        provider_label: &catalog.label,
        api_type,
        api_key: context.get("api_key").cloned().unwrap_or_default(),
        base_url: context.get("base_url").cloned().unwrap_or_default(),
        model: context.get("model").cloned().unwrap_or_default(),
        model_context_window: model_def.and_then(|model_def| model_def.context_window),
        model_capabilities,
        reasoning: endpoint.capabilities.reasoning_effort,
        headers: endpoint.headers.clone(),
        auth_header: endpoint.auth_header,
        file_stem: provider_id,
    })
}

fn command_args_for(launch_target: &str, ctx: &BTreeMap<String, String>) -> Vec<String> {
    if launch_target != "codex" {
        return Vec::new();
    }

    let mut args = Vec::new();
    let mut push_config = |key: &str, value: String| {
        args.push("-c".to_string());
        args.push(format!("{key}={value}"));
    };
    if let Some(model) = ctx.get("model").filter(|v| !v.is_empty()) {
        push_config("model", toml_string(model));
    }
    if let Some(provider_id) = ctx.get("provider_id").filter(|v| !v.is_empty()) {
        push_config("model_provider", toml_string(provider_id));
    }
    if let Some(reasoning_effort) = ctx.get("reasoning_effort").filter(|v| !v.is_empty()) {
        push_config("model_reasoning_effort", toml_string(reasoning_effort));
    }
    if let Some(context_window) = ctx.get("model_context_window").filter(|v| !v.is_empty()) {
        push_config("model_context_window", context_window.clone());
    }

    let Some(provider_id) = ctx.get("provider_id").filter(|v| !v.is_empty()) else {
        return args;
    };
    let provider_key = format!("model_providers.{provider_id}");
    if let Some(provider_label) = ctx.get("provider_label").filter(|v| !v.is_empty()) {
        push_config(&format!("{provider_key}.name"), toml_string(provider_label));
    }
    if let Some(base_url) = ctx.get("base_url").filter(|v| !v.is_empty()) {
        push_config(&format!("{provider_key}.base_url"), toml_string(base_url));
    }
    if let Some(api_type) = ctx.get("api_type").filter(|v| !v.is_empty()) {
        let wire_api = if api_type == "openai-chat" {
            "chat"
        } else {
            "responses"
        };
        push_config(&format!("{provider_key}.wire_api"), toml_string(wire_api));
    }
    push_config(
        &format!("{provider_key}.env_key"),
        toml_string(codex_provider_env_key(provider_id)),
    );
    args
}

#[derive(Debug)]
struct SelectedModelMetadata {
    context_window: u64,
    capabilities: ContentCapabilities,
}

fn selected_model_metadata(
    ctx: &BTreeMap<String, String>,
    endpoint: &EndpointDef,
) -> Option<SelectedModelMetadata> {
    let model = ctx.get("model").filter(|value| !value.is_empty())?;
    let model_def = catalog::find_model(endpoint, model)?;
    Some(SelectedModelMetadata {
        context_window: model_def.context_window?,
        capabilities: endpoint.capabilities.content.merge(&model_def.capabilities),
    })
}

fn add_codex_model_catalog(
    profile: &ProfileDef,
    launch_target: &str,
    ctx: &BTreeMap<String, String>,
    metadata: &SelectedModelMetadata,
    settings_files: &mut Vec<RenderedSettingsFile>,
    command_args: &mut Vec<String>,
) -> anyhow::Result<()> {
    if launch_target != "codex" {
        return Ok(());
    }
    let Some(model) = ctx.get("model").filter(|value| !value.is_empty()) else {
        return Ok(());
    };
    let Some(provider_label) = ctx.get("provider_label").filter(|value| !value.is_empty()) else {
        return Ok(());
    };
    let spec = CodexModelCatalogSpec {
        model,
        provider_label,
        context_window: Some(metadata.context_window),
        capabilities: &metadata.capabilities,
    };
    let Some(model_catalog_json) = codex_metadata::build_model_catalog_json(&[spec]) else {
        return Ok(());
    };

    let rel_path = codex_model_catalog_rel_path(model);
    validate_rel_path(&rel_path)?;
    let catalog_path = super::runtime::profile_state_dir(&profile.id).join(&rel_path);
    let catalog_path = catalog_path.to_string_lossy();
    command_args.push("-c".to_string());
    command_args.push(format!(
        "model_catalog_json={}",
        toml_string(catalog_path.as_ref())
    ));
    settings_files.push(RenderedSettingsFile {
        rel_path,
        contents: model_catalog_json,
    });

    Ok(())
}

fn codex_model_catalog_rel_path(model: &str) -> String {
    let mut slug = String::with_capacity(model.len());
    for ch in model.chars().take(96) {
        if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
            slug.push(ch);
        } else {
            slug.push('_');
        }
    }
    if slug.is_empty() {
        slug.push_str("model");
    }
    format!("codex-model-catalog-{slug}.json")
}

fn normalize_claude_env(env: &mut Vec<(String, String)>, ctx: &BTreeMap<String, String>) {
    let api_key = first_env_value(env, &["ANTHROPIC_API_KEY", "ANTHROPIC_AUTH_TOKEN"])
        .or_else(|| ctx.get("api_key").cloned())
        .unwrap_or_default();
    let base_url = first_env_value(env, &["ANTHROPIC_BASE_URL"])
        .or_else(|| ctx.get("base_url").cloned())
        .unwrap_or_default();
    let model = first_env_value(env, &["ANTHROPIC_MODEL"])
        .or_else(|| ctx.get("model").cloned())
        .unwrap_or_default();
    let auto_compact_window = ctx
        .get("model_context_window")
        .filter(|value| !value.is_empty())
        .cloned();

    env.retain(|(key, _)| !is_standardized_claude_env_key(key));
    push_env_if_nonempty(env, "ANTHROPIC_API_KEY", api_key.clone());
    push_env_if_nonempty(env, "ANTHROPIC_AUTH_TOKEN", api_key);
    push_env_if_nonempty(env, "ANTHROPIC_BASE_URL", base_url);
    push_env_if_nonempty(env, "ANTHROPIC_MODEL", model);
    if let Some(auto_compact_window) = auto_compact_window {
        push_env_if_nonempty(env, "CLAUDE_CODE_AUTO_COMPACT_WINDOW", auto_compact_window);
    }
}

fn first_env_value(env: &[(String, String)], keys: &[&str]) -> Option<String> {
    keys.iter().find_map(|key| {
        env.iter()
            .find(|(candidate, value)| candidate == key && !value.is_empty())
            .map(|(_, value)| value.clone())
    })
}

fn push_env_if_nonempty(env: &mut Vec<(String, String)>, key: &str, value: String) {
    if !value.is_empty() {
        env.push((key.to_string(), value));
    }
}

fn is_standardized_claude_env_key(key: &str) -> bool {
    matches!(
        key,
        "ANTHROPIC_API_KEY"
            | "ANTHROPIC_AUTH_TOKEN"
            | "ANTHROPIC_BASE_URL"
            | "ANTHROPIC_MODEL"
            | "ANTHROPIC_DEFAULT_OPUS_MODEL"
            | "ANTHROPIC_DEFAULT_SONNET_MODEL"
            | "ANTHROPIC_DEFAULT_HAIKU_MODEL"
            | "CLAUDE_CODE_AUTO_COMPACT_WINDOW"
            | "CLAUDE_CODE_SUBAGENT_MODEL"
            | "CLAUDE_CODE_EFFORT_LEVEL"
    )
}

fn codex_provider_env_key(provider_id: &str) -> &'static str {
    match provider_id {
        "azure" => "AZURE_OPENAI_API_KEY",
        _ => "OPENAI_API_KEY",
    }
}

/// Wraps a value as a TOML literal string (`'...'`).  Literal strings have no
/// escape sequences so they never contain `"` or `\` delimiters.  This is
/// important on Windows where PowerShell 5.1 mangles native-command arguments
/// that contain `"` characters.
///
/// Falls back to a basic (double-quoted) string when the value contains `'`,
/// which is the only character forbidden inside TOML literal strings.
fn toml_string(s: &str) -> String {
    if s.contains('\'') {
        // Fallback: TOML basic string with standard escaping.
        let mut out = String::with_capacity(s.len() + 2);
        out.push('"');
        for ch in s.chars() {
            match ch {
                '"' => out.push_str("\\\""),
                '\\' => out.push_str("\\\\"),
                '\n' => out.push_str("\\n"),
                '\r' => out.push_str("\\r"),
                '\t' => out.push_str("\\t"),
                other => out.push(other),
            }
        }
        out.push('"');
        out
    } else {
        let mut out = String::with_capacity(s.len() + 2);
        out.push('\'');
        out.push_str(s);
        out.push('\'');
        out
    }
}

// ---------------------------------------------------------------------------
// Context builder
// ---------------------------------------------------------------------------

fn build_context(
    profile: &ProfileDef,
    api_type: &str,
    endpoint: &EndpointDef,
    catalog: &ProviderCatalog,
) -> BTreeMap<String, String> {
    let overrides = profile
        .overrides
        .get(api_type)
        .cloned()
        .unwrap_or_else(ApiTypeOverrides::default);

    let mut ctx: BTreeMap<String, String> = BTreeMap::new();
    ctx.insert("provider_id".to_string(), profile.provider.clone());
    ctx.insert("provider_label".to_string(), catalog.label.clone());
    ctx.insert("api_type".to_string(), api_type.to_string());
    ctx.insert(
        "base_url".to_string(),
        overrides
            .base_url
            .unwrap_or_else(|| endpoint.default_base_url.clone()),
    );
    let requested_model = overrides
        .model
        .filter(|model| !model.trim().is_empty())
        .or_else(|| endpoint.models.first().map(|model| model.id.clone()))
        .unwrap_or_default();
    let model_def = catalog::find_model(endpoint, &requested_model);
    let model = model_def
        .map(|model_def| model_def.id.clone())
        .unwrap_or(requested_model);
    if let Some(context_window) = model_def.and_then(|model_def| model_def.context_window) {
        ctx.insert(
            "model_context_window".to_string(),
            context_window.to_string(),
        );
    }
    ctx.insert("model".to_string(), model);
    ctx.insert(
        "reasoning_effort".to_string(),
        overrides
            .reasoning_effort
            .unwrap_or_else(|| "medium".to_string()),
    );

    // Credentials are flattened in last so a (hypothetical) catalog field
    // named "model" doesn't shadow the explicitly-resolved override above.
    // In practice fields are domain-specific (e.g. `api_key`); the ordering
    // is defensive.
    for (k, v) in &profile.credentials {
        if k == "base_url" || k == "model" {
            continue;
        }
        ctx.insert(k.clone(), v.clone());
    }
    ctx
}

// ---------------------------------------------------------------------------
// Mustache-lite
// ---------------------------------------------------------------------------

fn substitute(template: &str, ctx: &BTreeMap<String, String>) -> String {
    let bytes = template.as_bytes();
    let mut out = String::with_capacity(template.len());
    let mut i = 0;
    while i < bytes.len() {
        if i + 1 < bytes.len() && bytes[i] == b'{' && bytes[i + 1] == b'{' {
            // Find the closing `}}`.
            let after_open = i + 2;
            if let Some(close_rel) = template[after_open..].find("}}") {
                let raw = template[after_open..after_open + close_rel].trim();
                // `{{name|filter}}` runs the named value through a filter
                // before substitution. Used to JSON-escape secrets that
                // get spliced into auth.json templates — without this an
                // api_key containing `"` or `\` would corrupt the file.
                let (name, filter) = match raw.split_once('|') {
                    Some((n, f)) => (n.trim(), Some(f.trim())),
                    None => (raw, None),
                };
                if let Some(v) = ctx.get(name) {
                    let rendered = match filter {
                        Some("json") => json_escape(v),
                        Some(other) => {
                            tracing::warn!(
                                "[profiles] unknown template filter '{}' on '{{{{ {} | {} }}}}'; \
                                 substituting raw value",
                                other,
                                name,
                                other
                            );
                            v.clone()
                        }
                        None => v.clone(),
                    };
                    out.push_str(&rendered);
                }
                i = after_open + close_rel + 2;
                continue;
            }
            // No closing — treat as literal and bail out of the scan.
            out.push_str(&template[i..]);
            break;
        }
        out.push(bytes[i] as char);
        i += 1;
    }
    out
}

/// JSON-escape the *contents* of a string literal — the caller is
/// responsible for the surrounding `"`. This intentionally does NOT add
/// the outer quotes so catalog templates can keep the JSON shape
/// human-readable (`"OPENAI_API_KEY": "{{api_key|json}}"`).
fn json_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{08}' => out.push_str("\\b"),
            '\u{0c}' => out.push_str("\\f"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Validators
// ---------------------------------------------------------------------------

pub fn validate_rel_path(rel: &str) -> anyhow::Result<()> {
    if rel.is_empty() {
        bail!("rel_path is empty");
    }
    if rel.starts_with('/') || rel.starts_with('\\') {
        bail!("rel_path must not be absolute: '{}'", rel);
    }
    for component in rel.split(['/', '\\']) {
        if component == ".." {
            bail!("rel_path must not contain '..': '{}'", rel);
        }
    }
    Ok(())
}

fn is_valid_env_key(key: &str) -> bool {
    !key.is_empty()
        && key
            .chars()
            .next()
            .map(|c| c.is_ascii_alphabetic() || c == '_')
            .unwrap_or(false)
        && key.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use crate::profiles::schema::{ApiTypeOverrides, AuthMode, ProfileDef};
    use serde_json::Value;

    use super::*;

    #[test]
    fn claude_launch_env_has_same_shape_for_anthropic_providers() {
        for profile in [
            anthropic_profile("deepseek", None, "deepseek-v4-pro"),
            anthropic_profile("dashscope", Some("coding-plan"), "qwen3.6-plus"),
            anthropic_profile("kimi", None, "kimi-for-coding"),
        ] {
            let provider = catalog::get(&profile.provider).expect("provider exists");
            let rendered =
                render(&profile, "anthropic", "claude", provider).expect("claude profile renders");
            let keys: Vec<_> = rendered.env.iter().map(|(key, _)| key.as_str()).collect();

            assert_eq!(
                keys,
                vec![
                    "ANTHROPIC_API_KEY",
                    "ANTHROPIC_AUTH_TOKEN",
                    "ANTHROPIC_BASE_URL",
                    "ANTHROPIC_MODEL",
                    "CLAUDE_CODE_AUTO_COMPACT_WINDOW",
                ]
            );
            assert_eq!(
                rendered
                    .env
                    .iter()
                    .find(|(key, _)| key == "ANTHROPIC_MODEL")
                    .map(|(_, value)| value.as_str()),
                Some(model_for(&profile))
            );
            assert_eq!(
                rendered
                    .env
                    .iter()
                    .find(|(key, _)| key == "CLAUDE_CODE_AUTO_COMPACT_WINDOW")
                    .map(|(_, value)| value.as_str()),
                Some(match profile.provider.as_str() {
                    "kimi" => "256000",
                    _ => "1000000",
                })
            );
        }
    }

    #[test]
    fn codex_launch_includes_model_catalog_for_context_window() {
        let profile = openai_responses_profile("xai", "grok-4.3");
        let provider = catalog::get(&profile.provider).expect("provider exists");

        let rendered =
            render(&profile, "openai-responses", "codex", provider).expect("codex profile renders");

        assert!(rendered
            .command_args
            .iter()
            .any(|arg| arg == "model='grok-4.3'"));
        assert!(rendered
            .command_args
            .iter()
            .any(|arg| arg == "model_context_window=1000000"));
        assert!(rendered
            .command_args
            .iter()
            .any(|arg| arg.starts_with("model_catalog_json='")));
        assert!(rendered.config_env.is_none());

        let catalog_file = rendered
            .settings_files
            .iter()
            .find(|settings_file| settings_file.rel_path.starts_with("codex-model-catalog-"))
            .expect("codex model catalog file");
        let catalog: Value =
            serde_json::from_str(&catalog_file.contents).expect("catalog json parses");
        let model = &catalog["models"][0];
        assert_eq!(model["slug"], "grok-4.3");
        assert_eq!(model["context_window"], 1_000_000);
        assert_eq!(model["max_context_window"], 1_000_000);
        assert_eq!(
            model["input_modalities"],
            serde_json::json!(["text", "image"])
        );
    }

    #[test]
    fn pi_launch_materializes_openai_chat_extension() {
        let profile = openai_chat_profile("dashscope", Some("coding-plan"), "qwen3.6-plus");
        let provider = catalog::get(&profile.provider).expect("provider exists");

        let rendered = render(&profile, "openai-chat", "pi", provider).expect("pi profile renders");

        assert!(rendered
            .env
            .contains(&("VIBEAROUND_PI_API_KEY".to_string(), "test-key".to_string())));
        assert!(rendered
            .command_args
            .windows(2)
            .any(|args| args[0] == "--provider"
                && args[1] == "vibearound-dashscope-test-openai-chat"));
        assert!(rendered
            .command_args
            .windows(2)
            .any(|args| args[0] == "--model" && args[1] == "qwen3.6-plus"));
        assert!(rendered.config_env.is_none());

        let extension = rendered
            .settings_files
            .iter()
            .find(|settings_file| settings_file.rel_path.ends_with(".mjs"))
            .expect("pi extension file");
        assert!(extension.contents.contains("pi.registerProvider"));
        assert!(extension
            .contents
            .contains("\"api\": \"openai-completions\""));
        assert!(extension
            .contents
            .contains("\"baseUrl\": \"https://coding-intl.dashscope.aliyuncs.com/v1\""));
        assert!(extension
            .contents
            .contains("\"apiKey\": \"VIBEAROUND_PI_API_KEY\""));
        assert!(extension
            .contents
            .contains("\"X-DashScope-AuthType\": \"openai\""));
        assert!(extension.contents.contains("\"contextWindow\": 1000000"));
        assert!(extension.contents.contains("\"maxTokens\": 16384"));
        assert!(extension.contents.contains("\"image\""));
    }

    #[test]
    fn pi_launch_preserves_anthropic_headers_and_auth_header() {
        let profile = anthropic_profile("dashscope", Some("coding-plan"), "qwen3.6-plus");
        let provider = catalog::get(&profile.provider).expect("provider exists");

        let rendered = render(&profile, "anthropic", "pi", provider).expect("pi profile renders");
        let extension = rendered
            .settings_files
            .iter()
            .find(|settings_file| settings_file.rel_path.ends_with(".mjs"))
            .expect("pi extension file");

        assert!(extension
            .contents
            .contains("\"api\": \"anthropic-messages\""));
        assert!(extension.contents.contains("\"authHeader\": true"));
        assert!(extension
            .contents
            .contains("\"User-Agent\": \"claude-code/0.1.0\""));
    }

    fn anthropic_profile(provider: &str, endpoint_id: Option<&str>, model: &str) -> ProfileDef {
        let mut credentials = BTreeMap::new();
        credentials.insert("api_key".to_string(), "test-key".to_string());

        let mut overrides = BTreeMap::new();
        overrides.insert(
            "anthropic".to_string(),
            ApiTypeOverrides {
                endpoint_id: endpoint_id.map(ToOwned::to_owned),
                base_url: None,
                model: Some(model.to_string()),
                reasoning_effort: Some("medium".to_string()),
                capabilities: None,
            },
        );

        ProfileDef {
            id: format!("{provider}-test"),
            label: format!("{provider} test"),
            provider: provider.to_string(),
            auth_mode: AuthMode::ApiKey,
            api_types: vec!["anthropic".to_string()],
            credentials,
            overrides,
            use_settings_proxy: false,
            provider_settings: Default::default(),
        }
    }

    fn openai_chat_profile(provider: &str, endpoint_id: Option<&str>, model: &str) -> ProfileDef {
        let mut credentials = BTreeMap::new();
        credentials.insert("api_key".to_string(), "test-key".to_string());

        let mut overrides = BTreeMap::new();
        overrides.insert(
            "openai-chat".to_string(),
            ApiTypeOverrides {
                endpoint_id: endpoint_id.map(ToOwned::to_owned),
                base_url: None,
                model: Some(model.to_string()),
                reasoning_effort: Some("medium".to_string()),
                capabilities: None,
            },
        );

        ProfileDef {
            id: format!("{provider}-test"),
            label: format!("{provider} test"),
            provider: provider.to_string(),
            auth_mode: AuthMode::ApiKey,
            api_types: vec!["openai-chat".to_string()],
            credentials,
            overrides,
            use_settings_proxy: false,
            provider_settings: Default::default(),
        }
    }

    fn openai_responses_profile(provider: &str, model: &str) -> ProfileDef {
        let mut credentials = BTreeMap::new();
        credentials.insert("api_key".to_string(), "test-key".to_string());

        let mut overrides = BTreeMap::new();
        overrides.insert(
            "openai-responses".to_string(),
            ApiTypeOverrides {
                endpoint_id: None,
                base_url: None,
                model: Some(model.to_string()),
                reasoning_effort: Some("medium".to_string()),
                capabilities: None,
            },
        );

        ProfileDef {
            id: format!("{provider}-test"),
            label: format!("{provider} test"),
            provider: provider.to_string(),
            auth_mode: AuthMode::ApiKey,
            api_types: vec!["openai-responses".to_string()],
            credentials,
            overrides,
            use_settings_proxy: false,
            provider_settings: Default::default(),
        }
    }

    fn model_for(profile: &ProfileDef) -> &str {
        profile
            .overrides
            .get("anthropic")
            .and_then(|overrides| overrides.model.as_deref())
            .expect("test profile has model")
    }
}
