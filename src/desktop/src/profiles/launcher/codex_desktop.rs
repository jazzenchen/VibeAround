//! Codex Desktop profile overlay.
//!
//! Codex Desktop reads the shared `~/.codex/config.toml`, while the CLI can
//! take profile-specific `-c` args. For desktop profile launches, reconcile our
//! previous marker blocks first, then write a fresh VibeAround-owned overlay.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use ::common::{auth, config, profiles};
use anyhow::Context;
use profiles::{render::RenderedProfile, ProfileDef};

use super::codex;

const MARKER: &str = "VIBEAROUND-CODEX-DESKTOP";
const ROOT_KEYS: &[&str] = &[
    "model",
    "model_provider",
    "model_reasoning_effort",
    "model_context_window",
    "model_catalog_json",
    "disable_response_storage",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OverlayBlock {
    Restore,
    Active,
    Provider,
}

#[derive(Debug)]
struct CodexDesktopOverlay {
    launch_id: String,
    profile_id: String,
    root_entries: Vec<(String, String)>,
    provider_id: String,
    provider_entries: Vec<(String, String)>,
}

pub(super) fn apply_profile_overlay(
    profile: &ProfileDef,
    launch_id: &str,
    rendered: RenderedProfile,
) -> anyhow::Result<()> {
    let env = profiles::runtime::materialize_env(&profile.id, rendered.clone())
        .with_context(|| format!("materialize Codex Desktop profile '{}'", profile.id))?;
    let overlay = CodexDesktopOverlay::from_rendered(profile, launch_id, &rendered, &env);
    apply_overlay_to_path(&codex_config_path(), &overlay)
}

pub(super) fn cleanup_profile_overlay() -> anyhow::Result<()> {
    cleanup_overlay_at_path(&codex_config_path())
}

fn codex_config_path() -> PathBuf {
    config::home_dir().join(".codex").join("config.toml")
}

fn apply_overlay_to_path(path: &Path, overlay: &CodexDesktopOverlay) -> anyhow::Result<()> {
    let current = std::fs::read_to_string(path).unwrap_or_default();
    let next = apply_overlay_to_string(&current, overlay);
    write_config_if_changed(path, &current, next)
}

fn cleanup_overlay_at_path(path: &Path) -> anyhow::Result<()> {
    let current = match std::fs::read_to_string(path) {
        Ok(current) => current,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error).with_context(|| format!("read Codex config {:?}", path)),
    };
    let next = cleanup_vibearound_blocks(&current);
    write_config_if_changed(path, &current, next)
}

fn write_config_if_changed(path: &Path, current: &str, next: String) -> anyhow::Result<()> {
    if next == current {
        return Ok(());
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("create Codex config dir {:?}", parent))?;
    }
    let tmp = path.with_file_name(format!(
        ".{}.{}.tmp",
        path.file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("config.toml"),
        uuid::Uuid::new_v4()
    ));
    std::fs::write(&tmp, next).with_context(|| format!("write Codex config temp {:?}", tmp))?;
    auth::set_owner_only(&tmp).with_context(|| format!("chmod Codex config temp {:?}", tmp))?;
    std::fs::rename(&tmp, path).with_context(|| format!("replace Codex config {:?}", path))?;
    auth::set_owner_only(path).with_context(|| format!("chmod Codex config {:?}", path))?;
    Ok(())
}

fn apply_overlay_to_string(current: &str, overlay: &CodexDesktopOverlay) -> String {
    let cleaned = normalize_dashboard_mcp_urls(&cleanup_vibearound_blocks(current));
    let (body, restore_lines) = remove_conflicting_root_keys(&cleaned);
    let (root_body, table_body) = split_root_and_table_body(&body);
    let mut sections = Vec::new();
    if !restore_lines.is_empty() {
        sections.push(render_restore_block(overlay, &restore_lines));
    }
    sections.push(render_active_block(overlay));
    push_non_empty_section(&mut sections, &root_body);
    if !overlay.provider_entries.is_empty() {
        sections.push(render_provider_table(overlay));
    }
    push_non_empty_section(&mut sections, &table_body);

    ensure_trailing_newline(sections.join("\n\n"))
}

fn cleanup_vibearound_blocks(input: &str) -> String {
    let lines: Vec<&str> = input.lines().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        let Some(kind) = begin_block_kind(lines[i]) else {
            if end_block_kind(lines[i]).is_some() {
                i += 1;
                continue;
            }
            out.push(lines[i].to_string());
            i += 1;
            continue;
        };

        let Some(end_index) = find_end_block(&lines, i + 1, kind) else {
            i += 1;
            continue;
        };

        match kind {
            OverlayBlock::Restore => {
                for line in &lines[i + 1..end_index] {
                    out.push(uncomment_restore_line(line));
                }
            }
            OverlayBlock::Active => {
                out.extend(preserve_foreign_root_lines(&lines[i + 1..end_index]));
            }
            OverlayBlock::Provider => {
                out.extend(cleanup_provider_block_lines(&lines[i + 1..end_index]));
            }
        }
        i = end_index + 1;
    }
    remove_unmarked_vibearound_overlay(&ensure_trailing_newline(out.join("\n")))
}

fn preserve_foreign_root_lines(lines: &[&str]) -> Vec<String> {
    lines
        .iter()
        .filter_map(|line| {
            let trimmed = line.trim_start();
            if root_key_for_line(trimmed).is_some_and(|key| ROOT_KEYS.contains(&key)) {
                return None;
            }
            if trimmed.is_empty() {
                return None;
            }
            Some((*line).to_string())
        })
        .collect()
}

fn cleanup_provider_block_lines(lines: &[&str]) -> Vec<String> {
    let cleaned = remove_unmarked_vibearound_overlay(&ensure_trailing_newline(lines.join("\n")));
    cleaned
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(ToOwned::to_owned)
        .collect()
}

fn remove_unmarked_vibearound_overlay(input: &str) -> String {
    let remove_root_keys = root_model_provider_is_vibearound(input);
    let mut out = Vec::new();
    let mut in_root = true;
    let mut skip_table = false;

    for line in input.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with('[') {
            in_root = false;
            skip_table = is_vibearound_model_provider_table(trimmed);
            if skip_table {
                continue;
            }
        } else if skip_table {
            continue;
        }

        if in_root && remove_root_keys {
            if let Some(key) = root_key_for_line(trimmed) {
                if ROOT_KEYS.contains(&key) {
                    continue;
                }
            }
        }

        out.push(line.to_string());
    }

    ensure_trailing_newline(out.join("\n"))
}

fn normalize_dashboard_mcp_urls(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for line in input.lines() {
        out.push_str(&normalize_dashboard_mcp_urls_in_line(line));
        out.push('\n');
    }
    out
}

fn normalize_dashboard_mcp_urls_in_line(line: &str) -> String {
    const HOST_PREFIXES: [&str; 2] = ["http://127.0.0.1:", "http://localhost:"];

    let mut out = line.to_string();
    for prefix in HOST_PREFIXES {
        let mut search_from = 0;
        while let Some(relative_start) = out[search_from..].find(prefix) {
            let start = search_from + relative_start;
            let port_start = start + prefix.len();
            let port_end = port_start
                + out[port_start..]
                    .chars()
                    .take_while(|ch| ch.is_ascii_digit())
                    .map(char::len_utf8)
                    .sum::<usize>();
            if port_end == port_start {
                search_from = port_start;
                continue;
            }

            let Some(rest) = out.get(port_end..) else {
                break;
            };
            if rest.starts_with("/mcp") && rest[4..].chars().next().is_none_or(is_mcp_url_boundary)
            {
                out.replace_range(port_end..port_end + 4, "/va/mcp");
                search_from = port_end + "/va/mcp".len();
            } else {
                search_from = port_end;
            }
        }
    }
    out
}

fn is_mcp_url_boundary(ch: char) -> bool {
    ch == '?' || ch == '"' || ch == '\'' || ch.is_ascii_whitespace()
}

fn remove_conflicting_root_keys(input: &str) -> (String, Vec<String>) {
    let mut body = Vec::new();
    let mut restore = Vec::new();
    let mut in_root = true;

    for line in input.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with('[') {
            in_root = false;
        }
        if in_root {
            if let Some(key) = root_key_for_line(trimmed) {
                if ROOT_KEYS.contains(&key) {
                    restore.push(line.to_string());
                    continue;
                }
            }
        }
        body.push(line.to_string());
    }

    (ensure_trailing_newline(body.join("\n")), restore)
}

fn split_root_and_table_body(input: &str) -> (String, String) {
    let mut root = Vec::new();
    let mut tables = Vec::new();
    let mut in_tables = false;

    for line in input.lines() {
        if line.trim_start().starts_with('[') {
            in_tables = true;
        }
        if in_tables {
            tables.push(line.to_string());
        } else {
            root.push(line.to_string());
        }
    }

    (
        ensure_trailing_newline(root.join("\n")),
        ensure_trailing_newline(tables.join("\n")),
    )
}

fn push_non_empty_section(sections: &mut Vec<String>, body: &str) {
    let trimmed = body.trim_matches('\n');
    if !trimmed.trim().is_empty() {
        sections.push(trimmed.to_string());
    }
}

impl CodexDesktopOverlay {
    fn from_rendered(
        profile: &ProfileDef,
        launch_id: &str,
        rendered: &RenderedProfile,
        env: &[(String, String)],
    ) -> Self {
        let entries = config_entries_from_args(&rendered.command_args);
        let provider_id = format!("vibearound_{}", safe_config_key(&profile.id));
        let original_provider = entries
            .iter()
            .find(|(key, _)| key == "model_provider")
            .and_then(|(_, value)| parse_toml_string(value))
            .unwrap_or_else(|| profile.provider.clone());

        let mut root_entries = Vec::new();
        for (key, value) in &entries {
            if ROOT_KEYS.contains(&key.as_str()) {
                if key == "model_provider" {
                    root_entries.push((key.clone(), codex::toml_string(&provider_id)));
                } else {
                    root_entries.push((key.clone(), value.clone()));
                }
            }
        }
        if !root_entries.iter().any(|(key, _)| key == "model_provider") {
            root_entries.push((
                "model_provider".to_string(),
                codex::toml_string(&provider_id),
            ));
        }

        let mut provider_entries = BTreeMap::new();
        let prefix = format!("model_providers.{original_provider}.");
        for (key, value) in &entries {
            if let Some(field) = key.strip_prefix(&prefix) {
                provider_entries.insert(field.to_string(), value.clone());
            }
        }

        if let Some(token) = provider_token(&provider_entries, env) {
            provider_entries.remove("env_key");
            provider_entries.remove("requires_openai_auth");
            provider_entries.insert(
                "experimental_bearer_token".to_string(),
                codex::toml_string(&token),
            );
        }

        Self {
            launch_id: launch_id.to_string(),
            profile_id: profile.id.clone(),
            root_entries,
            provider_id,
            provider_entries: provider_entries.into_iter().collect(),
        }
    }
}

fn config_entries_from_args(args: &[String]) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut i = 0;
    while i + 1 < args.len() {
        if args[i] == "-c" {
            if let Some((key, value)) = args[i + 1].split_once('=') {
                let key = key.trim();
                let value = value.trim();
                if !key.is_empty() && !value.is_empty() {
                    out.push((key.to_string(), value.to_string()));
                }
            }
            i += 2;
        } else {
            i += 1;
        }
    }
    out
}

fn provider_token(
    provider_entries: &BTreeMap<String, String>,
    env: &[(String, String)],
) -> Option<String> {
    let env_key = provider_entries
        .get("env_key")
        .and_then(|value| parse_toml_string(value));
    env_key
        .as_deref()
        .and_then(|key| env_value(env, key))
        .or_else(|| {
            if env.len() == 1 {
                Some(env[0].1.clone())
            } else {
                None
            }
        })
}

fn env_value(env: &[(String, String)], key: &str) -> Option<String> {
    env.iter()
        .find(|(candidate, value)| candidate == key && !value.is_empty())
        .map(|(_, value)| value.clone())
}

fn render_restore_block(overlay: &CodexDesktopOverlay, restore_lines: &[String]) -> String {
    let mut lines = vec![begin_marker(OverlayBlock::Restore, overlay)];
    lines.extend(restore_lines.iter().map(|line| format!("# {line}")));
    lines.push(end_marker(OverlayBlock::Restore));
    lines.join("\n")
}

fn render_active_block(overlay: &CodexDesktopOverlay) -> String {
    let mut lines = vec![begin_marker(OverlayBlock::Active, overlay)];
    lines.extend(
        overlay
            .root_entries
            .iter()
            .map(|(key, value)| format!("{key} = {value}")),
    );
    lines.push(end_marker(OverlayBlock::Active));
    lines.join("\n")
}

fn render_provider_table(overlay: &CodexDesktopOverlay) -> String {
    let mut lines = vec![format!("[model_providers.{}]", overlay.provider_id)];
    lines.extend(
        overlay
            .provider_entries
            .iter()
            .map(|(key, value)| format!("{key} = {value}")),
    );
    lines.join("\n")
}

fn begin_marker(kind: OverlayBlock, overlay: &CodexDesktopOverlay) -> String {
    format!(
        "# {MARKER} BEGIN {} run={} profile={}",
        block_name(kind),
        overlay.launch_id,
        overlay.profile_id
    )
}

fn end_marker(kind: OverlayBlock) -> String {
    format!("# {MARKER} END {}", block_name(kind))
}

fn begin_block_kind(line: &str) -> Option<OverlayBlock> {
    let trimmed = line.trim();
    if !trimmed.starts_with("# ") || !trimmed.contains(MARKER) || !trimmed.contains(" BEGIN ") {
        return None;
    }
    if trimmed.contains(" BEGIN RESTORE") {
        Some(OverlayBlock::Restore)
    } else if trimmed.contains(" BEGIN ACTIVE") {
        Some(OverlayBlock::Active)
    } else if trimmed.contains(" BEGIN PROVIDER") {
        Some(OverlayBlock::Provider)
    } else {
        None
    }
}

fn end_block_kind(line: &str) -> Option<OverlayBlock> {
    let trimmed = line.trim();
    if !trimmed.starts_with("# ") || !trimmed.contains(MARKER) || !trimmed.contains(" END ") {
        return None;
    }
    if trimmed.contains(" END RESTORE") {
        Some(OverlayBlock::Restore)
    } else if trimmed.contains(" END ACTIVE") {
        Some(OverlayBlock::Active)
    } else if trimmed.contains(" END PROVIDER") {
        Some(OverlayBlock::Provider)
    } else {
        None
    }
}

fn find_end_block(lines: &[&str], start: usize, kind: OverlayBlock) -> Option<usize> {
    let end = end_marker(kind);
    lines
        .iter()
        .enumerate()
        .skip(start)
        .find(|(_, line)| line.trim() == end)
        .map(|(index, _)| index)
}

fn block_name(kind: OverlayBlock) -> &'static str {
    match kind {
        OverlayBlock::Restore => "RESTORE",
        OverlayBlock::Active => "ACTIVE",
        OverlayBlock::Provider => "PROVIDER",
    }
}

fn root_model_provider_is_vibearound(input: &str) -> bool {
    for line in input.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with('[') {
            return false;
        }
        if root_key_for_line(trimmed) == Some("model_provider") {
            let Some((_, value)) = trimmed.split_once('=') else {
                continue;
            };
            return parse_toml_string(value.trim())
                .as_deref()
                .is_some_and(is_vibearound_provider_id);
        }
    }
    false
}

fn is_vibearound_model_provider_table(trimmed: &str) -> bool {
    let Some(table) = toml_table_name(trimmed) else {
        return false;
    };
    let Some(provider_id) = table.strip_prefix("model_providers.") else {
        return false;
    };
    let provider_id = provider_id.trim().trim_matches('"').trim_matches('\'');
    is_vibearound_provider_id(provider_id)
}

fn is_vibearound_provider_id(value: &str) -> bool {
    value.starts_with("vibearound_")
}

fn toml_table_name(trimmed: &str) -> Option<&str> {
    if !trimmed.starts_with('[') || trimmed.starts_with("[[") {
        return None;
    }
    let end = trimmed.find(']')?;
    Some(trimmed[1..end].trim())
}

fn uncomment_restore_line(line: &str) -> String {
    line.strip_prefix("# ")
        .or_else(|| line.strip_prefix('#'))
        .unwrap_or(line)
        .to_string()
}

fn root_key_for_line(trimmed: &str) -> Option<&str> {
    if trimmed.is_empty() || trimmed.starts_with('#') {
        return None;
    }
    let (key, _) = trimmed.split_once('=')?;
    let key = key.trim();
    if key.contains('.') || key.is_empty() {
        return None;
    }
    Some(key)
}

fn parse_toml_string(value: &str) -> Option<String> {
    let doc = format!("value = {value}");
    let parsed: toml::Value = toml::from_str(&doc).ok()?;
    parsed.get("value")?.as_str().map(ToOwned::to_owned)
}

fn safe_config_key(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for ch in value.chars() {
        if ch.is_ascii_alphanumeric() || ch == '_' || ch == '-' {
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

fn ensure_trailing_newline(mut value: String) -> String {
    if !value.is_empty() && !value.ends_with('\n') {
        value.push('\n');
    }
    value
}

#[cfg(test)]
mod tests {
    use super::*;
    use ::common::profiles::schema::{AuthMode, ProfileDef};

    fn profile() -> ProfileDef {
        ProfileDef {
            id: "deepseek-main".to_string(),
            label: "DeepSeek".to_string(),
            provider: "deepseek".to_string(),
            auth_mode: AuthMode::ApiKey,
            api_types: vec!["openai-responses".to_string()],
            credentials: Default::default(),
            overrides: Default::default(),
            api_configs: Default::default(),
            use_settings_proxy: false,
            provider_settings: Default::default(),
            connections: Default::default(),
        }
    }

    fn overlay() -> CodexDesktopOverlay {
        let rendered = RenderedProfile {
            env: vec![("OPENAI_API_KEY".to_string(), "sk-test".to_string())],
            settings_files: Vec::new(),
            command_args: vec![
                "-c".to_string(),
                "model='deepseek-v4-pro'".to_string(),
                "-c".to_string(),
                "model_provider='deepseek'".to_string(),
                "-c".to_string(),
                "model_providers.deepseek.name='DeepSeek'".to_string(),
                "-c".to_string(),
                "model_providers.deepseek.base_url='https://api.deepseek.com/v1'".to_string(),
                "-c".to_string(),
                "model_providers.deepseek.wire_api='responses'".to_string(),
                "-c".to_string(),
                "model_providers.deepseek.supports_websockets=false".to_string(),
                "-c".to_string(),
                "model_providers.deepseek.env_key='OPENAI_API_KEY'".to_string(),
            ],
            config_env: None,
        };
        CodexDesktopOverlay::from_rendered(&profile(), "launch-123", &rendered, &rendered.env)
    }

    #[test]
    fn overlay_comments_existing_root_keys_and_uses_managed_provider() {
        let current = r#"model = "gpt-5-codex"
model_provider = "openai"

[mcp_servers.local]
url = "http://127.0.0.1:12358/va/mcp"
"#;

        let next = apply_overlay_to_string(current, &overlay());

        assert!(next.contains("# VIBEAROUND-CODEX-DESKTOP BEGIN RESTORE"));
        assert!(next.contains("# model = \"gpt-5-codex\""));
        assert!(next.contains("model_provider = 'vibearound_deepseek-main'"));
        assert!(next.contains("[model_providers.vibearound_deepseek-main]"));
        assert!(!next.contains("BEGIN PROVIDER"));
        assert!(!next.contains("END PROVIDER"));
        assert!(next.contains("supports_websockets = false"));
        assert!(next.contains("experimental_bearer_token = 'sk-test'"));
        assert!(!next.contains("env_key = 'OPENAI_API_KEY'"));
        assert!(next.contains("[mcp_servers.local]\nurl = \"http://127.0.0.1:12358/va/mcp\""));
    }

    #[test]
    fn overlay_keeps_existing_root_keys_before_provider_tables() {
        let current = r#"notify = ["echo", "done"]

[mcp_servers.local]
url = "http://127.0.0.1:12358/va/mcp"
"#;

        let next = apply_overlay_to_string(current, &overlay());
        let notify_index = next.find("notify = [\"echo\", \"done\"]").unwrap();
        let provider_index = next
            .find("[model_providers.vibearound_deepseek-main]")
            .unwrap();
        let mcp_index = next.find("[mcp_servers.local]").unwrap();
        let parsed: toml::Value = toml::from_str(&next).expect("overlay remains valid TOML");
        let provider = parsed
            .get("model_providers")
            .and_then(|providers| providers.get("vibearound_deepseek-main"))
            .expect("managed provider exists");

        assert!(notify_index < provider_index);
        assert!(provider_index < mcp_index);
        assert!(parsed.get("notify").is_some());
        assert!(provider.get("notify").is_none());
    }

    #[test]
    fn overlay_migrates_legacy_local_mcp_url_to_va_base() {
        let current = r#"[mcp_servers.local]
url = "http://127.0.0.1:12358/mcp?token=secret"

[mcp_servers.remote]
url = "https://example.com/mcp"
"#;

        let next = apply_overlay_to_string(current, &overlay());

        assert!(next.contains("url = \"http://127.0.0.1:12358/va/mcp?token=secret\""));
        assert!(!next.contains("127.0.0.1:12358/mcp"));
        assert!(next.contains("url = \"https://example.com/mcp\""));
    }

    #[test]
    fn cleanup_restores_previous_root_keys_and_removes_provider_block() {
        let with_overlay = apply_overlay_to_string("model = \"gpt-5-codex\"\n", &overlay());
        let cleaned = cleanup_vibearound_blocks(&with_overlay);

        assert!(cleaned.contains("model = \"gpt-5-codex\""));
        assert!(!cleaned.contains(MARKER));
        assert!(!cleaned.contains("[model_providers.vibearound_deepseek-main]"));
    }

    #[test]
    fn cleanup_removes_orphan_end_markers() {
        let current = r#"notify = ["echo", "done"]
# VIBEAROUND-CODEX-DESKTOP END PROVIDER

[model_providers.openai]
name = "OpenAI"
"#;

        let cleaned = cleanup_vibearound_blocks(current);

        assert!(cleaned.contains("notify = [\"echo\", \"done\"]"));
        assert!(cleaned.contains("[model_providers.openai]"));
        assert!(!cleaned.contains(MARKER));
    }

    #[test]
    fn cleanup_preserves_codex_notify_inserted_inside_provider_markers() {
        let current = r#"# VIBEAROUND-CODEX-DESKTOP BEGIN ACTIVE run=stale profile=deepseek-main
model = "deepseek-v4-pro"
model_provider = "vibearound_deepseek-main"
# VIBEAROUND-CODEX-DESKTOP END ACTIVE

# VIBEAROUND-CODEX-DESKTOP BEGIN PROVIDER run=stale profile=deepseek-main
notify = ["/Users/example/SkyComputerUseClient", "vibearound-profile-ended"]

[model_providers.vibearound_deepseek-main]
base_url = "http://127.0.0.1:12358/stale/v1"
experimental_bearer_token = "sk-stale"
supports_websockets = false
wire_api = "responses"
# VIBEAROUND-CODEX-DESKTOP END PROVIDER

[marketplaces.openai-bundled]
last_updated = "2026-06-21T07:38:57Z"
"#;

        let cleaned = cleanup_vibearound_blocks(current);
        let parsed: toml::Value = toml::from_str(&cleaned).expect("cleaned config is valid TOML");

        assert!(parsed.get("notify").is_some());
        assert!(cleaned.contains("vibearound-profile-ended"));
        assert!(cleaned.contains("[marketplaces.openai-bundled]"));
        assert!(!cleaned.contains(MARKER));
        assert!(!cleaned.contains("vibearound_deepseek-main"));
        assert!(!cleaned.contains("sk-stale"));
    }

    #[test]
    fn cleanup_preserves_codex_notify_inserted_inside_active_markers() {
        let current = r#"# VIBEAROUND-CODEX-DESKTOP BEGIN ACTIVE run=stale profile=deepseek-main
model = "deepseek-v4-pro"
notify = ["/Users/example/SkyComputerUseClient", "vibearound-profile-ended"]
model_provider = "vibearound_deepseek-main"
# VIBEAROUND-CODEX-DESKTOP END ACTIVE

[model_providers.openai]
name = "OpenAI"
"#;

        let cleaned = cleanup_vibearound_blocks(current);
        let parsed: toml::Value = toml::from_str(&cleaned).expect("cleaned config is valid TOML");

        assert!(parsed.get("notify").is_some());
        assert!(cleaned.contains("vibearound-profile-ended"));
        assert!(cleaned.contains("[model_providers.openai]"));
        assert!(!cleaned.contains(MARKER));
        assert!(!cleaned.contains("model = \"deepseek-v4-pro\""));
        assert!(!cleaned.contains("model_provider = \"vibearound_deepseek-main\""));
    }

    #[test]
    fn cleanup_removes_orphan_begin_markers_and_unmarked_vibearound_provider() {
        let current = r#"notify = ["echo", "done"]
# VIBEAROUND-CODEX-DESKTOP BEGIN PROVIDER run=stale profile=deepseek-main

[model_providers.vibearound_deepseek-main]
base_url = "http://127.0.0.1:12358/stale/v1"
experimental_bearer_token = "sk-stale"
supports_websockets = false
wire_api = "responses"

[model_providers.openai]
name = "OpenAI"
"#;

        let cleaned = cleanup_vibearound_blocks(current);

        assert!(cleaned.contains("notify = [\"echo\", \"done\"]"));
        assert!(cleaned.contains("[model_providers.openai]"));
        assert!(!cleaned.contains(MARKER));
        assert!(!cleaned.contains("vibearound_deepseek-main"));
        assert!(!cleaned.contains("sk-stale"));
    }

    #[test]
    fn cleanup_removes_unmarked_vibearound_root_overlay() {
        let current = r#"notify = ["echo", "done"]
model = "deepseek-v4-pro"
model_provider = "vibearound_deepseek-main"
model_reasoning_effort = "medium"
model_context_window = 1000000
model_catalog_json = "/Users/example/.vibearound/profile-state/deepseek/catalog.json"

[model_providers.openai]
name = "OpenAI"
"#;

        let cleaned = cleanup_vibearound_blocks(current);

        assert!(cleaned.contains("notify = [\"echo\", \"done\"]"));
        assert!(cleaned.contains("[model_providers.openai]"));
        assert!(!cleaned.contains("model = \"deepseek-v4-pro\""));
        assert!(!cleaned.contains("model_provider = \"vibearound_deepseek-main\""));
        assert!(!cleaned.contains("model_reasoning_effort"));
        assert!(!cleaned.contains("model_context_window"));
        assert!(!cleaned.contains("model_catalog_json"));
    }

    #[test]
    fn overlay_replaces_unmarked_stale_vibearound_provider() {
        let current = r#"notify = ["echo", "done"]

[model_providers.vibearound_deepseek-main]
base_url = "http://127.0.0.1:12358/stale/v1"
experimental_bearer_token = "sk-stale"
supports_websockets = false
wire_api = "responses"
"#;

        let next = apply_overlay_to_string(current, &overlay());

        assert_eq!(
            next.matches("[model_providers.vibearound_deepseek-main]")
                .count(),
            1
        );
        assert!(next.contains("notify = [\"echo\", \"done\"]"));
        assert!(next.contains("base_url = 'https://api.deepseek.com/v1'"));
        assert!(next.contains("experimental_bearer_token = 'sk-test'"));
        assert!(!next.contains("sk-stale"));
        assert!(!next.contains("12358/stale"));
    }

    #[test]
    fn overlay_is_idempotent_for_vibearound_blocks() {
        let first = apply_overlay_to_string("model = \"gpt-5-codex\"\n", &overlay());
        let second = apply_overlay_to_string(&first, &overlay());

        assert_eq!(
            second
                .matches("# VIBEAROUND-CODEX-DESKTOP BEGIN ACTIVE")
                .count(),
            1
        );
        assert_eq!(
            second
                .matches("[model_providers.vibearound_deepseek-main]")
                .count(),
            1
        );
    }

    #[test]
    fn cleanup_overlay_at_path_restores_config_for_direct_launch() {
        let dir = std::env::temp_dir().join(format!(
            "vibearound-codex-desktop-test-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&dir).expect("create temp dir");
        let path = dir.join("config.toml");
        let with_overlay = apply_overlay_to_string("model = \"gpt-5-codex\"\n", &overlay());
        std::fs::write(&path, with_overlay).expect("write test config");

        cleanup_overlay_at_path(&path).expect("cleanup overlay");

        let cleaned = std::fs::read_to_string(&path).expect("read cleaned config");
        assert!(cleaned.contains("model = \"gpt-5-codex\""));
        assert!(!cleaned.contains(MARKER));
        let _ = std::fs::remove_dir_all(dir);
    }
}
