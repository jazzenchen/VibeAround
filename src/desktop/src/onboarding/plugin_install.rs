//! Plugin installation: git clone plus kind-specific install/build steps.

use anyhow::{bail, Context};
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncBufReadExt, BufReader};

use common::{plugins, resources};

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

/// Invoke npm via `node npm-cli.js`.
///
/// On Windows, `npm` is a `.cmd` batch script that `Command::new("npm")` cannot
/// spawn directly. Locating npm-cli.js next to `node` and calling it via node
/// works cross-platform without any PATH or shell workarounds.
async fn npm_process(
    args: &[String],
    cwd: &std::path::Path,
) -> std::io::Result<tokio::process::Command> {
    let node_info = common::process::env::command("node")
        .args(["-p", "process.execPath"])
        .output()
        .await?;
    let node_exec = String::from_utf8_lossy(&node_info.stdout)
        .trim()
        .to_string();
    let node_dir = std::path::Path::new(&node_exec).parent().ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "cannot determine node install directory",
        )
    })?;

    // Try multiple known locations for npm-cli.js:
    // 1. Next to node binary (nvm, volta, default installs)
    // 2. Homebrew global lib (brew install node on macOS)
    // 3. Homebrew prefix /opt/homebrew or /usr/local
    let candidates = [
        node_dir
            .join("node_modules")
            .join("npm")
            .join("bin")
            .join("npm-cli.js"),
        node_dir
            .join("../lib/node_modules/npm/bin/npm-cli.js")
            .into(),
        std::path::PathBuf::from("/opt/homebrew/lib/node_modules/npm/bin/npm-cli.js"),
        std::path::PathBuf::from("/usr/local/lib/node_modules/npm/bin/npm-cli.js"),
    ];
    let npm_cli = candidates
        .iter()
        .find(|p| p.exists())
        .cloned()
        .ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!(
                    "npm-cli.js not found in any of: {:?} — is npm installed with Node.js?",
                    candidates
                ),
            )
        })?;

    let mut node_args: Vec<String> = vec![npm_cli.to_string_lossy().to_string()];
    node_args.extend(args.iter().cloned());

    tracing::info!(
        "[npm_command] node {} {}",
        npm_cli.display(),
        args.join(" ")
    );
    let mut command = common::process::env::command("node");
    command
        .args(&node_args)
        .current_dir(cwd)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    Ok(command)
}

#[tauri::command]
pub async fn install_plugin(
    request: InstallPluginRequest,
) -> Result<InstallPluginResponse, String> {
    run_install_inner(request).await.map_err(|e| e.to_string())
}

/// Internal implementation — uses anyhow for ergonomic error chaining.
/// Also callable from the onboarding install orchestrator in mod.rs.
pub(super) async fn run_install_inner(
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
        // If a previous install left a partial directory, wipe it for a clean clone.
        let needs_clone = if target_dir.exists() {
            if installed_tree_complete(&target_dir, plugin_kind) {
                tracing::info!(
                    "[install_plugin] {} already installed at {:?}, skipping clone",
                    request.plugin_id,
                    target_dir
                );
                logs.push("Existing plugin directory is complete; skipping git clone".into());
                on_log("Existing plugin directory is complete; skipping git clone".into());
                false
            } else {
                tracing::info!(
                    "[install_plugin] {} has a stale install at {:?}, re-cloning",
                    request.plugin_id,
                    target_dir
                );
                std::fs::remove_dir_all(&target_dir).context("removing stale plugin directory")?;
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
    } else if !target_dir.exists() {
        bail!("plugin directory does not exist: {}", target_dir.display());
    }

    if has_step(&install_steps, "npm_install") {
        tracing::info!("[install_plugin] npm install in {:?}", target_dir);
        logs.push("Running: npm install".into());
        on_log("Running: npm install".into());
        let mut install_args = vec!["install".to_string()];
        install_args.extend(common::process::env::npm_registry_args());
        let output = command_streaming(
            npm_process(&install_args, &target_dir).await?,
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
        tracing::info!("[install_plugin] npm run build in {:?}", target_dir);
        logs.push("Running: npm run build".into());
        on_log("Running: npm run build".into());
        let build_args = vec!["run".to_string(), "build".to_string()];
        let output = command_streaming(
            npm_process(&build_args, &target_dir).await?,
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

    if plugin_kind == "channel" {
        // Channel builds must produce dist/main.js; its absence usually means tsc had silent errors.
        let main_script = target_dir.join("dist").join("main.js");
        if !main_script.exists() {
            bail!("channel plugin install did not produce dist/main.js");
        }
        logs.push("Verified: dist/main.js exists".into());
        on_log("Verified: dist/main.js exists".into());
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
pub fn check_plugin_status(plugin_id: String) -> String {
    let plugin_def = resources::plugin_by_id(&plugin_id);
    let plugin_kind = plugin_def
        .map(|plugin| plugin.kind.as_str())
        .unwrap_or("channel");

    // Onboarding installs must verify the per-user plugin tree. Project plugins
    // are useful in debug builds, but they should not satisfy Startkit's
    // "installed" check for a fresh user's ~/.vibearound/plugins directory.
    let ready = match plugin_kind {
        "channel" => plugins::channel::find_user(&plugin_id).is_some(),
        _ => plugins::find_user(&plugin_id).is_some(),
    };
    if ready {
        return "ready".to_string();
    }

    let target_dir = plugins::user_plugins_dir().join(
        plugin_def
            .map(resources::PluginDef::install_dir_name)
            .unwrap_or(plugin_id.as_str()),
    );
    if !target_dir.join("plugin.json").exists() {
        return "not_installed".to_string();
    }
    if plugin_kind == "channel" && !target_dir.join("dist").join("main.js").exists() {
        return "installed_not_built".to_string();
    }
    "installed_not_discoverable".to_string()
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

fn installed_tree_complete(target_dir: &std::path::Path, plugin_kind: &str) -> bool {
    if !target_dir.join("plugin.json").exists() {
        return false;
    }
    plugin_kind != "channel" || target_dir.join("dist").join("main.js").exists()
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
    let mut child = command.spawn()?;
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
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
                    let _ = child.start_kill();
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
