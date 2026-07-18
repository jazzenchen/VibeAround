use std::collections::HashSet;

use common::{agent, plugins, resources};
use serde::{Deserialize, Serialize};

use super::plugin_install::{run_install_inner, InstallPluginRequest};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ManagedPluginCategory {
    Im,
    Acp,
    Search,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ManagedPluginStatus {
    Ok,
    Missing,
    Outdated,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ManagedPluginSummary {
    pub category: ManagedPluginCategory,
    pub id: String,
    pub kind: String,
    pub name: String,
    pub description: String,
    pub status: ManagedPluginStatus,
    pub installed: bool,
    pub installable: bool,
    pub version: Option<String>,
    pub latest_version: Option<String>,
    pub source: Option<plugins::PluginSource>,
    pub path: Option<String>,
    pub github: Option<String>,
    pub message: Option<String>,
    pub actions: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ManagedPluginInstallRequest {
    pub category: ManagedPluginCategory,
    pub id: String,
}

#[tauri::command]
pub async fn list_managed_plugins() -> Result<Vec<ManagedPluginSummary>, String> {
    Ok(build_managed_plugins(false).await)
}

#[tauri::command]
pub async fn refresh_managed_plugins() -> Result<Vec<ManagedPluginSummary>, String> {
    Ok(build_managed_plugins(true).await)
}

#[tauri::command]
pub async fn install_managed_plugin(
    request: ManagedPluginInstallRequest,
) -> Result<ManagedPluginSummary, String> {
    match request.category {
        ManagedPluginCategory::Im => install_im_plugin(&request.id).await,
        ManagedPluginCategory::Acp => install_acp_plugin(&request.id).await,
        ManagedPluginCategory::Search => install_search_plugin(&request.id).await,
    }
}

async fn build_managed_plugins(include_latest: bool) -> Vec<ManagedPluginSummary> {
    let mut items = Vec::new();
    items.extend(im_plugins(include_latest).await);
    items.extend(acp_plugins());
    items.extend(search_plugins(include_latest).await);
    items.sort_by(|left, right| {
        left.category
            .label()
            .cmp(right.category.label())
            .then_with(|| left.name.cmp(&right.name))
    });
    items
}

async fn install_im_plugin(plugin_id: &str) -> Result<ManagedPluginSummary, String> {
    let plugin = resources::plugin_by_id(plugin_id)
        .ok_or_else(|| format!("unknown IM plugin '{plugin_id}'"))?;
    run_install_inner(InstallPluginRequest {
        plugin_id: plugin.id.clone(),
    })
    .await
    .map_err(|error| error.to_string())?;
    Ok(im_plugin_from_registry(plugin, false).await)
}

async fn install_acp_plugin(agent_id: &str) -> Result<ManagedPluginSummary, String> {
    let agent_def =
        resources::agent_by_id(agent_id).ok_or_else(|| format!("unknown agent '{agent_id}'"))?;
    let Some(npm_package) = agent_def.acp.npm_package.as_deref() else {
        return Err(format!(
            "agent '{agent_id}' does not use an installable ACP adapter"
        ));
    };
    let bin_name = acp_bin_name(agent_def);
    if !agent::npm_package_installed(npm_package, &bin_name) {
        agent::auto_install_npm_agent(npm_package)
            .await
            .map_err(|error| error.to_string())?;
    }
    Ok(acp_plugin(agent_def))
}

async fn install_search_plugin(plugin_id: &str) -> Result<ManagedPluginSummary, String> {
    let plugin = resources::plugin_by_id(plugin_id)
        .filter(|plugin| plugin.is_kind("search"))
        .ok_or_else(|| format!("unknown search plugin '{plugin_id}'"))?;
    let current = search_plugin_from_registry(plugin, false).await;
    if matches!(
        current.source.as_ref(),
        Some(plugins::PluginSource::Project)
    ) {
        return Ok(current);
    }
    run_install_inner(InstallPluginRequest {
        plugin_id: plugin.id.clone(),
    })
    .await
    .map_err(|error| error.to_string())?;
    Ok(search_plugin_from_registry(plugin, false).await)
}

async fn im_plugins(include_latest: bool) -> Vec<ManagedPluginSummary> {
    let registry = resources::PLUGINS
        .iter()
        .filter(|plugin| plugin.is_kind("channel"));
    let mut seen = HashSet::new();
    let mut items = Vec::new();

    for plugin in registry {
        seen.insert(plugin.id.clone());
        items.push(im_plugin_from_registry(plugin, include_latest).await);
    }

    for discovered in plugins::channel::list_summaries() {
        if seen.contains(&discovered.id) {
            continue;
        }
        items.push(ManagedPluginSummary {
            category: ManagedPluginCategory::Im,
            id: discovered.id.clone(),
            kind: discovered.kind.clone(),
            name: discovered.name.clone(),
            description: format!("{} plugin", discovered.kind),
            status: ManagedPluginStatus::Ok,
            installed: true,
            installable: false,
            version: Some(discovered.version.clone()),
            latest_version: None,
            source: Some(discovered.source.clone()),
            path: Some(discovered.entry.clone()),
            github: None,
            message: Some("Plugin is installed".to_string()),
            actions: Vec::new(),
        });
    }

    items
}

async fn search_plugins(include_latest: bool) -> Vec<ManagedPluginSummary> {
    let registry = resources::PLUGINS
        .iter()
        .filter(|plugin| plugin.is_kind("search"));
    let mut seen = HashSet::new();
    let mut items = Vec::new();

    for plugin in registry {
        seen.insert(plugin.id.clone());
        items.push(search_plugin_from_registry(plugin, include_latest).await);
    }

    for discovered in plugins::discover_plugins()
        .into_values()
        .filter(|plugin| plugin.manifest.kind == "search")
    {
        if seen.contains(&discovered.manifest.id) {
            continue;
        }
        let summary = plugins::DiscoveredPluginSummary::from(&discovered);
        items.push(ManagedPluginSummary {
            category: ManagedPluginCategory::Search,
            id: summary.id,
            kind: "Search runtime".to_string(),
            name: summary.name,
            description: "Host-side web search runtime".to_string(),
            status: ManagedPluginStatus::Ok,
            installed: true,
            installable: false,
            version: Some(summary.version),
            latest_version: None,
            source: Some(summary.source),
            path: Some(discovered.entry_path().to_string_lossy().to_string()),
            github: None,
            message: Some("Search runtime is installed".to_string()),
            actions: Vec::new(),
        });
    }

    items
}

async fn im_plugin_from_registry(
    plugin: &resources::PluginDef,
    include_latest: bool,
) -> ManagedPluginSummary {
    let discovered = plugins::channel::find(&plugin.id);
    let version = discovered.as_ref().map(|plugin| plugin.installed_version());
    let latest = if include_latest {
        super::github_plugin_version(&plugin.github)
            .await
            .ok()
            .flatten()
    } else {
        None
    };
    let installed = discovered.is_some();
    let outdated = installed
        && matches!((&latest, &version), (Some(latest), Some(version)) if latest != version);
    let status = if outdated {
        ManagedPluginStatus::Outdated
    } else if installed {
        ManagedPluginStatus::Ok
    } else {
        ManagedPluginStatus::Missing
    };
    let action = if status == ManagedPluginStatus::Outdated {
        "update"
    } else if installed {
        "refresh"
    } else {
        "install"
    };

    ManagedPluginSummary {
        category: ManagedPluginCategory::Im,
        id: plugin.id.clone(),
        kind: plugin.kind.clone(),
        name: plugin.name.clone(),
        description: plugin.description.clone(),
        status,
        installed,
        installable: true,
        version,
        latest_version: latest,
        source: discovered.as_ref().map(|plugin| plugin.source.clone()),
        path: discovered
            .as_ref()
            .map(|plugin| plugin.entry_path().to_string_lossy().to_string()),
        github: Some(plugin.github.clone()),
        message: Some(if installed {
            "Plugin is installed".to_string()
        } else {
            "Plugin is not installed".to_string()
        }),
        actions: vec![action.to_string()],
    }
}

async fn search_plugin_from_registry(
    plugin: &resources::PluginDef,
    include_latest: bool,
) -> ManagedPluginSummary {
    let discovered = plugins::find(&plugin.id);
    let version = discovered.as_ref().map(|plugin| plugin.installed_version());
    let latest = if include_latest {
        super::github_plugin_version(&plugin.github)
            .await
            .ok()
            .flatten()
    } else {
        None
    };
    let installed = discovered.is_some();
    let outdated = installed
        && matches!((&latest, &version), (Some(latest), Some(version)) if latest != version);
    let status = if outdated {
        ManagedPluginStatus::Outdated
    } else if installed {
        ManagedPluginStatus::Ok
    } else {
        ManagedPluginStatus::Missing
    };
    let action = if status == ManagedPluginStatus::Outdated {
        "update"
    } else if installed {
        "refresh"
    } else {
        "install"
    };

    ManagedPluginSummary {
        category: ManagedPluginCategory::Search,
        id: plugin.id.clone(),
        kind: "Search runtime".to_string(),
        name: plugin.name.clone(),
        description: plugin.description.clone(),
        status,
        installed,
        installable: true,
        version,
        latest_version: latest,
        source: discovered.as_ref().map(|plugin| plugin.source.clone()),
        path: discovered
            .as_ref()
            .map(|plugin| plugin.entry_path().to_string_lossy().to_string()),
        github: Some(plugin.github.clone()),
        message: Some(if installed {
            "Search runtime is installed".to_string()
        } else {
            "Search runtime is not installed".to_string()
        }),
        actions: vec![action.to_string()],
    }
}

fn acp_plugins() -> Vec<ManagedPluginSummary> {
    resources::AGENTS
        .iter()
        .filter(|agent| agent.supports_current_platform())
        .filter(|agent| agent.acp.npm_package.is_some())
        .map(acp_plugin)
        .collect()
}

fn acp_plugin(agent_def: &resources::AgentDef) -> ManagedPluginSummary {
    let npm_package = agent_def
        .acp
        .npm_package
        .as_deref()
        .expect("acp_plugin only handles npm adapters");
    let bin_name = acp_bin_name(agent_def);
    let installed = agent::npm_package_installed(npm_package, &bin_name);
    ManagedPluginSummary {
        category: ManagedPluginCategory::Acp,
        id: agent_def.id.clone(),
        kind: "ACP adapter".to_string(),
        name: format!("{} ACP", agent_def.display_name),
        description: format!("Adapter package {npm_package}"),
        status: if installed {
            ManagedPluginStatus::Ok
        } else {
            ManagedPluginStatus::Missing
        },
        installed,
        installable: true,
        version: None,
        latest_version: None,
        source: None,
        path: agent::npm_package_installed(npm_package, &bin_name)
            .then(|| common::process::env::resolve_acp_agent_bin(&bin_name).ok())
            .flatten()
            .map(|path| path.to_string_lossy().to_string()),
        github: None,
        message: Some(if installed {
            "ACP adapter is installed".to_string()
        } else {
            "ACP adapter is not installed".to_string()
        }),
        actions: if installed {
            vec!["refresh".to_string()]
        } else {
            vec!["install".to_string()]
        },
    }
}

fn acp_bin_name(agent_def: &resources::AgentDef) -> String {
    let npm_package = agent_def
        .acp
        .npm_package
        .as_deref()
        .expect("npm ACP adapter");
    agent_def
        .acp
        .bin_name
        .clone()
        .unwrap_or_else(|| agent::npm_package_bin_name(npm_package))
}

impl ManagedPluginCategory {
    fn label(self) -> &'static str {
        match self {
            Self::Acp => "acp",
            Self::Im => "im",
            Self::Search => "search",
        }
    }
}
