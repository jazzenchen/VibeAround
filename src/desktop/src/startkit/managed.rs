use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use anyhow::anyhow;
use tauri::{AppHandle, Runtime};

use super::{
    base_report, current_platform, emit_progress, portable_toolchain_enabled, Manifest,
    StartkitChoices, StartkitItem, StartkitItemReport, StartkitItemStatus, StartkitProgress,
};

pub(in crate::startkit) async fn execute_managed_toolchain_item(
    manifest: &Manifest,
    item: &StartkitItem,
    choices: &StartkitChoices,
    cancelled: Option<&Arc<AtomicBool>>,
    progress: StartkitProgress<'_>,
) -> anyhow::Result<Option<StartkitItemReport>> {
    if !portable_toolchain_enabled(choices) {
        return Ok(None);
    }

    let mut log_progress = |line: String| {
        if let Some(progress) = progress {
            progress(item, StartkitItemStatus::Running, Some(line));
        }
    };
    let is_cancelled = || {
        cancelled
            .map(|flag| flag.load(Ordering::Relaxed))
            .unwrap_or(false)
    };

    let report = match item.id.as_str() {
        "essentials.node" => {
            if let Some(progress) = progress {
                progress(
                    item,
                    StartkitItemStatus::Running,
                    Some("Installing VibeAround portable Node.js".to_string()),
                );
            }
            let source = node_source_for_choices(manifest, choices)?;
            let status =
                common::toolchain::ensure_node_lts(&source, &mut log_progress, is_cancelled)
                    .await?;
            report_from_managed_tool_status(item, status)
        }
        "essentials.git" if current_platform() == "windows" => {
            if let Some(progress) = progress {
                progress(
                    item,
                    StartkitItemStatus::Running,
                    Some("Installing VibeAround portable Git".to_string()),
                );
            }
            let status =
                common::toolchain::ensure_windows_portable_git(&mut log_progress, is_cancelled)
                    .await?;
            report_from_managed_tool_status(item, status)
        }
        "essentials.git" => StartkitItemReport {
            status: StartkitItemStatus::Skipped,
            message: Some("Portable toolchain does not require Git on this platform".to_string()),
            actions: Vec::new(),
            ..base_report(item)
        },
        _ => return Ok(None),
    };

    Ok(Some(report))
}

pub(in crate::startkit) async fn run_managed_npm_package_item<R: Runtime>(
    app: &AppHandle<R>,
    run_id: Option<&str>,
    item: &StartkitItem,
    cancelled: &Arc<AtomicBool>,
) -> anyhow::Result<StartkitItemReport> {
    let before = scan_managed_npm_package_item(item);
    if !before.status.needs_install() {
        return Ok(before);
    }

    let Some(package) = item.npm_package.as_deref() else {
        return Ok(StartkitItemReport {
            status: StartkitItemStatus::Blocked,
            message: Some("No npm package is configured".to_string()),
            ..base_report(item)
        });
    };

    let install_dir = managed_item_dependency_dir(item)?;
    emit_progress(
        app,
        run_id,
        item,
        StartkitItemStatus::Running,
        Some(format!("Installing {}", item.label)),
        None,
    );

    common::agent::auto_install_npm_package_in_dir_with_progress_and_cancel(
        package,
        &install_dir,
        |line| {
            emit_progress(
                app,
                run_id,
                item,
                StartkitItemStatus::Running,
                Some(line),
                None,
            );
        },
        || cancelled.load(Ordering::Relaxed),
    )
    .await?;

    let after = scan_managed_npm_package_item(item);
    if matches!(after.status, StartkitItemStatus::Ok) {
        Ok(after)
    } else {
        Ok(StartkitItemReport {
            status: StartkitItemStatus::Error,
            message: Some(format!(
                "{} install finished, but it is still unavailable",
                item.label
            )),
            ..base_report(item)
        })
    }
}

pub(in crate::startkit) async fn scan_managed_toolchain_item(
    item: &StartkitItem,
    choices: &StartkitChoices,
    platform: &str,
) -> Option<StartkitItemReport> {
    if !portable_toolchain_enabled(choices) {
        return None;
    }

    let report = match item.id.as_str() {
        "essentials.node" => report_from_managed_tool_status(
            item,
            common::toolchain::managed_node_status(item.min_version.as_deref()).await,
        ),
        "essentials.git" if platform == "windows" => {
            report_from_managed_tool_status(item, common::toolchain::managed_git_status().await)
        }
        "essentials.git" => StartkitItemReport {
            status: StartkitItemStatus::Skipped,
            message: Some("Portable toolchain does not require Git on this platform".to_string()),
            actions: Vec::new(),
            ..base_report(item)
        },
        _ => return None,
    };

    Some(report)
}

fn report_from_managed_tool_status(
    item: &StartkitItem,
    status: common::toolchain::ManagedToolStatus,
) -> StartkitItemReport {
    let report_status = if status.ready {
        StartkitItemStatus::Ok
    } else if status.installed {
        StartkitItemStatus::Outdated
    } else {
        StartkitItemStatus::Missing
    };
    StartkitItemReport {
        status: report_status.clone(),
        version: status.version,
        path: status.path.map(|path| path.to_string_lossy().to_string()),
        message: status.message.or_else(|| {
            Some(if report_status == StartkitItemStatus::Ok {
                format!("{} is ready", item.label)
            } else {
                format!("{} will be installed by VibeAround", item.label)
            })
        }),
        actions: if report_status == StartkitItemStatus::Ok {
            Vec::new()
        } else {
            vec!["install".to_string()]
        },
        ..base_report(item)
    }
}

pub(in crate::startkit) fn scan_managed_npm_package_item(
    item: &StartkitItem,
) -> StartkitItemReport {
    let Some(package) = item.npm_package.as_deref() else {
        return StartkitItemReport {
            status: StartkitItemStatus::Blocked,
            message: Some("No npm package is configured".to_string()),
            ..base_report(item)
        };
    };
    let bin_name = item
        .program
        .as_deref()
        .map(str::to_string)
        .unwrap_or_else(|| common::agent::npm_package_bin_name(package));
    let Ok(install_dir) = managed_item_dependency_dir(item) else {
        return StartkitItemReport {
            status: StartkitItemStatus::Blocked,
            message: Some("No managed dependency directory is configured".to_string()),
            ..base_report(item)
        };
    };

    if common::agent::npm_package_installed_in_dir(package, &bin_name, &install_dir) {
        let bin_path = common::process::env::resolve_npm_bin_in_dir(&install_dir, &bin_name)
            .ok()
            .map(|path| path.to_string_lossy().to_string());
        return StartkitItemReport {
            status: StartkitItemStatus::Ok,
            path: bin_path,
            message: Some(format!("{} is ready", item.label)),
            actions: Vec::new(),
            ..base_report(item)
        };
    }

    StartkitItemReport {
        status: StartkitItemStatus::Missing,
        message: Some(format!("{} will be installed", item.label)),
        actions: vec!["install".to_string()],
        ..base_report(item)
    }
}

fn managed_item_dependency_dir(item: &StartkitItem) -> anyhow::Result<PathBuf> {
    let dependency_id = item
        .plugin_dependency
        .as_deref()
        .ok_or_else(|| anyhow!("managed item '{}' has no dependency id", item.id))?;
    Ok(common::plugins::user_plugin_dependency_dir(dependency_id))
}

fn node_source_for_choices(
    manifest: &Manifest,
    choices: &StartkitChoices,
) -> anyhow::Result<common::toolchain::NodeSource> {
    let source = manifest
        .sources
        .get(&choices.source)
        .or_else(|| manifest.sources.get("global"))
        .ok_or_else(|| anyhow!("startkit source '{}' not found", choices.source))?;
    Ok(common::toolchain::NodeSource {
        index_url: source.node_index.clone(),
        dist_base: source.node_dist.clone(),
    })
}

pub(in crate::startkit) fn item_uses_managed_dependency_dir(item: &StartkitItem) -> bool {
    item.managed && item.plugin_dependency.is_some()
}
