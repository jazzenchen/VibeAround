//! Install orchestration: runs agents + plugin installs with progress reporting.
//!
//! ## Module layout
//!
//! - [`util`]   — progress-event emission + log-file append + enabled-agent
//!                resolution helpers.
//! - [`steps`]  — per-step executors (`install_npm_agent`,
//!                `install_script_agent`, `install_channel_plugin`).
//!
//! The top-level `get_install_manifest`, `start`, and `cancel` functions
//! live in this file and drive `run_install` — the background task that
//! installs CLI packages and channel plugins. Project MCP/skill files are
//! installed lazily for the active workspace at launch time.

mod steps;
mod util;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use serde_json::Value;
use tauri::{AppHandle, Emitter, Runtime};
use tokio::sync::Mutex;

use common::config;

use crate::onboarding::{write_settings_value, InstallTaskInfo, OnboardingInstallState};

use steps::{install_channel_plugin, install_npm_agent, install_script_agent};
use util::{log_line, resolve_enabled_agents};

#[derive(Debug, Clone, Copy, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstallScope {
    #[serde(default = "default_true")]
    pub agents: bool,
    #[serde(default = "default_true")]
    pub channels: bool,
}

impl Default for InstallScope {
    fn default() -> Self {
        Self {
            agents: true,
            channels: true,
        }
    }
}

fn default_true() -> bool {
    true
}

/// Returns the list of install tasks for the given settings, so the frontend
/// can pre-populate the progress list before install starts.
pub fn get_install_manifest(settings: &Value, scope: InstallScope) -> Vec<InstallTaskInfo> {
    let _agent_setup_requested = scope.agents;
    let all_agents = common::resources::agent_ids();
    let enabled_channels = if scope.channels {
        enabled_registry_channel_ids(settings)
    } else {
        Vec::new()
    };
    let needs_acp_agents = !enabled_channels.is_empty();
    let acp_agents = if needs_acp_agents {
        resolve_enabled_agents(settings, &all_agents)
    } else {
        Vec::new()
    };

    let mut tasks = Vec::new();

    for agent_id in &acp_agents {
        let agent_def = match common::resources::agent_by_id(agent_id) {
            Some(def) => def,
            None => continue,
        };
        // ACP agent install is only needed when at least one IM/channel is enabled.
        let install_type = agent_def.install.as_ref().map(|i| i.install_type.as_str());
        if matches!(install_type, Some("npm") | Some("script")) {
            tasks.push(InstallTaskInfo {
                id: format!("agent:{}:acp", agent_id),
                label: format!("{} — CLI install", agent_def.display_name),
            });
        }
    }

    // Channel plugins
    for channel_id in &enabled_channels {
        let plugin_def = common::resources::plugin_by_id(channel_id);
        let label = plugin_def
            .map(|p| p.name.clone())
            .unwrap_or_else(|| channel_id.clone());
        tasks.push(InstallTaskInfo {
            id: format!("plugin:{}", channel_id),
            label: format!("{} — Plugin install", label),
        });
    }

    tasks
}

/// Start the install, saving settings first, then spawning the background task.
pub async fn start<R: Runtime>(
    app: AppHandle<R>,
    install_state: &OnboardingInstallState,
    settings: Value,
    scope: InstallScope,
) -> Result<(), String> {
    install_state.cancelled.store(false, Ordering::Relaxed);

    // Save settings early so install steps can read credentials/config, but
    // do not mark onboarding complete yet. If the user cancels or the app
    // quits mid-install, the next launch must return to onboarding instead
    // of treating a partial install as ready.
    let val = settings;
    write_settings_value(&val)?;

    // Create log file
    let log_dir = config::data_dir().join("logs").join("onboarding");
    std::fs::create_dir_all(&log_dir).map_err(|e| e.to_string())?;
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let log_path = log_dir.join(format!("{}.log", timestamp));
    let log_file = std::fs::File::create(&log_path).map_err(|e| e.to_string())?;
    {
        let mut lf = install_state.log_file.lock().await;
        *lf = Some(log_file);
    }

    let cancelled = Arc::clone(&install_state.cancelled);
    let child_proc = Arc::clone(&install_state.child_process);
    let log_file_arc = Arc::clone(&install_state.log_file);

    tauri::async_runtime::spawn(async move {
        run_install(app, val, scope, cancelled, child_proc, log_file_arc).await;
    });

    Ok(())
}

/// Cancel a running install.
pub async fn cancel(install_state: &OnboardingInstallState) -> Result<(), String> {
    install_state.cancelled.store(true, Ordering::Relaxed);
    let mut child = install_state.child_process.lock().await;
    if let Some(ref mut proc) = *child {
        let _ = proc.kill().await;
    }
    *child = None;
    Ok(())
}

async fn run_install<R: Runtime>(
    app: AppHandle<R>,
    settings: Value,
    scope: InstallScope,
    cancelled: Arc<AtomicBool>,
    _child_proc: Arc<Mutex<Option<tokio::process::Child>>>,
    log_file: Arc<Mutex<Option<std::fs::File>>>,
) {
    let all_agents = common::resources::agent_ids();
    let enabled_channels = if scope.channels {
        enabled_registry_channel_ids(&settings)
    } else {
        Vec::new()
    };
    let needs_acp_agents = !enabled_channels.is_empty();
    let acp_agents = if needs_acp_agents {
        resolve_enabled_agents(&settings, &all_agents)
    } else {
        Vec::new()
    };
    let mut had_error = false;

    // --- ACP agent installs ---
    for agent_id in &acp_agents {
        if cancelled.load(Ordering::Relaxed) {
            break;
        }

        let agent_def = match common::resources::agent_by_id(agent_id) {
            Some(def) => def,
            None => continue,
        };

        // ACP agent install (npm or script) is only needed for IM/channel use.
        let install_type = agent_def.install.as_ref().map(|i| i.install_type.as_str());
        match install_type {
            Some("npm") => {
                install_npm_agent(
                    &app,
                    agent_id,
                    agent_def,
                    &log_file,
                    &cancelled,
                    &mut had_error,
                )
                .await
            }
            Some("script") => {
                install_script_agent(&app, agent_id, agent_def, &log_file, &mut had_error).await
            }
            _ => {} // "path" type — nothing to install
        }
    }

    // --- Channel plugin installs ---
    for channel_id in &enabled_channels {
        if cancelled.load(Ordering::Relaxed) {
            break;
        }
        install_channel_plugin(&app, channel_id, &log_file, &cancelled, &mut had_error).await;
    }

    // Emit final complete event
    let final_status = if cancelled.load(Ordering::Relaxed) {
        "cancelled"
    } else if had_error {
        "error"
    } else {
        "complete"
    };

    let _ = app.emit(
        "onboarding-install-complete",
        serde_json::json!({
            "status": final_status,
        }),
    );

    log_line(
        &log_file,
        &format!("[onboarding] Install finished with status: {final_status}"),
    );

    let mut lf = log_file.lock().await;
    *lf = None;
}

fn enabled_channel_ids(settings: &Value) -> Vec<String> {
    settings
        .get("channels")
        .and_then(|v| v.as_object())
        .map(|obj| obj.keys().cloned().collect::<Vec<_>>())
        .unwrap_or_default()
}

fn enabled_registry_channel_ids(settings: &Value) -> Vec<String> {
    // settings.channels can also hold internal channel config such as web/ws;
    // only registry-backed channel plugins have onboarding install tasks.
    enabled_channel_ids(settings)
        .into_iter()
        .filter(|id| {
            matches!(
                common::resources::plugin_by_id(id),
                Some(plugin) if plugin.is_kind("channel")
            )
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manifest_ids(settings: Value, scope: InstallScope) -> Vec<String> {
        get_install_manifest(&settings, scope)
            .into_iter()
            .map(|task| task.id)
            .collect()
    }

    #[test]
    fn manifest_installs_configured_acp_agents_when_only_channels_are_in_scope() {
        let settings = serde_json::json!({
            "enabled_agents": ["claude", "codex"],
            "channels": {
                "telegram": {}
            }
        });

        let ids = manifest_ids(
            settings,
            InstallScope {
                agents: false,
                channels: true,
            },
        );

        assert!(ids.contains(&"agent:claude:acp".to_string()));
        assert!(ids.contains(&"agent:codex:acp".to_string()));
        assert!(ids.contains(&"plugin:telegram".to_string()));
        assert!(!ids.contains(&"agent:claude:mcp".to_string()));
        assert!(!ids.contains(&"agent:codex:mcp".to_string()));
    }

    #[test]
    fn manifest_installs_configured_pi_acp_agent() {
        let settings = serde_json::json!({
            "enabled_agents": ["pi"],
            "channels": {
                "telegram": {}
            }
        });

        let ids = manifest_ids(
            settings,
            InstallScope {
                agents: false,
                channels: true,
            },
        );

        assert!(ids.contains(&"agent:pi:acp".to_string()));
        assert!(ids.contains(&"plugin:telegram".to_string()));
        assert!(!ids.contains(&"agent:pi:mcp".to_string()));
    }

    #[test]
    fn manifest_does_not_install_acp_agents_without_channels() {
        let settings = serde_json::json!({
            "enabled_agents": ["claude", "codex"]
        });

        let ids = manifest_ids(
            settings,
            InstallScope {
                agents: true,
                channels: false,
            },
        );

        assert!(!ids.contains(&"agent:claude:acp".to_string()));
        assert!(!ids.contains(&"agent:codex:acp".to_string()));
        assert!(!ids.contains(&"agent:claude:mcp".to_string()));
        assert!(!ids.contains(&"agent:codex:mcp".to_string()));
        assert!(ids.is_empty());
    }

    #[test]
    fn manifest_ignores_internal_channels_for_plugin_install() {
        let settings = serde_json::json!({
            "enabled_agents": ["claude", "codex"],
            "channels": {
                "web": { "verbose": { "show_thinking": true } },
                "ws": { "verbose": { "show_tool_use": true } },
                "onboarding": {},
                "telegram": {}
            }
        });

        let ids = manifest_ids(
            settings,
            InstallScope {
                agents: false,
                channels: true,
            },
        );

        assert!(ids.contains(&"plugin:telegram".to_string()));
        assert!(ids.contains(&"agent:claude:acp".to_string()));
        assert!(ids.contains(&"agent:codex:acp".to_string()));
        assert!(!ids.contains(&"plugin:web".to_string()));
        assert!(!ids.contains(&"plugin:ws".to_string()));
        assert!(!ids.contains(&"plugin:onboarding".to_string()));
    }

    #[test]
    fn manifest_does_not_install_acp_agents_for_internal_channels_only() {
        let settings = serde_json::json!({
            "enabled_agents": ["claude", "codex"],
            "channels": {
                "web": { "verbose": { "show_thinking": true } },
                "ws": { "verbose": { "show_tool_use": true } }
            }
        });

        let ids = manifest_ids(
            settings,
            InstallScope {
                agents: false,
                channels: true,
            },
        );

        assert!(!ids.contains(&"agent:claude:acp".to_string()));
        assert!(!ids.contains(&"agent:codex:acp".to_string()));
        assert!(!ids.contains(&"plugin:web".to_string()));
        assert!(!ids.contains(&"plugin:ws".to_string()));
        assert!(ids.is_empty());
    }
}
