use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use anyhow::anyhow;
use tauri::{AppHandle, Runtime};

use super::{
    base_report, emit_progress_event, StartkitChoices, StartkitItem, StartkitItemReport,
    StartkitItemStatus,
};

pub(in crate::startkit) async fn run_channel_plugins_item<R: Runtime>(
    app: &AppHandle<R>,
    run_id: Option<&str>,
    item: &StartkitItem,
    choices: &StartkitChoices,
    cancelled: &Arc<AtomicBool>,
    skipped_item_ids: &HashSet<String>,
) -> anyhow::Result<StartkitItemReport> {
    if choices.channels.is_empty() {
        return Ok(StartkitItemReport {
            status: StartkitItemStatus::Skipped,
            message: Some("No channel plugins selected".to_string()),
            ..base_report(item)
        });
    }

    let mut attempted = 0usize;
    let mut failed = 0usize;

    for channel_id in &choices.channels {
        if cancelled.load(Ordering::Relaxed) {
            return Ok(StartkitItemReport {
                status: StartkitItemStatus::Skipped,
                message: Some("Cancelled".to_string()),
                ..base_report(item)
            });
        }

        let progress_id = format!("channels.plugins.{channel_id}");
        if skipped_item_ids.contains(&progress_id) {
            emit_progress_event(
                app,
                run_id,
                progress_id,
                channel_id.to_string(),
                StartkitItemStatus::Skipped,
                Some("Skipped for now".to_string()),
                None,
            );
            continue;
        }

        attempted += 1;
        if install_channel_plugin(app, run_id, channel_id, cancelled)
            .await
            .is_err()
        {
            failed += 1;
        }
    }

    if failed > 0 {
        return Ok(StartkitItemReport {
            status: StartkitItemStatus::Error,
            message: Some(format!(
                "{failed} of {attempted} channel plugins failed to install"
            )),
            actions: vec!["install".to_string()],
            ..base_report(item)
        });
    }

    if attempted == 0 {
        return Ok(StartkitItemReport {
            status: StartkitItemStatus::Skipped,
            message: Some("All selected channel plugins were skipped".to_string()),
            ..base_report(item)
        });
    }

    Ok(StartkitItemReport {
        status: StartkitItemStatus::Ok,
        message: Some("Channel plugins are ready".to_string()),
        actions: Vec::new(),
        ..base_report(item)
    })
}

async fn install_channel_plugin<R: Runtime>(
    app: &AppHandle<R>,
    run_id: Option<&str>,
    channel_id: &str,
    cancelled: &Arc<AtomicBool>,
) -> anyhow::Result<()> {
    let progress_id = format!("channels.plugins.{channel_id}");
    if crate::onboarding::plugin_install::check_plugin_status_sync(channel_id) == "ready" {
        emit_progress_event(
            app,
            run_id,
            progress_id,
            channel_id.to_string(),
            StartkitItemStatus::Ok,
            Some(format!("{channel_id} plugin already installed")),
            None,
        );
        return Ok(());
    }

    let plugin = match common::resources::plugin_by_id(channel_id) {
        Some(plugin) => plugin,
        None => {
            let error = anyhow!("channel plugin '{channel_id}' not found in registry");
            emit_progress_event(
                app,
                run_id,
                progress_id,
                channel_id.to_string(),
                StartkitItemStatus::Error,
                Some(error.to_string()),
                None,
            );
            return Err(error);
        }
    };

    emit_progress_event(
        app,
        run_id,
        progress_id.clone(),
        plugin.name.clone(),
        StartkitItemStatus::Running,
        Some(format!("Installing {} plugin", plugin.name)),
        None,
    );

    let result = crate::onboarding::plugin_install::run_install_inner_with_progress(
        crate::onboarding::plugin_install::InstallPluginRequest {
            plugin_id: channel_id.to_string(),
        },
        |line| {
            emit_progress_event(
                app,
                run_id,
                progress_id.clone(),
                plugin.name.clone(),
                StartkitItemStatus::Running,
                Some(line),
                None,
            );
        },
        || cancelled.load(Ordering::Relaxed),
    )
    .await;

    match result {
        Ok(_) => {
            emit_progress_event(
                app,
                run_id,
                progress_id,
                plugin.name.clone(),
                StartkitItemStatus::Ok,
                Some("Plugin is installed".to_string()),
                None,
            );
            Ok(())
        }
        Err(error) => {
            emit_progress_event(
                app,
                run_id,
                progress_id,
                plugin.name.clone(),
                StartkitItemStatus::Error,
                Some(error.to_string()),
                None,
            );
            Err(error)
        }
    }
}
