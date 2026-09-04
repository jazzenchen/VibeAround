use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{anyhow, bail, Context};
use serde::Deserialize;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, BufReader};
use tokio::process::Command;
use tokio::sync::mpsc;
use tokio::time::sleep;

use super::redact::redact;
use super::{
    is_managed_mode, item_uses_managed_dependency_dir, Manifest, PlatformScript, StartkitChoices,
    StartkitItem, StartkitItemStatus, StartkitPaths, StartkitProgress,
};

#[derive(Debug, Clone, Deserialize)]
pub(super) struct ScriptOutput {
    pub(super) status: String,
    #[serde(default)]
    pub(super) version: Option<String>,
    #[serde(default)]
    pub(super) latest_version: Option<String>,
    #[serde(default)]
    pub(super) path: Option<String>,
    #[serde(default)]
    pub(super) message: Option<String>,
    #[serde(default)]
    pub(super) actions: Vec<String>,
    #[serde(default)]
    pub(super) manual_command: Option<String>,
    #[serde(default)]
    pub(super) manual_url: Option<String>,
}

/// One line of a script's NDJSON stdout.
///
/// Scripts stream `{"event":"progress","message":"…"}` lines while they work and
/// finish with a result line. A result is either the explicit
/// `{"event":"result", …}` form or the original bare `{"status":"…"}` object, so
/// scripts written before progress streaming keep working unchanged.
enum ScriptLine {
    Progress(Option<String>),
    Result,
}

fn classify_line(line: &str) -> Option<ScriptLine> {
    let value: serde_json::Value = serde_json::from_str(line).ok()?;
    let object = value.as_object()?;
    match object.get("event").and_then(|value| value.as_str()) {
        Some("progress") => Some(ScriptLine::Progress(
            object
                .get("message")
                .and_then(|value| value.as_str())
                .map(str::to_string),
        )),
        Some("result") => Some(ScriptLine::Result),
        // An unrecognized event kind is ignored rather than mistaken for a result.
        Some(_) => None,
        None => object.contains_key("status").then_some(ScriptLine::Result),
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn run_script(
    manifest: &Manifest,
    paths: &StartkitPaths,
    item: &StartkitItem,
    choices: &StartkitChoices,
    platform: &str,
    script_path: &str,
    script: &PlatformScript,
    cancelled: Option<&Arc<AtomicBool>>,
    progress: StartkitProgress<'_>,
) -> anyhow::Result<ScriptOutput> {
    let full_path = paths.root.join(script_path);
    if !full_path.exists() {
        bail!("script not found: {}", full_path.display());
    }

    let mut command = if platform == "windows" {
        let mut cmd = common::process::env::silent_command("powershell.exe");
        cmd.args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-File"]);
        cmd.arg(&full_path);
        cmd
    } else {
        let mut cmd = common::process::env::silent_command("sh");
        cmd.arg(&full_path);
        cmd
    };

    command.args(&script.args);
    command.env_clear();
    command.envs(common::process::env::enriched_env().clone());
    apply_startkit_env(&mut command, manifest, paths, item, choices)?;
    command.stdout(std::process::Stdio::piped());
    command.stderr(std::process::Stdio::piped());

    // The last result line wins, matching the pre-streaming behaviour.
    let mut result_line: Option<String> = None;
    let stderr = run_command_with_cancel(
        command,
        Duration::from_secs(manifest.runner.default_timeout_secs),
        cancelled,
        |line| {
            let trimmed = line.trim_start();
            if !trimmed.starts_with('{') {
                return;
            }
            match classify_line(trimmed) {
                Some(ScriptLine::Progress(message)) => {
                    if let Some(progress) = progress {
                        progress(
                            item,
                            StartkitItemStatus::Running,
                            message.map(|text| redact(&text, &manifest.runner.log_redact_keys)),
                        );
                    }
                }
                Some(ScriptLine::Result) => result_line = Some(trimmed.to_string()),
                None => {}
            }
        },
    )
    .await
    .context("running startkit script")?;

    let stderr = String::from_utf8_lossy(&stderr);
    let line = result_line.ok_or_else(|| {
        anyhow!(
            "script did not emit JSON{}",
            if stderr.trim().is_empty() {
                String::new()
            } else {
                format!(": {}", redact(&stderr, &manifest.runner.log_redact_keys))
            }
        )
    })?;

    let parsed: ScriptOutput =
        serde_json::from_str(&line).with_context(|| format!("parsing script JSON: {line}"))?;
    Ok(parsed)
}

/// Runs the script, delivering each stdout line to `on_line` as it arrives, and
/// returns the collected stderr. Cancellation and the timeout keep polling the
/// child exactly as before; only stdout delivery became incremental.
async fn run_command_with_cancel(
    mut command: Command,
    max_duration: Duration,
    cancelled: Option<&Arc<AtomicBool>>,
    mut on_line: impl FnMut(&str),
) -> anyhow::Result<Vec<u8>> {
    let mut child =
        common::process::spawn_tree_killable(&mut command).context("spawning startkit script")?;
    let stdout = child
        .take_stdout()
        .ok_or_else(|| anyhow!("startkit script stdout was not captured"))?;
    let mut stderr = child
        .take_stderr()
        .ok_or_else(|| anyhow!("startkit script stderr was not captured"))?;

    let (tx, mut rx) = mpsc::unbounded_channel::<String>();
    let stdout_task = tokio::spawn(async move {
        let mut lines = BufReader::new(stdout).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            if tx.send(line).is_err() {
                break;
            }
        }
    });
    let stderr_task = tokio::spawn(async move {
        let mut buf = Vec::new();
        stderr.read_to_end(&mut buf).await.map(|_| buf)
    });

    let started = Instant::now();
    loop {
        while let Ok(line) = rx.try_recv() {
            on_line(&line);
        }
        if cancelled
            .map(|flag| flag.load(Ordering::Relaxed))
            .unwrap_or(false)
        {
            let _ = child.terminate_tree().await;
            bail!("cancelled");
        }
        if started.elapsed() >= max_duration {
            let _ = child.terminate_tree().await;
            bail!("startkit script timed out");
        }
        if child
            .try_wait()
            .context("polling startkit script")?
            .is_some()
        {
            break;
        }
        sleep(Duration::from_millis(200)).await;
    }

    // The reader may still hold buffered lines after the child exits; wait for it
    // to finish so no progress or result line is dropped.
    let _ = stdout_task.await;
    while let Some(line) = rx.recv().await {
        on_line(&line);
    }

    stderr_task
        .await
        .context("joining startkit stderr reader")?
        .context("reading startkit stderr")
}
fn apply_startkit_env(
    command: &mut Command,
    manifest: &Manifest,
    paths: &StartkitPaths,
    item: &StartkitItem,
    choices: &StartkitChoices,
) -> anyhow::Result<()> {
    std::fs::create_dir_all(&paths.cache_dir).ok();

    let source = manifest
        .sources
        .get(&choices.source)
        .or_else(|| manifest.sources.get("global"))
        .ok_or_else(|| anyhow!("startkit source '{}' not found", choices.source))?;

    command.env("STARTKIT_HOME", &paths.home);
    command.env("STARTKIT_ROOT", &paths.root);
    command.env("STARTKIT_CACHE_DIR", &paths.cache_dir);
    command.env("STARTKIT_SOURCE", &choices.source);
    let managed_item_active = item_uses_managed_dependency_dir(item) && is_managed_mode(choices);
    command.env(
        "STARTKIT_ITEM_MANAGED",
        if managed_item_active { "true" } else { "false" },
    );
    command.env("STARTKIT_NPM_REGISTRY", &source.npm_registry);
    command.env("STARTKIT_NODE_INDEX_URL", &source.node_index);
    command.env("STARTKIT_NODE_DIST_BASE", &source.node_dist);
    command.env(
        "STARTKIT_CAN_INSTALL",
        if item.install.is_some() && (!item.managed || managed_item_active) {
            "true"
        } else {
            "false"
        },
    );
    command.env("STARTKIT_ITEM_ID", &item.id);
    if let Some(value) = &item.min_version {
        command.env("STARTKIT_MIN_VERSION", value);
    }
    if let Some(value) = &item.program {
        command.env("STARTKIT_PROGRAM", value);
    }
    if let Some(value) = &item.version_arg {
        command.env("STARTKIT_VERSION_ARG", value);
    }
    if let Some(value) = &item.npm_package {
        command.env("STARTKIT_NPM_PACKAGE", value);
    }
    if let Some(value) = &item.plugin_dependency {
        let plugin_dir = common::plugins::user_plugin_dependency_dir(value);
        let plugin_bin_dir = plugin_dir.join("bin");
        std::fs::create_dir_all(&plugin_bin_dir).ok();
        command.env("STARTKIT_PLUGIN_DIR", plugin_dir);
        command.env("STARTKIT_PLUGIN_BIN_DIR", plugin_bin_dir);
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{classify_line, ScriptLine};

    #[test]
    fn legacy_bare_status_object_is_a_result() {
        assert!(matches!(
            classify_line(r#"{"status":"ok","version":"22.11.0"}"#),
            Some(ScriptLine::Result)
        ));
    }

    #[test]
    fn explicit_result_event_is_a_result() {
        assert!(matches!(
            classify_line(r#"{"event":"result","status":"ok"}"#),
            Some(ScriptLine::Result)
        ));
    }

    #[test]
    fn progress_event_carries_its_message() {
        let Some(ScriptLine::Progress(message)) =
            classify_line(r#"{"event":"progress","message":"Extracting"}"#)
        else {
            panic!("expected a progress line");
        };
        assert_eq!(message.as_deref(), Some("Extracting"));
    }

    #[test]
    fn progress_event_without_a_message_is_still_progress() {
        assert!(matches!(
            classify_line(r#"{"event":"progress"}"#),
            Some(ScriptLine::Progress(None))
        ));
    }

    #[test]
    fn unknown_event_kinds_are_ignored_rather_than_read_as_results() {
        assert!(classify_line(r#"{"event":"telemetry","status":"ok"}"#).is_none());
    }

    #[test]
    fn non_object_and_non_json_lines_are_ignored() {
        assert!(classify_line("Downloading...").is_none());
        assert!(classify_line(r#"["status"]"#).is_none());
        assert!(classify_line(r#"{"note":"no status here"}"#).is_none());
    }
}
