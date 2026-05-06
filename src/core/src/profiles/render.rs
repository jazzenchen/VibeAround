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
    AuthModeDef, EndpointDef, ProviderCatalog, RenderRules, SettingsFileTemplate,
};
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
    let endpoint = pick_endpoint(catalog, api_type)?;
    let auth = pick_auth_mode(endpoint, &profile.auth_mode)?;
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

    let context = build_context(profile, api_type, endpoint, catalog);

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

    // Settings files — substitute against the same context, validate each path.
    let mut settings_files: Vec<RenderedSettingsFile> = Vec::new();
    for sf in &render_rules.settings_files {
        validate_rel_path(&sf.rel_path)?;
        settings_files.push(RenderedSettingsFile {
            rel_path: sf.rel_path.clone(),
            contents: substitute(&sf.template, &context),
        });
    }

    let config_env = if settings_files.is_empty() {
        None
    } else {
        config_env_for(launch_target)
    };

    Ok(RenderedProfile {
        env,
        settings_files,
        command_args: command_args_for(launch_target, &context),
        config_env,
    })
}

// ---------------------------------------------------------------------------
// Lookups
// ---------------------------------------------------------------------------

fn pick_endpoint<'a>(
    catalog: &'a ProviderCatalog,
    api_type: &str,
) -> anyhow::Result<&'a EndpointDef> {
    catalog
        .endpoints
        .iter()
        .find(|e| e.api_type == api_type)
        .ok_or_else(|| {
            anyhow!(
                "provider '{}' has no endpoint for api_type '{}'",
                catalog.id,
                api_type
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
            rel_path: ".config/opencode/opencode.json",
        }),
        _ => None,
    }
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
        other => bail!("opencode launch is not wired for api kind '{}'", other),
    }
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
    ctx.insert("model".to_string(), overrides.model.unwrap_or_default());
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
