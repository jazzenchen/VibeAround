//! MCP server entry install/uninstall for an agent's settings.
//!
//! Supports both JSON (Claude Code, Gemini, Cursor, Kiro, Qwen) and TOML
//! (Codex) formats. Global helpers are kept only for cleanup/migration;
//! runtime installation should prefer project-scoped workspace files.

use std::path::{Path, PathBuf};

use anyhow::{anyhow, Context};

use crate::{config, resources};

/// Merge VibeAround MCP server entry into an agent's global settings.
/// Supports JSON (default) and TOML formats. Also writes to legacy path
/// if configured.
pub(super) fn install_mcp_config(agent: &str, mcp_url: &str) -> anyhow::Result<()> {
    let home = home_dir()?;

    let agent_def = match resources::agent_by_id(agent) {
        Some(def) => def,
        None => return Ok(()),
    };
    let global_config = match &agent_def.global_config {
        Some(cfg) => cfg,
        None => return Ok(()),
    };

    let config_path = home.join(&global_config.settings_path);
    install_mcp_config_at_path(agent, global_config, &config_path, mcp_url)?;

    // Also write to legacy path for backward compat (e.g. older Claude Code versions)
    if let Some(legacy) = &global_config.settings_path_legacy {
        let legacy_path = home.join(legacy);
        let _ = install_mcp_config_json(
            &legacy_path,
            &global_config.mcp_key,
            &global_config.mcp_entry,
            mcp_url,
            agent,
        );
    }

    Ok(())
}

/// Merge VibeAround MCP server entry into an agent's project/workspace settings.
pub(super) fn install_project_mcp_config(
    agent: &str,
    workspace: &Path,
    mcp_url: &str,
) -> anyhow::Result<()> {
    let agent_def = match resources::agent_by_id(agent) {
        Some(def) => def,
        None => return Ok(()),
    };
    let global_config = match &agent_def.global_config {
        Some(cfg) => cfg,
        None => return Ok(()),
    };

    let config_path = workspace.join(project_mcp_settings_path(agent, global_config));
    install_mcp_config_at_path(agent, global_config, &config_path, mcp_url)
}

/// Remove VibeAround MCP server entry from an agent's global settings.
pub(super) fn uninstall_mcp_config(agent: &str) -> anyhow::Result<()> {
    let home = home_dir()?;

    let agent_def = match resources::agent_by_id(agent) {
        Some(def) => def,
        None => return Ok(()),
    };
    let global_config = match &agent_def.global_config {
        Some(cfg) => cfg,
        None => return Ok(()),
    };

    let config_path = home.join(&global_config.settings_path);
    uninstall_mcp_config_at_path(agent, global_config, &config_path)?;

    if let Some(legacy) = &global_config.settings_path_legacy {
        let legacy_path = home.join(legacy);
        uninstall_mcp_config_json(&legacy_path, &global_config.mcp_key, agent)?;
    }

    Ok(())
}

/// Remove VibeAround MCP server entry from an agent's project/workspace settings.
pub(super) fn uninstall_project_mcp_config(agent: &str, workspace: &Path) -> anyhow::Result<()> {
    let agent_def = match resources::agent_by_id(agent) {
        Some(def) => def,
        None => return Ok(()),
    };
    let global_config = match &agent_def.global_config {
        Some(cfg) => cfg,
        None => return Ok(()),
    };

    let config_path = workspace.join(project_mcp_settings_path(agent, global_config));
    uninstall_mcp_config_at_path(agent, global_config, &config_path)
}

fn install_mcp_config_at_path(
    agent: &str,
    global_config: &resources::AgentGlobalConfig,
    config_path: &Path,
    mcp_url: &str,
) -> anyhow::Result<()> {
    if is_toml_format(global_config) {
        install_mcp_config_toml(
            config_path,
            &global_config.mcp_key,
            &global_config.mcp_entry,
            mcp_url,
            agent,
        )
    } else {
        install_mcp_config_json(
            config_path,
            &global_config.mcp_key,
            &global_config.mcp_entry,
            mcp_url,
            agent,
        )
    }
}

fn uninstall_mcp_config_at_path(
    agent: &str,
    global_config: &resources::AgentGlobalConfig,
    config_path: &Path,
) -> anyhow::Result<()> {
    if is_toml_format(global_config) {
        uninstall_mcp_config_toml(config_path, &global_config.mcp_key, agent)
    } else {
        uninstall_mcp_config_json(config_path, &global_config.mcp_key, agent)
    }
}

fn project_mcp_settings_path(agent: &str, global_config: &resources::AgentGlobalConfig) -> PathBuf {
    match agent {
        // Claude Code project-scoped MCP is shared through workspace .mcp.json.
        "claude" => PathBuf::from(".mcp.json"),
        _ => PathBuf::from(&global_config.settings_path),
    }
}

/// Check if the agent uses TOML config format.
fn is_toml_format(global_config: &resources::AgentGlobalConfig) -> bool {
    global_config.settings_format.as_deref() == Some("toml")
}

pub(super) fn home_dir() -> anyhow::Result<PathBuf> {
    let dir = config::home_dir();
    if dir.as_os_str() == "/tmp" {
        anyhow::bail!("Cannot determine home directory");
    }
    Ok(dir)
}

fn install_mcp_config_json(
    config_path: &Path,
    mcp_key: &str,
    mcp_entry_template: &serde_json::Value,
    mcp_url: &str,
    agent: &str,
) -> anyhow::Result<()> {
    if let Some(parent) = config_path.parent() {
        std::fs::create_dir_all(parent).with_context(|| format!("Create {:?}", parent))?;
    }

    // Substitute placeholders in the entry template
    let mcp_value_str = serde_json::to_string(mcp_entry_template).context("serialize mcp_entry")?;
    let mcp_value: serde_json::Value =
        serde_json::from_str(&mcp_value_str.replace("{mcp_url}", mcp_url))
            .context("parse mcp_entry after substitution")?;

    let mut root: serde_json::Value = match std::fs::read_to_string(config_path) {
        Ok(data) => {
            serde_json::from_str(&data).with_context(|| format!("Parse JSON {:?}", config_path))?
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => serde_json::json!({}),
        Err(error) => return Err(error).with_context(|| format!("Read {:?}", config_path)),
    };

    // Always replace (full replace on every startup)
    let obj = root
        .as_object_mut()
        .ok_or_else(|| anyhow!("{:?} root is not a JSON object", config_path))?;
    let servers = obj.entry(mcp_key).or_insert_with(|| serde_json::json!({}));
    let servers_obj = servers
        .as_object_mut()
        .ok_or_else(|| anyhow!("{} is not an object in {:?}", mcp_key, config_path))?;
    servers_obj.insert("vibearound".to_string(), mcp_value);

    let pretty = serde_json::to_string_pretty(&root).context("JSON serialize")?;
    std::fs::write(config_path, pretty).with_context(|| format!("Write {:?}", config_path))?;

    tracing::info!(
        "[integrations] Installed MCP config for {} at {:?}",
        agent,
        config_path
    );
    Ok(())
}

fn install_mcp_config_toml(
    config_path: &Path,
    mcp_key: &str,
    mcp_entry_template: &serde_json::Value,
    mcp_url: &str,
    agent: &str,
) -> anyhow::Result<()> {
    use toml_edit::{DocumentMut, Item, Table};

    if let Some(parent) = config_path.parent() {
        std::fs::create_dir_all(parent).with_context(|| format!("Create {:?}", parent))?;
    }

    // Substitute placeholders in the entry template
    let mcp_value_str = serde_json::to_string(mcp_entry_template).context("serialize mcp_entry")?;
    let substituted = mcp_value_str.replace("{mcp_url}", mcp_url);
    let mcp_value: serde_json::Value =
        serde_json::from_str(&substituted).context("parse mcp_entry after substitution")?;

    let data = match std::fs::read_to_string(config_path) {
        Ok(data) => data,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(error) => return Err(error).with_context(|| format!("Read {:?}", config_path)),
    };
    let mut doc: DocumentMut = data
        .parse::<DocumentMut>()
        .with_context(|| format!("Parse TOML {:?}", config_path))?;

    // Ensure [mcp_key] table exists (e.g. [mcp_servers])
    if !doc.contains_key(mcp_key) {
        doc[mcp_key] = Item::Table(Table::new());
    }

    // Create the [mcp_key.vibearound] sub-table
    let servers = doc[mcp_key]
        .as_table_mut()
        .ok_or_else(|| anyhow!("{} is not a table in {:?}", mcp_key, config_path))?;

    let mut entry_table = Table::new();
    if let Some(obj) = mcp_value.as_object() {
        for (k, v) in obj {
            match v {
                serde_json::Value::String(s) => {
                    entry_table[k.as_str()] = toml_edit::value(s.as_str());
                }
                serde_json::Value::Bool(b) => {
                    entry_table[k.as_str()] = toml_edit::value(*b);
                }
                serde_json::Value::Number(n) => {
                    if let Some(i) = n.as_i64() {
                        entry_table[k.as_str()] = toml_edit::value(i);
                    } else if let Some(f) = n.as_f64() {
                        entry_table[k.as_str()] = toml_edit::value(f);
                    }
                }
                _ => {} // skip complex values
            }
        }
    }

    servers["vibearound"] = Item::Table(entry_table);

    std::fs::write(config_path, doc.to_string())
        .with_context(|| format!("Write {:?}", config_path))?;

    tracing::info!(
        "[integrations] Installed MCP config for {} at {:?} (TOML)",
        agent,
        config_path
    );
    Ok(())
}

fn uninstall_mcp_config_toml(config_path: &Path, mcp_key: &str, agent: &str) -> anyhow::Result<()> {
    use toml_edit::DocumentMut;

    if !config_path.exists() {
        return Ok(());
    }

    let data =
        std::fs::read_to_string(config_path).with_context(|| format!("Read {:?}", config_path))?;
    let mut doc: DocumentMut = data
        .parse::<DocumentMut>()
        .with_context(|| format!("Parse TOML {:?}", config_path))?;

    let mut changed = false;
    if let Some(servers) = doc.get_mut(mcp_key).and_then(|v| v.as_table_mut()) {
        if servers.remove("vibearound").is_some() {
            changed = true;
        }
    }

    if changed {
        std::fs::write(config_path, doc.to_string())
            .with_context(|| format!("Write {:?}", config_path))?;
        tracing::info!(
            "[integrations] Removed MCP config for {} at {:?} (TOML)",
            agent,
            config_path
        );
    }

    Ok(())
}

fn uninstall_mcp_config_json(config_path: &Path, mcp_key: &str, agent: &str) -> anyhow::Result<()> {
    if !config_path.exists() {
        return Ok(());
    }

    let data =
        std::fs::read_to_string(config_path).with_context(|| format!("Read {:?}", config_path))?;
    let mut root: serde_json::Value =
        serde_json::from_str(&data).with_context(|| format!("Parse JSON {:?}", config_path))?;

    let mut changed = false;
    if let Some(obj) = root.as_object_mut() {
        if let Some(servers) = obj.get_mut(mcp_key) {
            if let Some(servers_obj) = servers.as_object_mut() {
                let managed_keys: Vec<String> = servers_obj
                    .iter()
                    .filter(|(key, value)| {
                        key.as_str() == "vibearound" || is_legacy_vibearound_managed(value)
                    })
                    .map(|(key, _)| key.clone())
                    .collect();
                for key in managed_keys {
                    servers_obj.remove(&key);
                    changed = true;
                }
            }
        }
    }

    if changed {
        let pretty = serde_json::to_string_pretty(&root).context("JSON serialize")?;
        std::fs::write(config_path, pretty).with_context(|| format!("Write {:?}", config_path))?;
        tracing::info!(
            "[integrations] Removed MCP config for {} at {:?}",
            agent,
            config_path
        );
    }

    Ok(())
}

fn is_legacy_vibearound_managed(value: &serde_json::Value) -> bool {
    value.get("metadata").and_then(|v| v.as_str()) == Some("vibearound")
        || value
            .get("_vibearound")
            .and_then(|m| m.get("managed"))
            .and_then(|v| v.as_bool())
            .unwrap_or(false)
}
