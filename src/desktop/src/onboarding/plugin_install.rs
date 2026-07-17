//! Plugin installation: git clone plus kind-specific install/build steps.

use anyhow::{bail, Context};
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncBufReadExt, BufReader};

use common::{archive, plugins, resources};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstallPluginRequest {
    pub plugin_id: String,
    pub github_url: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstallPluginResponse {
    pub success: bool,
    pub message: String,
    /// The plugin ID as declared in the installed plugin.json (may differ from the requested id).
    pub actual_plugin_id: Option<String>,
    pub logs: Vec<String>,
}

#[tauri::command]
pub async fn install_plugin(
    request: InstallPluginRequest,
) -> Result<InstallPluginResponse, String> {
    run_install_inner(request).await.map_err(|e| e.to_string())
}

/// Internal implementation — uses anyhow for ergonomic error chaining.
/// Also callable from the onboarding install orchestrator in mod.rs.
pub(crate) async fn run_install_inner(
    request: InstallPluginRequest,
) -> anyhow::Result<InstallPluginResponse> {
    run_install_inner_with_progress(request, |_| {}, || false).await
}

pub(crate) async fn run_install_inner_with_progress<F, C>(
    request: InstallPluginRequest,
    mut on_log: F,
    is_cancelled: C,
) -> anyhow::Result<InstallPluginResponse>
where
    F: FnMut(String),
    C: Fn() -> bool,
{
    let plugin_def = resources::plugin_by_id(&request.plugin_id);
    let plugin_kind = plugin_def
        .map(|plugin| plugin.kind.as_str())
        .unwrap_or("channel");
    let install_steps = install_steps_for(plugin_def);
    let plugins_dir = plugins::user_plugins_dir();
    let target_dir = plugins_dir.join(
        plugin_def
            .map(resources::PluginDef::install_dir_name)
            .unwrap_or(request.plugin_id.as_str()),
    );
    let mut logs = Vec::new();

    std::fs::create_dir_all(&plugins_dir).context("creating plugins directory")?;

    if has_step(&install_steps, "git_clone") {
        if let Some(archive_url) = portable_archive_url(&request.github_url) {
            install_github_archive_checkout(
                &target_dir,
                &archive_url,
                &mut logs,
                &mut on_log,
                &is_cancelled,
            )
            .await?;
        } else {
            ensure_portable_git_for_plugin(&mut on_log, &is_cancelled).await?;
            // If a previous install left a partial directory, wipe it for a clean clone.
            // Complete git installs are refreshed to the registry HEAD so "Install"
            // also acts as "Update" for already-installed plugins.
            let needs_clone = if target_dir.exists() {
                if installed_tree_complete(&target_dir, plugin_kind) {
                    if target_dir.join(".git").exists() {
                        tracing::info!(
                            "[install_plugin] {} already installed at {:?}, refreshing git HEAD",
                            request.plugin_id,
                            target_dir
                        );
                        let message = "Refreshing existing plugin checkout".to_string();
                        logs.push(message.clone());
                        on_log(message);

                        let mut fetch = common::process::env::command("git");
                        fetch.args(["fetch", "--depth", "1", "origin", "HEAD"]);
                        fetch.current_dir(&target_dir);
                        let output = command_streaming(fetch, &mut on_log, &is_cancelled)
                            .await
                            .context("git fetch")?;
                        push_output_logs(&mut logs, "git fetch", &output);
                        if !output.status.success() {
                            bail!(
                                "git fetch failed: {}",
                                String::from_utf8_lossy(&output.stderr)
                            );
                        }

                        let mut reset = common::process::env::command("git");
                        reset.args(["reset", "--hard", "FETCH_HEAD"]);
                        reset.current_dir(&target_dir);
                        let output = command_streaming(reset, &mut on_log, &is_cancelled)
                            .await
                            .context("git reset")?;
                        push_output_logs(&mut logs, "git reset", &output);
                        if !output.status.success() {
                            bail!(
                                "git reset failed: {}",
                                String::from_utf8_lossy(&output.stderr)
                            );
                        }
                        false
                    } else {
                        tracing::info!(
                            "[install_plugin] {} exists without git metadata at {:?}, re-cloning",
                            request.plugin_id,
                            target_dir
                        );
                        std::fs::remove_dir_all(&target_dir)
                            .context("removing non-git plugin directory")?;
                        true
                    }
                } else {
                    tracing::info!(
                        "[install_plugin] {} has a stale install at {:?}, re-cloning",
                        request.plugin_id,
                        target_dir
                    );
                    std::fs::remove_dir_all(&target_dir)
                        .context("removing stale plugin directory")?;
                    true
                }
            } else {
                true
            };

            if needs_clone {
                tracing::info!(
                    "[install_plugin] cloning {} → {:?}",
                    request.github_url,
                    target_dir
                );
                let message = format!("Running: git clone --depth 1 {}", request.github_url);
                logs.push(message.clone());
                on_log(message);
                let mut command = common::process::env::command("git");
                command.args([
                    "clone",
                    "--depth",
                    "1",
                    &request.github_url,
                    &target_dir.to_string_lossy(),
                ]);
                let output = command_streaming(command, &mut on_log, &is_cancelled)
                    .await
                    .context("git clone")?;
                push_output_logs(&mut logs, "git clone", &output);
                if !output.status.success() {
                    bail!(
                        "git clone failed: {}",
                        String::from_utf8_lossy(&output.stderr)
                    );
                }
            }
        }
    } else if !target_dir.exists() {
        bail!("plugin directory does not exist: {}", target_dir.display());
    }

    if has_step(&install_steps, "npm_install") {
        ensure_managed_node_for_plugin(&mut on_log, &is_cancelled).await?;
        tracing::info!("[install_plugin] npm install in {:?}", target_dir);
        let mut install_args = npm_install_args_for(&target_dir);
        install_args.extend(common::process::env::npm_registry_args());
        let install_message = format!("Running: npm {}", install_args.join(" "));
        logs.push(install_message.clone());
        on_log(install_message);
        let output = command_streaming(
            common::process::env::npm_process(&install_args, &target_dir).await?,
            &mut on_log,
            &is_cancelled,
        )
        .await
        .context("npm install")?;
        push_output_logs(&mut logs, "npm install", &output);
        if !output.status.success() {
            bail!(
                "npm install failed: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
    }

    if has_step(&install_steps, "npm_build") {
        ensure_managed_node_for_plugin(&mut on_log, &is_cancelled).await?;
        tracing::info!("[install_plugin] npm run build in {:?}", target_dir);
        logs.push("Running: npm run build".into());
        on_log("Running: npm run build".into());
        let build_args = vec!["run".to_string(), "build".to_string()];
        let output = command_streaming(
            common::process::env::npm_process(&build_args, &target_dir).await?,
            &mut on_log,
            &is_cancelled,
        )
        .await
        .context("npm run build")?;
        push_output_logs(&mut logs, "npm run build", &output);
        if !output.status.success() {
            bail!(
                "npm run build failed: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
    }

    if requires_built_entry(plugin_kind) {
        let entry =
            plugin_entry_path(&target_dir).unwrap_or_else(|| target_dir.join("dist/main.js"));
        if !entry.exists() {
            bail!(
                "{} plugin install did not produce {}",
                plugin_kind,
                entry.strip_prefix(&target_dir).unwrap_or(&entry).display()
            );
        }
        let message = format!(
            "Verified: {} exists",
            entry.strip_prefix(&target_dir).unwrap_or(&entry).display()
        );
        logs.push(message.clone());
        on_log(message);
    }

    let actual_id = match discover_installed_plugin(&request.plugin_id, plugin_kind) {
        Some(p) => {
            tracing::info!(
                "[install_plugin] {} discoverable as {} plugin (manifest id='{}')",
                request.plugin_id,
                plugin_kind,
                p.manifest.id
            );
            p.manifest.id.clone()
        }
        None => {
            let fallback_id = plugin_manifest_id(&target_dir);
            tracing::info!(
                "[install_plugin] ERROR: {} installed but not discoverable (manifest id={:?})",
                request.plugin_id,
                fallback_id
            );
            bail!(
                "plugin installed but is not discoverable as '{}{}'",
                request.plugin_id,
                fallback_id
                    .as_deref()
                    .map(|id| format!(" (plugin.json id is '{id}')"))
                    .unwrap_or_default()
            );
        }
    };

    Ok(InstallPluginResponse {
        success: true,
        message: format!("Plugin '{}' installed successfully", request.plugin_id),
        actual_plugin_id: Some(actual_id),
        logs,
    })
}

#[tauri::command]
pub async fn check_plugin_status(plugin_id: String) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || check_plugin_status_sync(&plugin_id))
        .await
        .map_err(|error| error.to_string())
}

pub(crate) fn check_plugin_status_sync(plugin_id: &str) -> String {
    let plugin_def = resources::plugin_by_id(&plugin_id);
    let plugin_kind = plugin_def
        .map(|plugin| plugin.kind.as_str())
        .unwrap_or("channel");

    // Onboarding installs must verify the per-user plugin tree. Project plugins
    // are useful in debug builds, but they should not satisfy Startkit's
    // "installed" check for a fresh user's ~/.vibearound/plugins directory.
    let ready = match plugin_kind {
        "channel" => plugins::channel::find_user(plugin_id).is_some(),
        _ => plugins::find_user(plugin_id).is_some(),
    };
    if ready {
        return "ready".to_string();
    }

    let target_dir = plugins::user_plugins_dir().join(
        plugin_def
            .map(resources::PluginDef::install_dir_name)
            .unwrap_or(plugin_id),
    );
    if !target_dir.join("plugin.json").exists() {
        return "not_installed".to_string();
    }
    if requires_built_entry(plugin_kind)
        && !plugin_entry_path(&target_dir)
            .unwrap_or_else(|| target_dir.join("dist/main.js"))
            .exists()
    {
        return "installed_not_built".to_string();
    }
    "installed_not_discoverable".to_string()
}

fn portable_archive_url(github_url: &str) -> Option<String> {
    let config = common::config::ensure_loaded();
    if !config.portable_toolchain || cfg!(windows) {
        return None;
    }
    // Portable Git is only bundled on Windows; other portable installs avoid
    // requiring a system Git checkout by using GitHub source archives.
    archive::github_head_archive_url(github_url)
}

async fn install_github_archive_checkout<F, C>(
    target_dir: &std::path::Path,
    archive_url: &str,
    logs: &mut Vec<String>,
    on_log: &mut F,
    is_cancelled: &C,
) -> anyhow::Result<()>
where
    F: FnMut(String),
    C: Fn() -> bool,
{
    if is_cancelled() {
        bail!("install cancelled");
    }

    tracing::info!(
        "[install_plugin] installing plugin archive {} → {:?}",
        archive_url,
        target_dir
    );
    let message = format!("Downloading plugin archive: {archive_url}");
    logs.push(message.clone());
    on_log(message);

    let staging_dir = archive::staging_dir_for(target_dir, "plugin")?;
    archive::recreate_dir(&staging_dir)?;
    archive::download_and_extract_strip_root(
        archive_url,
        archive::ArchiveFormat::Zip,
        &staging_dir,
    )
    .await
    .context("downloading plugin archive")?;

    if is_cancelled() {
        let _ = std::fs::remove_dir_all(&staging_dir);
        bail!("install cancelled");
    }

    archive::atomic_replace_dir(&staging_dir, target_dir)?;
    let message = "Plugin archive extracted".to_string();
    logs.push(message.clone());
    on_log(message);
    Ok(())
}

async fn ensure_managed_node_for_plugin<F, C>(
    on_log: &mut F,
    is_cancelled: &C,
) -> anyhow::Result<()>
where
    F: FnMut(String),
    C: Fn() -> bool,
{
    if !common::config::ensure_loaded().portable_toolchain {
        return Ok(());
    }
    if common::toolchain::managed_node_status(None).await.ready {
        return Ok(());
    }
    on_log("Installing VibeAround portable Node.js".to_string());
    common::toolchain::ensure_node_lts(
        &common::toolchain::NodeSource::default(),
        on_log,
        is_cancelled,
    )
    .await
    .map(|_| ())
}

async fn ensure_portable_git_for_plugin<F, C>(
    on_log: &mut F,
    is_cancelled: &C,
) -> anyhow::Result<()>
where
    F: FnMut(String),
    C: Fn() -> bool,
{
    if !common::config::ensure_loaded().portable_toolchain || !cfg!(windows) {
        return Ok(());
    }
    if common::toolchain::managed_git_status().await.ready {
        return Ok(());
    }
    on_log("Installing VibeAround portable Git".to_string());
    common::toolchain::ensure_windows_portable_git(on_log, is_cancelled)
        .await
        .map(|_| ())
}

fn default_install_steps() -> Vec<String> {
    vec![
        "git_clone".to_string(),
        "npm_install".to_string(),
        "npm_build".to_string(),
    ]
}

fn install_steps_for(plugin_def: Option<&resources::PluginDef>) -> Vec<String> {
    match plugin_def {
        Some(plugin) if !plugin.install_steps.is_empty() => plugin.install_steps.clone(),
        _ => default_install_steps(),
    }
}

fn has_step(steps: &[String], step: &str) -> bool {
    steps.iter().any(|value| value == step)
}

fn npm_install_args_for(target_dir: &std::path::Path) -> Vec<String> {
    let mut args = vec!["install".to_string()];
    args.extend(platform_npm_install_args(target_dir));
    args
}

#[cfg(windows)]
fn platform_npm_install_args(target_dir: &std::path::Path) -> Vec<String> {
    if package_depends_on(target_dir, "@tencent-connect/openclaw-qqbot") {
        tracing::info!(
            "[install_plugin] detected @tencent-connect/openclaw-qqbot dependency; skipping npm scripts on Windows"
        );
        // The upstream package postinstall only creates an OpenClaw SDK link
        // for native OpenClaw extension installs, and its shell redirection is
        // not valid under Windows cmd.exe. VibeAround imports its API helpers.
        return vec![
            "--legacy-peer-deps".to_string(),
            "--ignore-scripts".to_string(),
        ];
    }
    Vec::new()
}

#[cfg(not(windows))]
fn platform_npm_install_args(_target_dir: &std::path::Path) -> Vec<String> {
    Vec::new()
}

#[cfg(windows)]
fn package_depends_on(target_dir: &std::path::Path, dependency: &str) -> bool {
    let package_json = match std::fs::read_to_string(target_dir.join("package.json")) {
        Ok(raw) => raw,
        Err(_) => return false,
    };
    let package_json = match serde_json::from_str::<serde_json::Value>(&package_json) {
        Ok(value) => value,
        Err(_) => return false,
    };
    package_json_has_dependency(&package_json, dependency)
}

#[cfg(any(test, windows))]
fn package_json_has_dependency(package_json: &serde_json::Value, dependency: &str) -> bool {
    [
        "dependencies",
        "devDependencies",
        "optionalDependencies",
        "peerDependencies",
    ]
    .iter()
    .any(|key| {
        package_json
            .get(*key)
            .and_then(|deps| deps.as_object())
            .is_some_and(|deps| deps.contains_key(dependency))
    })
}

fn installed_tree_complete(target_dir: &std::path::Path, plugin_kind: &str) -> bool {
    if !target_dir.join("plugin.json").exists() {
        return false;
    }
    !requires_built_entry(plugin_kind)
        || plugin_entry_path(target_dir)
            .unwrap_or_else(|| target_dir.join("dist/main.js"))
            .exists()
}

fn discover_installed_plugin(
    plugin_id: &str,
    plugin_kind: &str,
) -> Option<plugins::DiscoveredPlugin> {
    match plugin_kind {
        "channel" => plugins::channel::find(plugin_id),
        _ => plugins::find(plugin_id),
    }
}

fn requires_built_entry(plugin_kind: &str) -> bool {
    matches!(plugin_kind, "channel" | "search")
}

fn plugin_entry_path(target_dir: &std::path::Path) -> Option<std::path::PathBuf> {
    let raw = std::fs::read_to_string(target_dir.join("plugin.json")).ok()?;
    let manifest = serde_json::from_str::<serde_json::Value>(&raw).ok()?;
    manifest
        .get("entry")
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|entry| !entry.is_empty())
        .map(|entry| target_dir.join(entry))
}

fn plugin_manifest_id(target_dir: &std::path::Path) -> Option<String> {
    std::fs::read_to_string(target_dir.join("plugin.json"))
        .ok()
        .and_then(|raw| serde_json::from_str::<serde_json::Value>(&raw).ok())
        .and_then(|v| v.get("id").and_then(|id| id.as_str()).map(String::from))
}

fn push_output_logs(logs: &mut Vec<String>, step: &str, output: &std::process::Output) {
    let stdout = String::from_utf8_lossy(&output.stdout);
    if let Some(excerpt) = output_excerpt(&format!("{step} stdout"), &stdout) {
        logs.push(excerpt);
    }

    let stderr = String::from_utf8_lossy(&output.stderr);
    if let Some(excerpt) = output_excerpt(&format!("{step} stderr"), &stderr) {
        logs.push(excerpt);
    }
}

async fn command_streaming<F, C>(
    mut command: tokio::process::Command,
    on_log: &mut F,
    is_cancelled: &C,
) -> std::io::Result<std::process::Output>
where
    F: FnMut(String),
    C: Fn() -> bool,
{
    let mut child = common::process::spawn_tree_killable(&mut command)?;
    let stdout = child.take_stdout();
    let stderr = child.take_stderr();
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<(&'static str, String)>();

    if let Some(stdout) = stdout {
        let tx = tx.clone();
        tokio::spawn(async move {
            let mut lines = BufReader::new(stdout).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                let _ = tx.send(("stdout", line));
            }
        });
    }
    if let Some(stderr) = stderr {
        let tx = tx.clone();
        tokio::spawn(async move {
            let mut lines = BufReader::new(stderr).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                let _ = tx.send(("stderr", line));
            }
        });
    }
    drop(tx);

    let mut stdout_buf = String::new();
    let mut stderr_buf = String::new();
    let mut cancel_tick = tokio::time::interval(std::time::Duration::from_millis(200));
    let status = loop {
        tokio::select! {
            _ = cancel_tick.tick() => {
                if is_cancelled() {
                    let _ = child.terminate_tree().await;
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::Interrupted,
                        "install cancelled",
                    ));
                }
                if let Some(status) = child.try_wait()? {
                    break status;
                }
            }
            maybe = rx.recv() => {
                if let Some((stream, line)) = maybe {
                    if stream == "stdout" {
                        stdout_buf.push_str(&line);
                        stdout_buf.push('\n');
                    } else {
                        stderr_buf.push_str(&line);
                        stderr_buf.push('\n');
                    }
                    on_log(format!("{stream}: {line}"));
                }
            }
        }
    };

    while let Ok((stream, line)) = rx.try_recv() {
        if stream == "stdout" {
            stdout_buf.push_str(&line);
            stdout_buf.push('\n');
        } else {
            stderr_buf.push_str(&line);
            stderr_buf.push('\n');
        }
        on_log(format!("{stream}: {line}"));
    }

    Ok(std::process::Output {
        status,
        stdout: stdout_buf.into_bytes(),
        stderr: stderr_buf.into_bytes(),
    })
}

fn output_excerpt(label: &str, output: &str) -> Option<String> {
    let trimmed = output.trim();
    if trimmed.is_empty() {
        return None;
    }

    const MAX_CHARS: usize = 4000;
    let mut excerpt = trimmed.to_string();
    if excerpt.len() > MAX_CHARS {
        let start = excerpt.len().saturating_sub(MAX_CHARS);
        excerpt = format!("...{}", &excerpt[start..]);
    }
    Some(format!("{label}:\n{excerpt}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_tencent_openclaw_qqbot_dependency() {
        let package_json = serde_json::json!({
            "dependencies": {
                "@tencent-connect/openclaw-qqbot": "^1.7.1"
            }
        });

        assert!(package_json_has_dependency(
            &package_json,
            "@tencent-connect/openclaw-qqbot"
        ));
    }

    #[test]
    fn ignores_unrelated_dependencies() {
        let package_json = serde_json::json!({
            "dependencies": {
                "ws": "^8.20.1"
            }
        });

        assert!(!package_json_has_dependency(
            &package_json,
            "@tencent-connect/openclaw-qqbot"
        ));
    }
}
