//! Bridge profile rendering for route-aware launches.

use anyhow::{anyhow, bail};
use serde_json::{json, Map, Value};

use super::catalog;
use super::codex_metadata::{self, CodexModelCatalogSpec};
use super::connections::ProfileBridgeModelRoute;
use super::render::{ConfigEnvTarget, RenderedProfile, RenderedSettingsFile};
use super::schema::ProfileDef;
use crate::config;

pub(super) fn render_bridge_launch(
    profile: &ProfileDef,
    launch_target: &str,
    launch_id: &str,
    client_api_type: &str,
    target_api_type: &str,
    upstream_model: Option<&str>,
    fake_model_id: Option<&str>,
    bridge_models: &[ProfileBridgeModelRoute],
) -> anyhow::Result<RenderedProfile> {
    let mut settings = resolve_bridge_settings(
        profile,
        target_api_type,
        upstream_model,
        fake_model_id,
        bridge_models,
    )?;
    settings.scope = format!("{launch_target}-{client_api_type}");
    match launch_target {
        "claude" => Ok(render_claude_bridge_profile(profile, launch_id, settings)),
        "codex" => Ok(render_codex_bridge_profile(profile, launch_id, settings)),
        "gemini" => Ok(render_gemini_bridge_profile(profile, settings)),
        "opencode" => Ok(render_opencode_bridge_profile(
            profile,
            launch_id,
            client_api_type,
            settings,
        )),
        "pi" => render_pi_bridge_profile(profile, client_api_type, settings),
        other => bail!("bridge launch is not wired for '{}'", other),
    }
}

struct BridgeLaunchSettings {
    target_api_type: String,
    scope: String,
    provider_label: String,
    api_key: String,
    models: Vec<BridgeModelSettings>,
    reasoning_effort: String,
}

#[derive(Debug, Clone)]
struct BridgeModelSettings {
    agent_model: String,
    model_context_window: Option<u64>,
    model_capabilities: catalog::ContentCapabilities,
}

impl BridgeLaunchSettings {
    fn default_model(&self) -> &BridgeModelSettings {
        self.models
            .first()
            .expect("bridge settings must contain at least one model")
    }
}

fn resolve_bridge_settings(
    profile: &ProfileDef,
    target_api_type: &str,
    upstream_model: Option<&str>,
    fake_model_id: Option<&str>,
    bridge_models: &[ProfileBridgeModelRoute],
) -> anyhow::Result<BridgeLaunchSettings> {
    let provider = catalog::get(&profile.provider)
        .ok_or_else(|| anyhow!("unknown provider '{}'", profile.provider))?;
    if !profile
        .api_types
        .iter()
        .any(|api_type| api_type == target_api_type)
    {
        bail!(
            "profile '{}' does not expose bridge target '{}'",
            profile.id,
            target_api_type
        );
    }

    let endpoint_id = profile
        .overrides
        .get(target_api_type)
        .and_then(|overrides| overrides.endpoint_id.as_deref());
    let endpoint =
        catalog::find_endpoint(provider, target_api_type, endpoint_id).ok_or_else(|| {
            let suffix = endpoint_id
                .map(|id| format!(" endpoint_id '{id}'"))
                .unwrap_or_default();
            anyhow!(
                "provider '{}' does not expose bridge target '{}'{}",
                profile.provider,
                target_api_type,
                suffix
            )
        })?;
    let api_key = profile
        .credentials
        .get("api_key")
        .filter(|value| !value.is_empty())
        .ok_or_else(|| anyhow!("profile '{}' has no api_key credential", profile.id))?
        .clone();
    let profile_model = profile
        .overrides
        .get(target_api_type)
        .and_then(|overrides| overrides.model.clone())
        .or_else(|| endpoint.models.first().map(|model| model.id.clone()))
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            anyhow!(
                "profile '{}' has no model configured for bridge target '{}'",
                profile.id,
                target_api_type
            )
        })?;
    let requested_upstream_model = upstream_model
        .map(str::trim)
        .filter(|model| !model.is_empty())
        .map(ToOwned::to_owned)
        .unwrap_or(profile_model);
    let routes = if bridge_models.is_empty() {
        vec![ProfileBridgeModelRoute {
            upstream_model: requested_upstream_model.clone(),
            agent_model: fake_model_id
                .map(str::trim)
                .filter(|model| !model.is_empty())
                .map(ToOwned::to_owned)
                .unwrap_or_else(|| requested_upstream_model.clone()),
        }]
    } else {
        bridge_models.to_vec()
    };
    let models = routes
        .into_iter()
        .filter_map(|route| bridge_model_settings(endpoint, route))
        .collect::<Vec<_>>();
    if models.is_empty() {
        bail!(
            "profile '{}' has no models configured for bridge target '{}'",
            profile.id,
            target_api_type
        );
    }
    let reasoning_effort = profile
        .overrides
        .get(target_api_type)
        .and_then(|overrides| overrides.reasoning_effort.clone())
        .unwrap_or_else(|| "medium".to_string());

    Ok(BridgeLaunchSettings {
        target_api_type: target_api_type.to_string(),
        scope: String::new(),
        provider_label: provider.label.clone(),
        api_key,
        models,
        reasoning_effort,
    })
}

fn bridge_model_settings(
    endpoint: &catalog::EndpointDef,
    route: ProfileBridgeModelRoute,
) -> Option<BridgeModelSettings> {
    let upstream_model = route.upstream_model.trim().to_string();
    let agent_model = route.agent_model.trim().to_string();
    if upstream_model.is_empty() || agent_model.is_empty() {
        return None;
    }
    let upstream_model =
        catalog::canonical_model_id(endpoint, &upstream_model).unwrap_or(upstream_model);
    let model_def = catalog::find_model(endpoint, &upstream_model);
    let model_context_window = model_def.and_then(|model_def| model_def.context_window);
    let model_capabilities = model_def
        .map(|model_def| endpoint.capabilities.content.merge(&model_def.capabilities))
        .unwrap_or_else(|| endpoint.capabilities.content.clone());
    Some(BridgeModelSettings {
        agent_model,
        model_context_window,
        model_capabilities,
    })
}

fn render_claude_bridge_profile(
    profile: &ProfileDef,
    _launch_id: &str,
    settings: BridgeLaunchSettings,
) -> RenderedProfile {
    let bridge_base_url = format!(
        "http://127.0.0.1:{}/va/local-api/{}/{}/{}",
        config::DEFAULT_PORT,
        profile.id,
        settings.scope,
        settings.target_api_type
    );
    RenderedProfile {
        env: claude_env(settings.api_key.clone(), bridge_base_url, &settings),
        settings_files: Vec::new(),
        command_args: Vec::new(),
        config_env: None,
    }
}

fn claude_env(
    api_key: String,
    base_url: String,
    settings: &BridgeLaunchSettings,
) -> Vec<(String, String)> {
    let default_model = settings.default_model();
    let mut env = vec![
        ("ANTHROPIC_API_KEY".to_string(), api_key.clone()),
        ("ANTHROPIC_BASE_URL".to_string(), base_url),
        (
            "ANTHROPIC_MODEL".to_string(),
            default_model.agent_model.clone(),
        ),
        (
            "CLAUDE_CODE_ENABLE_GATEWAY_MODEL_DISCOVERY".to_string(),
            "1".to_string(),
        ),
    ];
    if !is_claude_discoverable_model(&default_model.agent_model) {
        env.push((
            "ANTHROPIC_CUSTOM_MODEL_OPTION".to_string(),
            default_model.agent_model.clone(),
        ));
        env.push((
            "ANTHROPIC_CUSTOM_MODEL_OPTION_NAME".to_string(),
            default_model.agent_model.clone(),
        ));
        env.push((
            "ANTHROPIC_CUSTOM_MODEL_OPTION_DESCRIPTION".to_string(),
            format!("{} {}", settings.provider_label, default_model.agent_model),
        ));
    }
    if let Some(model_context_window) = default_model.model_context_window {
        env.push((
            "CLAUDE_CODE_AUTO_COMPACT_WINDOW".to_string(),
            model_context_window.to_string(),
        ));
    }
    env
}

fn is_claude_discoverable_model(model: &str) -> bool {
    let model = model.trim().to_ascii_lowercase();
    model.starts_with("claude") || model.starts_with("anthropic")
}

fn render_codex_bridge_profile(
    profile: &ProfileDef,
    launch_id: &str,
    settings: BridgeLaunchSettings,
) -> RenderedProfile {
    let bridge_base_url = format!(
        "http://127.0.0.1:{}/va/local-api/{}/{}/{}/v1",
        config::DEFAULT_PORT,
        profile.id,
        settings.scope,
        settings.target_api_type
    );
    let provider_key = format!("model_providers.{}", profile.provider);
    let mut command_args = Vec::new();
    let default_model = settings.default_model();

    push_config_arg(
        &mut command_args,
        "model",
        &toml_string(&default_model.agent_model),
    );
    push_config_arg(
        &mut command_args,
        "model_provider",
        &toml_string(&profile.provider),
    );
    push_config_arg(
        &mut command_args,
        "model_reasoning_effort",
        &toml_string(&settings.reasoning_effort),
    );
    let mut settings_files = Vec::new();
    if let Some(context_window) = default_model.model_context_window {
        push_config_arg(
            &mut command_args,
            "model_context_window",
            &context_window.to_string(),
        );
    }
    let specs: Vec<_> = settings
        .models
        .iter()
        .map(|model| CodexModelCatalogSpec {
            model: model.agent_model.as_str(),
            provider_label: &settings.provider_label,
            context_window: model.model_context_window,
            capabilities: &model.model_capabilities,
        })
        .collect();
    if let Some(model_catalog_json) = codex_metadata::build_model_catalog_json(&specs) {
        let rel_path = format!("codex-model-catalog-{launch_id}.json");
        let catalog_path = super::runtime::profile_state_dir(&profile.id).join(&rel_path);
        let catalog_path = catalog_path.to_string_lossy();
        push_config_arg(
            &mut command_args,
            "model_catalog_json",
            &toml_string(catalog_path.as_ref()),
        );
        settings_files.push(RenderedSettingsFile {
            rel_path,
            contents: model_catalog_json,
        });
    }
    push_provider_config_arg(
        &mut command_args,
        &provider_key,
        "name",
        &toml_string(&settings.provider_label),
    );
    push_provider_config_arg(
        &mut command_args,
        &provider_key,
        "base_url",
        &toml_string(&bridge_base_url),
    );
    push_provider_config_arg(
        &mut command_args,
        &provider_key,
        "wire_api",
        &toml_string("responses"),
    );
    push_provider_config_arg(
        &mut command_args,
        &provider_key,
        "env_key",
        &toml_string("OPENAI_API_KEY"),
    );
    push_provider_config_arg(
        &mut command_args,
        &provider_key,
        "requires_openai_auth",
        "true",
    );

    RenderedProfile {
        env: vec![("OPENAI_API_KEY".to_string(), settings.api_key)],
        settings_files,
        command_args,
        config_env: None,
    }
}

fn render_opencode_bridge_profile(
    profile: &ProfileDef,
    _launch_id: &str,
    client_api_type: &str,
    settings: BridgeLaunchSettings,
) -> RenderedProfile {
    let bridge_base_url = opencode_bridge_base_url(profile, &settings, client_api_type);
    let npm = match client_api_type {
        "anthropic" => "@ai-sdk/anthropic",
        "openai-chat" => "@ai-sdk/openai-compatible",
        _ => "@ai-sdk/openai",
    };
    let provider_id = profile.provider.clone();
    let model = settings.default_model().agent_model.clone();
    let mut models = Map::new();
    for model in &settings.models {
        models.insert(
            model.agent_model.clone(),
            json!({ "name": model.agent_model.as_str() }),
        );
    }
    let mut providers = Map::new();
    providers.insert(
        provider_id.clone(),
        json!({
            "npm": npm,
            "name": settings.provider_label,
            "options": {
                "baseURL": bridge_base_url,
                "apiKey": "{env:VIBEAROUND_OPENCODE_API_KEY}",
                "setCacheKey": true
            },
            "models": Value::Object(models)
        }),
    );
    let config = json!({
        "$schema": "https://opencode.ai/config.json",
        "model": format!("{}/{}", provider_id, model),
        "provider": Value::Object(providers)
    });

    RenderedProfile {
        env: vec![("VIBEAROUND_OPENCODE_API_KEY".to_string(), settings.api_key)],
        settings_files: vec![RenderedSettingsFile {
            rel_path: "opencode.json".to_string(),
            contents: serde_json::to_string_pretty(&config).unwrap_or_else(|_| "{}".to_string()),
        }],
        command_args: Vec::new(),
        config_env: Some(ConfigEnvTarget::File {
            env: "OPENCODE_CONFIG",
            rel_path: "opencode.json",
        }),
    }
}

fn render_gemini_bridge_profile(
    profile: &ProfileDef,
    settings: BridgeLaunchSettings,
) -> RenderedProfile {
    let bridge_base_url = format!(
        "http://127.0.0.1:{}/va/local-api/{}/{}/{}",
        config::DEFAULT_PORT,
        profile.id,
        settings.scope,
        settings.target_api_type
    );
    RenderedProfile {
        env: vec![
            (
                "GEMINI_API_KEY".to_string(),
                "vibearound-local-bridge".to_string(),
            ),
            (
                "GOOGLE_API_KEY".to_string(),
                "vibearound-local-bridge".to_string(),
            ),
            (
                "GEMINI_DEFAULT_AUTH_TYPE".to_string(),
                "gemini-api-key".to_string(),
            ),
            ("GOOGLE_GEMINI_BASE_URL".to_string(), bridge_base_url),
            (
                "GEMINI_MODEL".to_string(),
                settings.default_model().agent_model.clone(),
            ),
        ],
        settings_files: Vec::new(),
        command_args: Vec::new(),
        config_env: None,
    }
}

fn render_pi_bridge_profile(
    profile: &ProfileDef,
    client_api_type: &str,
    settings: BridgeLaunchSettings,
) -> anyhow::Result<RenderedProfile> {
    let bridge_base_url = super::pi_launch::bridge_base_url(
        &profile.id,
        &settings.scope,
        &settings.target_api_type,
        client_api_type,
    );
    let provider_id = super::pi_launch::provider_id(
        &profile.id,
        &format!("bridge-{}-{}", client_api_type, settings.target_api_type),
    );
    let default_model = settings.default_model().clone();
    super::pi_launch::render_pi_provider(super::pi_launch::PiProviderLaunchConfig {
        profile_id: &profile.id,
        provider_id: provider_id.clone(),
        provider_label: &settings.provider_label,
        api_type: client_api_type,
        api_key: settings.api_key,
        base_url: bridge_base_url,
        model: default_model.agent_model,
        model_context_window: default_model.model_context_window,
        model_capabilities: default_model.model_capabilities,
        reasoning: client_api_type == "openai-responses",
        headers: Default::default(),
        auth_header: false,
        file_stem: provider_id,
    })
}

fn opencode_bridge_base_url(
    profile: &ProfileDef,
    settings: &BridgeLaunchSettings,
    client_api_type: &str,
) -> String {
    let suffix = if client_api_type == "anthropic" {
        ""
    } else {
        "/v1"
    };
    format!(
        "http://127.0.0.1:{}/va/local-api/{}/{}/{}{}",
        config::DEFAULT_PORT,
        profile.id,
        settings.scope,
        settings.target_api_type,
        suffix
    )
}

fn push_config_arg(args: &mut Vec<String>, key: &str, value: &str) {
    args.push("-c".to_string());
    args.push(format!("{key}={value}"));
}

fn push_provider_config_arg(args: &mut Vec<String>, provider_key: &str, field: &str, value: &str) {
    push_config_arg(args, &format!("{provider_key}.{field}"), value);
}

fn toml_string(s: &str) -> String {
    if s.contains('\'') {
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

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use crate::profiles::schema::{ApiTypeOverrides, AuthMode, ProfileDef};
    use serde_json::Value;

    use super::*;

    #[test]
    fn codex_bridge_launch_includes_catalog_context_window() {
        let profile = dashscope_profile();

        let rendered = render_bridge_launch(
            &profile,
            "codex",
            "launch-test",
            "openai-responses",
            "openai-chat",
            Some("qwen3.6-plus"),
            None,
            &[],
        )
        .expect("codex bridge launch renders");

        assert!(rendered
            .command_args
            .iter()
            .any(|arg| arg == "model='qwen3.6-plus'"));
        assert!(rendered
            .command_args
            .iter()
            .any(|arg| arg == "model_context_window=1000000"));
    }

    #[test]
    fn codex_bridge_launch_includes_kimi_model_catalog() {
        let profile = moonshot_profile();

        let rendered = render_bridge_launch(
            &profile,
            "codex",
            "launch-test",
            "openai-responses",
            "anthropic",
            Some("kimi-code"),
            None,
            &[],
        )
        .expect("codex bridge launch renders");

        assert!(rendered
            .command_args
            .iter()
            .any(|arg| arg == "model='kimi-code'"));
        assert!(rendered
            .command_args
            .iter()
            .any(|arg| arg == "model_context_window=256000"));
        assert!(rendered
            .command_args
            .iter()
            .any(|arg| arg.starts_with("model_catalog_json='")));

        let catalog_file = rendered
            .settings_files
            .iter()
            .find(|settings_file| settings_file.rel_path == "codex-model-catalog-launch-test.json")
            .expect("codex model catalog file");
        let catalog: Value =
            serde_json::from_str(&catalog_file.contents).expect("catalog json parses");
        let model = &catalog["models"][0];
        assert_eq!(model["slug"], "kimi-code");
        assert_eq!(model["context_window"], 256_000);
        assert_eq!(model["max_context_window"], 256_000);
    }

    #[test]
    fn codex_bridge_launch_includes_full_bridge_model_catalog() {
        let profile = dashscope_profile();
        let rendered = render_bridge_launch(
            &profile,
            "codex",
            "launch-test",
            "openai-responses",
            "openai-chat",
            Some("qwen3.6-plus"),
            None,
            &[
                ProfileBridgeModelRoute {
                    upstream_model: "qwen3.6-plus".to_string(),
                    agent_model: "gpt-5.1-codex".to_string(),
                },
                ProfileBridgeModelRoute {
                    upstream_model: "qwen3.5-plus".to_string(),
                    agent_model: "gpt-5.1-mini".to_string(),
                },
            ],
        )
        .expect("codex bridge launch renders");

        assert!(rendered
            .command_args
            .iter()
            .any(|arg| arg == "model='gpt-5.1-codex'"));
        let catalog_file = rendered
            .settings_files
            .iter()
            .find(|settings_file| settings_file.rel_path == "codex-model-catalog-launch-test.json")
            .expect("codex model catalog file");
        let catalog: Value =
            serde_json::from_str(&catalog_file.contents).expect("catalog json parses");
        let models = catalog["models"].as_array().expect("models array");

        assert_eq!(models.len(), 2);
        assert_eq!(models[0]["slug"], "gpt-5.1-codex");
        assert_eq!(models[0]["context_window"], 1_000_000);
        assert_eq!(models[1]["slug"], "gpt-5.1-mini");
        assert_eq!(models[1]["context_window"], 1_000_000);
    }

    #[test]
    fn gemini_bridge_launch_points_cli_at_local_gemini_api() {
        let profile = dashscope_profile();

        let rendered = render_bridge_launch(
            &profile,
            "gemini",
            "launch-test",
            "gemini",
            "openai-chat",
            Some("qwen3.6-plus"),
            Some("gemini-2.5-flash"),
            &[],
        )
        .expect("gemini bridge launch renders");

        assert!(rendered.env.contains(&(
            "GOOGLE_GEMINI_BASE_URL".to_string(),
            "http://127.0.0.1:12358/va/local-api/dashscope-test/gemini-gemini/openai-chat"
                .to_string()
        )));
        assert!(rendered.env.contains(&(
            "GEMINI_API_KEY".to_string(),
            "vibearound-local-bridge".to_string()
        )));
        assert!(rendered.env.contains(&(
            "GOOGLE_API_KEY".to_string(),
            "vibearound-local-bridge".to_string()
        )));
        assert!(rendered.env.contains(&(
            "GEMINI_DEFAULT_AUTH_TYPE".to_string(),
            "gemini-api-key".to_string()
        )));
        assert!(rendered
            .env
            .contains(&("GEMINI_MODEL".to_string(), "gemini-2.5-flash".to_string())));
        assert!(rendered.settings_files.is_empty());
        assert!(rendered.config_env.is_none());
    }

    #[test]
    fn pi_bridge_launch_materializes_local_provider_extension() {
        let profile = moonshot_profile();

        let rendered = render_bridge_launch(
            &profile,
            "pi",
            "launch-test",
            "openai-responses",
            "anthropic",
            Some("kimi-code"),
            Some("gpt-5.1"),
            &[],
        )
        .expect("pi bridge launch renders");

        assert!(rendered
            .env
            .contains(&("VIBEAROUND_PI_API_KEY".to_string(), "test-key".to_string())));
        assert!(rendered
            .command_args
            .windows(2)
            .any(|args| args[0] == "--provider"
                && args[1] == "vibearound-moonshot-test-bridge-openai-responses-anthropic"));
        assert!(rendered
            .command_args
            .windows(2)
            .any(|args| args[0] == "--model" && args[1] == "gpt-5.1"));
        let extension = rendered
            .settings_files
            .iter()
            .find(|settings_file| settings_file.rel_path.ends_with(".mjs"))
            .expect("pi extension file");
        assert!(extension.contents.contains("\"api\": \"openai-responses\""));
        assert!(extension.contents.contains(
            "\"baseUrl\": \"http://127.0.0.1:12358/va/local-api/moonshot-test/pi-openai-responses/anthropic/v1\""
        ));
        assert!(rendered.config_env.is_none());
    }

    #[test]
    fn codex_bridge_launch_keeps_gemini_alias_and_metadata() {
        let profile = gemini_profile();

        let rendered = render_bridge_launch(
            &profile,
            "codex",
            "launch-test",
            "openai-responses",
            "openai-chat",
            None,
            None,
            &[],
        )
        .expect("codex bridge launch renders");

        assert!(rendered
            .command_args
            .iter()
            .any(|arg| arg == "model='gemini-3.1-pro'"));
        assert!(rendered
            .command_args
            .iter()
            .any(|arg| arg == "model_context_window=1048576"));
    }

    #[test]
    fn claude_bridge_launch_uses_standard_env_shape() {
        for profile in [dashscope_profile(), deepseek_profile()] {
            let rendered = render_bridge_launch(
                &profile,
                "claude",
                "launch-test",
                "anthropic",
                "openai-chat",
                None,
                Some("claude-opus-4-7[1m]"),
                &[],
            )
            .expect("claude bridge launch renders");

            let keys: Vec<_> = rendered.env.iter().map(|(key, _)| key.as_str()).collect();
            assert_eq!(
                keys,
                vec![
                    "ANTHROPIC_API_KEY",
                    "ANTHROPIC_BASE_URL",
                    "ANTHROPIC_MODEL",
                    "CLAUDE_CODE_ENABLE_GATEWAY_MODEL_DISCOVERY",
                    "CLAUDE_CODE_AUTO_COMPACT_WINDOW",
                ]
            );
            assert_eq!(
                rendered
                    .env
                    .iter()
                    .find(|(key, _)| key == "CLAUDE_CODE_AUTO_COMPACT_WINDOW")
                    .map(|(_, value)| value.as_str()),
                Some("1000000")
            );
            assert!(rendered.settings_files.is_empty());
            assert!(rendered.command_args.is_empty());
            assert!(rendered.config_env.is_none());
        }
    }

    fn dashscope_profile() -> ProfileDef {
        let mut credentials = BTreeMap::new();
        credentials.insert("api_key".to_string(), "test-key".to_string());

        let mut overrides = BTreeMap::new();
        overrides.insert(
            "openai-chat".to_string(),
            ApiTypeOverrides {
                endpoint_id: Some("coding-plan".to_string()),
                base_url: None,
                model: Some("qwen3.6-plus".to_string()),
                reasoning_effort: Some("medium".to_string()),
                capabilities: None,
            },
        );

        ProfileDef {
            id: "dashscope-test".to_string(),
            label: "DashScope Test".to_string(),
            provider: "dashscope".to_string(),
            auth_mode: AuthMode::ApiKey,
            api_types: vec!["openai-chat".to_string()],
            credentials,
            overrides,
            use_settings_proxy: false,
            provider_settings: Default::default(),
        }
    }

    fn gemini_profile() -> ProfileDef {
        let mut credentials = BTreeMap::new();
        credentials.insert("api_key".to_string(), "test-key".to_string());

        let mut overrides = BTreeMap::new();
        overrides.insert(
            "openai-chat".to_string(),
            ApiTypeOverrides {
                endpoint_id: Some("openai-compatible".to_string()),
                base_url: None,
                model: Some("gemini-3.1-pro".to_string()),
                reasoning_effort: Some("medium".to_string()),
                capabilities: None,
            },
        );

        ProfileDef {
            id: "gemini-test".to_string(),
            label: "Gemini Test".to_string(),
            provider: "gemini".to_string(),
            auth_mode: AuthMode::ApiKey,
            api_types: vec!["openai-chat".to_string()],
            credentials,
            overrides,
            use_settings_proxy: false,
            provider_settings: Default::default(),
        }
    }

    fn deepseek_profile() -> ProfileDef {
        let mut credentials = BTreeMap::new();
        credentials.insert("api_key".to_string(), "test-key".to_string());

        let mut overrides = BTreeMap::new();
        overrides.insert(
            "openai-chat".to_string(),
            ApiTypeOverrides {
                endpoint_id: None,
                base_url: None,
                model: Some("deepseek-v4-pro".to_string()),
                reasoning_effort: Some("medium".to_string()),
                capabilities: None,
            },
        );

        ProfileDef {
            id: "deepseek-test".to_string(),
            label: "DeepSeek Test".to_string(),
            provider: "deepseek".to_string(),
            auth_mode: AuthMode::ApiKey,
            api_types: vec!["openai-chat".to_string()],
            credentials,
            overrides,
            use_settings_proxy: false,
            provider_settings: Default::default(),
        }
    }

    fn moonshot_profile() -> ProfileDef {
        let mut credentials = BTreeMap::new();
        credentials.insert("api_key".to_string(), "test-key".to_string());

        let mut overrides = BTreeMap::new();
        overrides.insert(
            "anthropic".to_string(),
            ApiTypeOverrides {
                endpoint_id: Some("kimi-coding".to_string()),
                base_url: None,
                model: Some("kimi-for-coding".to_string()),
                reasoning_effort: Some("medium".to_string()),
                capabilities: None,
            },
        );

        ProfileDef {
            id: "moonshot-test".to_string(),
            label: "Moonshot Test".to_string(),
            provider: "moonshot".to_string(),
            auth_mode: AuthMode::ApiKey,
            api_types: vec!["anthropic".to_string()],
            credentials,
            overrides,
            use_settings_proxy: false,
            provider_settings: Default::default(),
        }
    }
}
