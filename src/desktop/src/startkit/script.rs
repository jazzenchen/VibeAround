use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{anyhow, bail, Context};
use serde::Deserialize;
use tokio::io::AsyncReadExt;
use tokio::process::Command;
use tokio::time::sleep;

use super::redact::redact;
use super::{
    is_managed_mode, item_uses_managed_dependency_dir, Manifest, PlatformScript, StartkitChoices,
    StartkitItem, StartkitPaths,
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

    let output = run_command_with_cancel(
        command,
        Duration::from_secs(manifest.runner.default_timeout_secs),
        cancelled,
    )
    .await
    .context("running startkit script")?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let line = stdout
        .lines()
        .rev()
        .find(|line| line.trim_start().starts_with('{'))
        .ok_or_else(|| {
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
        serde_json::from_str(line).with_context(|| format!("parsing script JSON: {line}"))?;
    Ok(parsed)
}

async fn run_command_with_cancel(
    mut command: Command,
    max_duration: Duration,
    cancelled: Option<&Arc<AtomicBool>>,
) -> anyhow::Result<std::process::Output> {
    let mut child =
        common::process::spawn_tree_killable(&mut command).context("spawning startkit script")?;
    let mut stdout = child
        .take_stdout()
        .ok_or_else(|| anyhow!("startkit script stdout was not captured"))?;
    let mut stderr = child
        .take_stderr()
        .ok_or_else(|| anyhow!("startkit script stderr was not captured"))?;

    let stdout_task = tokio::spawn(async move {
        let mut buf = Vec::new();
        stdout.read_to_end(&mut buf).await.map(|_| buf)
    });
    let stderr_task = tokio::spawn(async move {
        let mut buf = Vec::new();
        stderr.read_to_end(&mut buf).await.map(|_| buf)
    });

    let started = Instant::now();
    let status = loop {
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
        if let Some(status) = child.try_wait().context("polling startkit script")? {
            break status;
        }
        sleep(Duration::from_millis(200)).await;
    };

    let stdout = stdout_task
        .await
        .context("joining startkit stdout reader")?
        .context("reading startkit stdout")?;
    let stderr = stderr_task
        .await
        .context("joining startkit stderr reader")?
        .context("reading startkit stderr")?;

    Ok(std::process::Output {
        status,
        stdout,
        stderr,
    })
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
