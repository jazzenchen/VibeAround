use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use crate::agent_detection;

use super::{
    base_report, is_managed_mode, StartkitChoices, StartkitItem, StartkitItemReport,
    StartkitItemStatus, StartkitProgress,
};

pub(in crate::startkit) async fn execute_agent_cli_item(
    item: &StartkitItem,
    agent_id: &str,
    choices: &StartkitChoices,
    cancelled: Option<&Arc<AtomicBool>>,
    progress: StartkitProgress<'_>,
) -> anyhow::Result<StartkitItemReport> {
    let before = scan_agent_cli_item(item, agent_id, choices).await;
    if !before.status.needs_install() {
        return Ok(before);
    }

    let Some(package) = agent_cli_npm_install_package(agent_id) else {
        return Ok(StartkitItemReport {
            status: StartkitItemStatus::Blocked,
            message: Some("No automatic install action is available".to_string()),
            ..base_report(item)
        });
    };

    if let Some(progress) = progress {
        progress(
            item,
            StartkitItemStatus::Running,
            Some(format!("Installing {}", item.label)),
        );
    }

    let log_progress = |line| {
        if let Some(progress) = progress {
            progress(item, StartkitItemStatus::Running, Some(line));
        }
    };
    let is_cancelled = || {
        cancelled
            .map(|flag| flag.load(Ordering::Relaxed))
            .unwrap_or(false)
    };

    let result = if is_managed_mode(choices) {
        let install_dir = common::process::env::acp_agents_dir();
        common::agent::auto_install_npm_package_in_dir_with_progress_and_cancel(
            &package,
            &install_dir,
            log_progress,
            is_cancelled,
        )
        .await
    } else {
        common::agent::auto_install_npm_global_package_with_progress_and_cancel(
            &package,
            log_progress,
            is_cancelled,
        )
        .await
    };

    if let Err(error) = result {
        return Ok(StartkitItemReport {
            status: StartkitItemStatus::Error,
            message: Some(error.to_string()),
            ..base_report(item)
        });
    }

    let after = scan_agent_cli_item(item, agent_id, choices).await;
    if matches!(after.status, StartkitItemStatus::Ok) {
        Ok(after)
    } else {
        Ok(StartkitItemReport {
            status: StartkitItemStatus::Error,
            message: Some(format!(
                "{} install finished, but the CLI is still unavailable{}",
                item.label,
                after
                    .message
                    .as_deref()
                    .map(|message| format!(": {message}"))
                    .unwrap_or_default()
            )),
            ..base_report(item)
        })
    }
}

pub(in crate::startkit) fn agent_cli_npm_install_package(agent_id: &str) -> Option<String> {
    if !agent_detection::agent_uses_npm_install(agent_id) {
        return None;
    }
    agent_detection::source_package(agent_id, "npm_global")
}

pub(in crate::startkit) async fn scan_agent_cli_item(
    item: &StartkitItem,
    agent_id: &str,
    choices: &StartkitChoices,
) -> StartkitItemReport {
    let selected = common::agent_availability::resolve_agent_availability(
        agent_id,
        common::agent_availability::AgentAvailabilityRequest {
            scan_policy: common::agent_availability::AgentScanPolicy::RefreshIfUnconfigured,
            toolchain_mode: &choices.toolchain_mode,
            candidate_preference:
                common::agent_availability::AgentCandidatePreference::ToolchainMode,
            include_configured_version: false,
        },
    )
    .await
    .ok()
    .and_then(|availability| availability.selected);

    match selected {
        Some(candidate) => StartkitItemReport {
            status: StartkitItemStatus::Ok,
            version: candidate.version,
            path: Some(candidate.path),
            message: Some(format!(
                "{} selected from {}",
                item.label, candidate.source_label
            )),
            actions: Vec::new(),
            ..base_report(item)
        },
        None => {
            if agent_cli_npm_install_package(agent_id).is_some() {
                let target = if is_managed_mode(choices) {
                    "in VibeAround managed"
                } else {
                    "with npm"
                };
                return StartkitItemReport {
                    status: StartkitItemStatus::Missing,
                    message: Some(format!("{} will be installed {target}", item.label)),
                    actions: vec!["install".to_string()],
                    ..base_report(item)
                };
            }

            apply_agent_manual_guidance(
                StartkitItemReport {
                    status: StartkitItemStatus::Blocked,
                    message: Some(agent_missing_message(item, &choices.toolchain_mode)),
                    actions: Vec::new(),
                    ..base_report(item)
                },
                agent_id,
            )
        }
    }
}

fn agent_missing_message(item: &StartkitItem, toolchain_mode: &str) -> String {
    if toolchain_mode == "managed" {
        return format!(
            "{} does not have a VibeAround managed installer.",
            item.label
        );
    }
    format!(
        "{} was not found in the system toolchain. Install it on this computer, then scan again.",
        item.label
    )
}

fn apply_agent_manual_guidance(
    mut report: StartkitItemReport,
    agent_id: &str,
) -> StartkitItemReport {
    report.actions = vec!["manual".to_string()];
    report.manual_command = agent_detection::source_command_template(agent_id, "native", "install");
    report.manual_url = manual_agent_url(agent_id).map(str::to_string);
    report
}

fn manual_agent_url(agent_id: &str) -> Option<&'static str> {
    match agent_id {
        "cursor" => Some("https://cursor.com/cli"),
        "kiro" => Some("https://kiro.dev/docs/cli/installation/"),
        _ => None,
    }
}

pub(in crate::startkit) fn agent_id_from_cli_item(item_id: &str) -> Option<&str> {
    item_id
        .strip_prefix("agents.")
        .and_then(|value| value.strip_suffix(".cli"))
}
