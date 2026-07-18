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
) -> anyhow::Result<StartkitItemReport> {
    if choices.channels.is_empty() {
        return Ok(StartkitItemReport {
            status: StartkitItemStatus::Skipped,
            message: Some("No channel plugins selected".to_string()),
            ..base_report(item)
        });
    }

    for channel_id in &choices.channels {
        if cancelled.load(Ordering::Relaxed) {
            return Ok(StartkitItemReport {
                status: StartkitItemStatus::Skipped,
                message: Some("Cancelled".to_string()),
                ..base_report(item)
            });
        }
        install_channel_plugin(app, run_id, channel_id, cancelled).await?;
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

    let plugin = common::resources::plugin_by_id(channel_id)
        .ok_or_else(|| anyhow!("channel plugin '{channel_id}' not found in registry"))?;

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
            github_url: plugin.github.clone(),
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
