//! Runtime helpers for applying rendered profiles to launched CLIs.

use std::path::{Path, PathBuf};

use super::catalog::{self, ProviderCatalog};
use super::connections::ProfileAgentRoute;
use super::render::{render, ConfigEnvTarget, RenderedProfile};
use super::schema::ProfileDef;
use crate::{auth, config};
use anyhow::{anyhow, bail, Context};

pub fn render_for_launch(
    profile: &ProfileDef,
    launch_target: &str,
) -> anyhow::Result<RenderedProfile> {
    let provider = catalog::get(&profile.provider)
        .ok_or_else(|| anyhow!("unknown provider '{}'", profile.provider))?;
    let api_type = api_type_for_launch_target(profile, provider, launch_target)?;
    render(profile, api_type, launch_target, provider)
}

pub fn render_for_launch_api_type(
    profile: &ProfileDef,
    launch_target: &str,
    api_type: &str,
) -> anyhow::Result<RenderedProfile> {
    let provider = catalog::get(&profile.provider)
        .ok_or_else(|| anyhow!("unknown provider '{}'", profile.provider))?;
    if !api_types_for_launch_target(launch_target).contains(&api_type) {
        bail!(
            "launch target '{}' does not support api kind '{}'",
            launch_target,
            api_type
        );
    }
    if !profile.api_types.iter().any(|t| t == api_type)
        || !provider.endpoints.iter().any(|e| e.api_type == api_type)
    {
        bail!(
            "profile '{}' cannot launch '{}' with api kind '{}'",
            profile.id,
            launch_target,
            api_type
        );
    }
    render(profile, api_type, launch_target, provider)
}

pub fn env_for_launch(
    profile: &ProfileDef,
    launch_target: &str,
) -> anyhow::Result<Vec<(String, String)>> {
    let rendered = render_for_launch(profile, launch_target)?;
    materialize_env_for_profile(profile, rendered)
}

pub fn render_for_agent_route(
    profile: &ProfileDef,
    launch_target: &str,
    launch_id: &str,
    route: &ProfileAgentRoute,
) -> anyhow::Result<RenderedProfile> {
    match route.bridge_target_api_type.as_deref() {
        Some(target_api_type) => super::bridge_launch::render_bridge_launch(
            profile,
            launch_target,
            launch_id,
            &route.client_api_type,
            target_api_type,
            route.bridge_upstream_model.as_deref(),
            route.bridge_fake_model_id.as_deref(),
            &route.bridge_models,
        ),
        None => render_for_launch_api_type(profile, launch_target, &route.client_api_type),
    }
}

pub fn materialize_env(
    profile_id: &str,
    rendered: RenderedProfile,
) -> anyhow::Result<Vec<(String, String)>> {
    let mut env = rendered.env.clone();
    let dir = profile_state_dir(profile_id);
    for sf in &rendered.settings_files {
        materialize_settings_file(&dir, &sf.rel_path, &sf.contents)?;
    }

    let Some(target) = rendered.config_env else {
        return Ok(env);
    };

    match target {
        ConfigEnvTarget::Directory(env_name) => {
            env.push((env_name.to_string(), dir.to_string_lossy().into_owned()));
        }
        ConfigEnvTarget::File {
            env: env_name,
            rel_path,
        } => {
            env.push((
                env_name.to_string(),
                dir.join(rel_path).to_string_lossy().into_owned(),
            ));
        }
    }

    Ok(env)
}

pub fn materialize_env_for_profile(
    profile: &ProfileDef,
    rendered: RenderedProfile,
) -> anyhow::Result<Vec<(String, String)>> {
    let mut env = materialize_env(&profile.id, rendered)?;
    append_settings_proxy_env(profile, &mut env)?;
    Ok(env)
}

pub fn append_settings_proxy_env(
    profile: &ProfileDef,
    env: &mut Vec<(String, String)>,
) -> anyhow::Result<()> {
    if !profile.use_settings_proxy {
        return Ok(());
    }

    let cfg = config::ensure_loaded();
    if !cfg.proxy.enabled {
        return Ok(());
    }
    append_proxy_env_vars(&profile.id, &cfg.proxy, env)
}

fn append_proxy_env_vars(
    profile_id: &str,
    proxy: &config::HttpProxyConfig,
    env: &mut Vec<(String, String)>,
) -> anyhow::Result<()> {
    let proxy_url = proxy.http_proxy.as_deref().ok_or_else(|| {
        anyhow!("profile '{profile_id}' uses Settings proxy but HTTP proxy URL is empty")
    })?;
    env.retain(|(key, _)| !is_proxy_env_key(key));
    env.extend([
        ("HTTP_PROXY".to_string(), proxy_url.to_string()),
        ("HTTPS_PROXY".to_string(), proxy_url.to_string()),
        ("http_proxy".to_string(), proxy_url.to_string()),
        ("https_proxy".to_string(), proxy_url.to_string()),
    ]);
    if let Some(no_proxy) = proxy.no_proxy.as_deref() {
        env.extend([
            ("NO_PROXY".to_string(), no_proxy.to_string()),
            ("no_proxy".to_string(), no_proxy.to_string()),
        ]);
    }
    Ok(())
}

fn is_proxy_env_key(key: &str) -> bool {
    matches!(
        key,
        "HTTP_PROXY" | "HTTPS_PROXY" | "http_proxy" | "https_proxy" | "NO_PROXY" | "no_proxy"
    )
}

pub fn launch_targets_for_api_types(
    api_types: &[String],
) -> Vec<(&'static str, &'static str, &'static str)> {
    let has = |needle: &str| api_types.iter().any(|t| t == needle);
    let mut out = Vec::new();
    if has("anthropic") {
        out.push(("claude", "Claude Code", "anthropic"));
    }
    if has("openai-responses") {
        out.push(("codex", "Codex", "openai-responses"));
    }
    if has("gemini") {
        out.push(("gemini", "Gemini CLI", "gemini"));
    }
    if let Some(api_type) = pi_api_type_for(api_types) {
        out.push(("pi", "Pi", api_type));
    }
    if has("openai-responses") {
        out.push(("opencode", "OpenCode", "openai-responses"));
    } else if has("openai-chat") {
        out.push(("opencode", "OpenCode", "openai-chat"));
    } else if has("anthropic") {
        out.push(("opencode", "OpenCode", "anthropic"));
    }
    out
}

pub fn agent_id_for(launch_target: &str) -> anyhow::Result<&'static str> {
    match launch_target {
        "claude" => Ok("claude"),
        "codex" => Ok("codex"),
        "gemini" => Ok("gemini"),
        "opencode" => Ok("opencode"),
        "pi" => Ok("pi"),
        other => bail!("unsupported launch target: '{}'", other),
    }
}

pub fn api_type_for_launch_target<'a>(
    profile: &'a ProfileDef,
    provider: &'a ProviderCatalog,
    launch_target: &str,
) -> anyhow::Result<&'a str> {
    let candidates = api_types_for_launch_target(launch_target);

    for candidate in candidates {
        if profile.api_types.iter().any(|t| t == candidate)
            && provider.endpoints.iter().any(|e| e.api_type == *candidate)
        {
            return Ok(candidate);
        }
    }

    bail!(
        "profile '{}' cannot launch '{}' with provider '{}'",
        profile.id,
        launch_target,
        profile.provider
    )
}

fn api_types_for_launch_target(launch_target: &str) -> &'static [&'static str] {
    match launch_target {
        "claude" => &["anthropic"],
        "codex" => &["openai-responses"],
        "gemini" => &["gemini"],
        "opencode" => &["openai-responses", "openai-chat", "anthropic"],
        "pi" => &["anthropic", "openai-responses", "openai-chat"],
        _ => &[],
    }
}

fn pi_api_type_for(api_types: &[String]) -> Option<&'static str> {
    ["anthropic", "openai-responses", "openai-chat"]
        .into_iter()
        .find(|candidate| api_types.iter().any(|api_type| api_type == candidate))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codex_native_launch_requires_responses_api() {
        let chat_only = vec!["openai-chat".to_string()];
        assert!(!launch_targets_for_api_types(&chat_only)
            .iter()
            .any(|(id, _, _)| *id == "codex"));

        let responses = vec!["openai-responses".to_string()];
        assert!(launch_targets_for_api_types(&responses)
            .iter()
            .any(|(id, _, api_type)| *id == "codex" && *api_type == "openai-responses"));
        assert!(launch_targets_for_api_types(&responses)
            .iter()
            .any(|(id, _, api_type)| *id == "pi" && *api_type == "openai-responses"));

        let chat = vec!["openai-chat".to_string()];
        assert!(launch_targets_for_api_types(&chat)
            .iter()
            .any(|(id, _, api_type)| *id == "pi" && *api_type == "openai-chat"));
    }

    #[test]
    fn proxy_env_vars_cover_common_http_proxy_names() {
        let mut env = vec![
            ("HTTP_PROXY".to_string(), "http://old.example".to_string()),
            ("OTHER".to_string(), "1".to_string()),
        ];

        append_proxy_env_vars(
            "profile-test",
            &config::HttpProxyConfig {
                enabled: true,
                http_proxy: Some("http://127.0.0.1:7890".to_string()),
                no_proxy: Some("localhost,127.0.0.1".to_string()),
            },
            &mut env,
        )
        .expect("proxy env appends");

        assert_eq!(
            env,
            vec![
                ("OTHER".to_string(), "1".to_string()),
                (
                    "HTTP_PROXY".to_string(),
                    "http://127.0.0.1:7890".to_string()
                ),
                (
                    "HTTPS_PROXY".to_string(),
                    "http://127.0.0.1:7890".to_string()
                ),
                (
                    "http_proxy".to_string(),
                    "http://127.0.0.1:7890".to_string()
                ),
                (
                    "https_proxy".to_string(),
                    "http://127.0.0.1:7890".to_string()
                ),
                ("NO_PROXY".to_string(), "localhost,127.0.0.1".to_string()),
                ("no_proxy".to_string(), "localhost,127.0.0.1".to_string()),
            ]
        );
    }

    #[test]
    fn proxy_env_vars_require_proxy_url() {
        let mut env = Vec::new();
        let error = append_proxy_env_vars(
            "profile-test",
            &config::HttpProxyConfig {
                enabled: true,
                http_proxy: None,
                no_proxy: None,
            },
            &mut env,
        )
        .unwrap_err();

        assert!(error.to_string().contains("HTTP proxy URL is empty"));
    }
}

pub fn profile_state_dir(id: &str) -> PathBuf {
    config::data_dir().join("profile-state").join(id)
}

fn materialize_settings_file(dir: &Path, rel_path: &str, contents: &str) -> anyhow::Result<()> {
    let target = dir.join(rel_path);

    // Defense in depth: render::validate_rel_path rejects `..`, but this
    // catches catalog paths whose parent resolves through a symlink.
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent).with_context(|| format!("create {:?}", parent))?;
        let canonical_parent =
            std::fs::canonicalize(parent).with_context(|| format!("canonicalize {:?}", parent))?;
        let canonical_root =
            std::fs::canonicalize(dir).with_context(|| format!("canonicalize {:?}", dir))?;
        if !canonical_parent.starts_with(&canonical_root) {
            bail!(
                "rendered settings_file escapes profile-state dir: {:?}",
                target
            );
        }
    }

    let tmp = target.with_extension("tmp");
    std::fs::write(&tmp, contents).with_context(|| format!("write {:?}", tmp))?;
    auth::set_owner_only(&tmp).ok();
    std::fs::rename(&tmp, &target).with_context(|| format!("rename to {:?}", target))?;
    Ok(())
}
