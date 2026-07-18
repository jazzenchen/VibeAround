//! Plugin installation: git clone plus kind-specific install/build steps.

use anyhow::{bail, Context};
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncBufReadExt, BufReader};

use common::{archive, plugins, resources};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstallPluginRequest {
    pub plugin_id: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstallPluginResponse {
    pub success: bool,
    pub message: String,
    /// The catalog ID, verified against the installed plugin.json.
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
    let plugin_def = catalog_plugin(&request.plugin_id)?;
    let plugins_dir = plugins::user_plugins_dir();
    let target_dir = plugins_dir.join(plugin_def.install_dir_name());
    let staging_dir = archive::staging_dir_for(&target_dir, "plugin")?;
    let mut logs = Vec::new();

    std::fs::create_dir_all(&plugins_dir).context("creating plugins directory")?;
    archive::recreate_dir(&staging_dir)?;

    let result = install_staged_plugin(
        plugin_def,
        &staging_dir,
        &target_dir,
        &mut logs,
        &mut on_log,
        &is_cancelled,
    )
    .await;

    if result.is_err() && staging_dir.exists() {
        if let Err(error) = std::fs::remove_dir_all(&staging_dir) {
            tracing::warn!(path = %staging_dir.display(), %error, "could not remove failed plugin staging directory");
        }
    }
    result
}

async fn install_staged_plugin<F, C>(
    plugin: &resources::PluginDef,
    staging_dir: &std::path::Path,
    target_dir: &std::path::Path,
    logs: &mut Vec<String>,
    on_log: &mut F,
    is_cancelled: &C,
) -> anyhow::Result<InstallPluginResponse>
where
    F: FnMut(String),
    C: Fn() -> bool,
{
    let install_steps = install_steps_for(plugin);
    if !has_step(&install_steps, "git_clone") {
        bail!("plugin catalog entry must include git_clone");
    }

    acquire_catalog_source(plugin, staging_dir, logs, on_log, is_cancelled).await?;
    validate_plugin_manifest(plugin, staging_dir)?;

    if has_step(&install_steps, "npm_install") {
        ensure_managed_node_for_plugin(on_log, is_cancelled).await?;
        let mut install_args = npm_install_args_for(staging_dir);
        install_args.extend(common::process::env::npm_registry_args());
        let command = common::process::env::npm_process(&install_args, staging_dir).await?;
        run_checked_command(
            "npm install",
            format!("Running: npm {}", install_args.join(" ")),
            command,
            logs,
            on_log,
            is_cancelled,
        )
        .await?;
    }

    if has_step(&install_steps, "npm_build") {
        ensure_managed_node_for_plugin(on_log, is_cancelled).await?;
        let build_args = vec!["run".to_string(), "build".to_string()];
        let command = common::process::env::npm_process(&build_args, staging_dir).await?;
        run_checked_command(
            "npm run build",
            "Running: npm run build".to_string(),
            command,
            logs,
            on_log,
            is_cancelled,
        )
        .await?;
    }

    let manifest = validate_plugin_manifest(plugin, staging_dir)?;
    validate_built_entry(&manifest, staging_dir)?;
    if is_cancelled() {
        bail!("install cancelled");
    }

    archive::atomic_replace_dir(staging_dir, target_dir)?;
    let message = format!("Installed catalog revision {}", plugin.revision);
    logs.push(message.clone());
    on_log(message);

    Ok(InstallPluginResponse {
        success: true,
        message: format!("Plugin '{}' installed successfully", plugin.id),
        actual_plugin_id: Some(plugin.id.clone()),
        logs: std::mem::take(logs),
    })
}

#[tauri::command]
pub async fn check_plugin_status(plugin_id: String) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || check_plugin_status_sync(&plugin_id))
        .await
        .map_err(|error| error.to_string())
}

pub(crate) fn check_plugin_status_sync(plugin_id: &str) -> String {
    let Ok(plugin_def) = catalog_plugin(plugin_id) else {
        return "unknown_plugin".to_string();
    };
    let plugin_kind = plugin_def.kind.as_str();

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

    let target_dir = plugins::user_plugins_dir().join(plugin_def.install_dir_name());
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

fn portable_archive_url(plugin: &resources::PluginDef) -> Option<String> {
    let config = common::config::ensure_loaded();
    if !config.portable_toolchain || cfg!(windows) {
        return None;
    }
    // Portable Git is only bundled on Windows; other portable installs avoid
    // requiring a system Git checkout by using GitHub source archives.
    archive::github_revision_archive_url(&plugin.github, &plugin.revision)
}

async fn acquire_catalog_source<F, C>(
    plugin: &resources::PluginDef,
    staging_dir: &std::path::Path,
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

    if let Some(archive_url) = portable_archive_url(plugin) {
        let message = format!("Downloading pinned plugin revision {}", plugin.revision);
        logs.push(message.clone());
        on_log(message);
        archive::download_and_extract_strip_root(
            &archive_url,
            archive::ArchiveFormat::Zip,
            staging_dir,
        )
        .await
        .context("downloading pinned plugin archive")?;
    } else {
        ensure_portable_git_for_plugin(on_log, is_cancelled).await?;

        let mut init = common::process::env::command("git");
        init.args(["init", "--quiet", "."]).current_dir(staging_dir);
        run_checked_command(
            "git init",
            "Preparing plugin checkout".to_string(),
            init,
            logs,
            on_log,
            is_cancelled,
        )
        .await?;

        let mut remote = common::process::env::command("git");
        remote
            .args(["remote", "add", "origin", &plugin.github])
            .current_dir(staging_dir);
        run_checked_command(
            "git remote add",
            "Configuring catalog source".to_string(),
            remote,
            logs,
            on_log,
            is_cancelled,
        )
        .await?;

        let mut fetch = common::process::env::command("git");
        fetch
            .args(["fetch", "--depth", "1", "origin", &plugin.revision])
            .current_dir(staging_dir);
        run_checked_command(
            "git fetch",
            format!("Fetching catalog revision {}", plugin.revision),
            fetch,
            logs,
            on_log,
            is_cancelled,
        )
        .await?;

        let mut checkout = common::process::env::command("git");
        checkout
            .args(["checkout", "--quiet", "--detach", "FETCH_HEAD"])
            .current_dir(staging_dir);
        run_checked_command(
            "git checkout",
            "Checking out pinned plugin revision".to_string(),
            checkout,
            logs,
            on_log,
            is_cancelled,
        )
        .await?;
    }

    if is_cancelled() {
        bail!("install cancelled");
    }
    Ok(())
}

async fn run_checked_command<F, C>(
    step: &str,
    message: String,
    command: tokio::process::Command,
    logs: &mut Vec<String>,
    on_log: &mut F,
    is_cancelled: &C,
) -> anyhow::Result<()>
where
    F: FnMut(String),
    C: Fn() -> bool,
{
    logs.push(message.clone());
    on_log(message);
    let output = command_streaming(command, on_log, is_cancelled)
        .await
        .with_context(|| step.to_string())?;
    push_output_logs(logs, step, &output);
    if !output.status.success() {
        bail!("{step} failed: {}", String::from_utf8_lossy(&output.stderr));
    }
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

fn install_steps_for(plugin_def: &resources::PluginDef) -> Vec<String> {
    if plugin_def.install_steps.is_empty() {
        default_install_steps()
    } else {
        plugin_def.install_steps.clone()
    }
}

fn catalog_plugin(plugin_id: &str) -> anyhow::Result<&'static resources::PluginDef> {
    if !valid_catalog_name(plugin_id) {
        bail!("invalid plugin id '{plugin_id}'");
    }
    let plugin = resources::plugin_by_id(plugin_id)
        .ok_or_else(|| anyhow::anyhow!("unknown managed plugin '{plugin_id}'"))?;
    if !valid_catalog_name(plugin.install_dir_name()) {
        bail!("plugin catalog contains an invalid install directory");
    }
    if !matches!(plugin.kind.as_str(), "channel" | "search") {
        bail!("plugin catalog contains an unsupported kind");
    }
    if archive::github_revision_archive_url(&plugin.github, &plugin.revision).is_none() {
        bail!("plugin catalog contains an invalid source or revision");
    }
    Ok(plugin)
}

fn valid_catalog_name(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
        && value.as_bytes()[0].is_ascii_alphanumeric()
        && value.as_bytes()[value.len() - 1].is_ascii_alphanumeric()
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

fn validate_plugin_manifest(
    plugin: &resources::PluginDef,
    staging_dir: &std::path::Path,
) -> anyhow::Result<plugins::PluginManifest> {
    let manifest_path = staging_dir.join("plugin.json");
    let raw = std::fs::read_to_string(&manifest_path)
        .with_context(|| format!("reading {}", manifest_path.display()))?;
    let manifest: plugins::PluginManifest = serde_json::from_str(&raw)
        .with_context(|| format!("parsing {}", manifest_path.display()))?;

    if manifest.id != plugin.id {
        bail!(
            "plugin manifest id '{}' does not match catalog id '{}'",
            manifest.id,
            plugin.id
        );
    }
    if manifest.kind != plugin.kind {
        bail!(
            "plugin manifest kind '{}' does not match catalog kind '{}'",
            manifest.kind,
            plugin.kind
        );
    }

    Ok(manifest)
}

fn requires_built_entry(plugin_kind: &str) -> bool {
    matches!(plugin_kind, "channel" | "search")
}

fn validate_built_entry(
    manifest: &plugins::PluginManifest,
    staging_dir: &std::path::Path,
) -> anyhow::Result<()> {
    if !requires_built_entry(&manifest.kind) {
        return Ok(());
    }
    let entry = safe_plugin_entry_path(staging_dir, &manifest.entry)
        .ok_or_else(|| anyhow::anyhow!("plugin manifest contains an invalid entry path"))?;
    if !entry.is_file() {
        bail!(
            "{} plugin install did not produce {}",
            manifest.kind,
            entry.strip_prefix(staging_dir).unwrap_or(&entry).display()
        );
    }
    Ok(())
}

fn plugin_entry_path(target_dir: &std::path::Path) -> Option<std::path::PathBuf> {
    let raw = std::fs::read_to_string(target_dir.join("plugin.json")).ok()?;
    let manifest = serde_json::from_str::<serde_json::Value>(&raw).ok()?;
    let entry = manifest
        .get("entry")
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|entry| !entry.is_empty())?;
    safe_plugin_entry_path(target_dir, entry)
}

fn safe_plugin_entry_path(plugin_dir: &std::path::Path, entry: &str) -> Option<std::path::PathBuf> {
    let relative = std::path::Path::new(entry);
    if relative.as_os_str().is_empty()
        || relative.components().any(|component| {
            !matches!(
                component,
                std::path::Component::Normal(_) | std::path::Component::CurDir
            )
        })
    {
        return None;
    }
    Some(plugin_dir.join(relative))
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
    fn accepts_only_single_safe_catalog_names() {
        assert!(valid_catalog_name("va-search-tool"));
        assert!(valid_catalog_name("qqbot"));
        assert!(!valid_catalog_name("../qqbot"));
        assert!(!valid_catalog_name("plugin/name"));
        assert!(!valid_catalog_name("Plugin"));
        assert!(!valid_catalog_name("-plugin"));
        assert!(!valid_catalog_name("plugin-"));
    }

    #[test]
    fn rejects_plugins_outside_the_catalog() {
        let error = catalog_plugin("not-in-catalog").unwrap_err();
        assert!(error.to_string().contains("unknown managed plugin"));
        assert!(catalog_plugin("../../tmp").is_err());
    }

    #[test]
    fn all_catalog_plugins_have_pinned_sources() {
        for plugin in resources::PLUGINS.iter() {
            assert!(catalog_plugin(&plugin.id).is_ok(), "{}", plugin.id);
        }
    }

    #[test]
    fn validates_manifest_identity_and_entry() {
        let root = test_dir("manifest");
        std::fs::create_dir_all(root.join("dist")).unwrap();
        std::fs::write(root.join("dist/main.js"), "export {};").unwrap();
        write_test_manifest(&root, "telegram", "channel", "0.1.0", "dist/main.js");
        let plugin = test_plugin("telegram", "channel");

        let manifest = validate_plugin_manifest(&plugin, &root).unwrap();
        validate_built_entry(&manifest, &root).unwrap();

        write_test_manifest(&root, "other", "channel", "0.1.0", "dist/main.js");
        assert!(validate_plugin_manifest(&plugin, &root)
            .unwrap_err()
            .to_string()
            .contains("does not match catalog id"));

        write_test_manifest(&root, "telegram", "search", "0.1.0", "dist/main.js");
        assert!(validate_plugin_manifest(&plugin, &root)
            .unwrap_err()
            .to_string()
            .contains("does not match catalog kind"));

        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn rejects_manifest_entry_outside_staging() {
        let root = test_dir("entry");
        std::fs::create_dir_all(&root).unwrap();
        write_test_manifest(&root, "telegram", "channel", "0.1.0", "../outside.js");
        let plugin = test_plugin("telegram", "channel");
        let manifest = validate_plugin_manifest(&plugin, &root).unwrap();

        assert!(validate_built_entry(&manifest, &root)
            .unwrap_err()
            .to_string()
            .contains("invalid entry path"));
        std::fs::remove_dir_all(root).unwrap();
    }

    fn test_dir(label: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!("va-plugin-{label}-{}", uuid::Uuid::new_v4()))
    }

    fn test_plugin(id: &str, kind: &str) -> resources::PluginDef {
        resources::PluginDef {
            id: id.to_string(),
            kind: kind.to_string(),
            slug: None,
            name: id.to_string(),
            description: String::new(),
            github: "https://github.com/acme/plugin".to_string(),
            revision: "0123456789abcdef0123456789abcdef01234567".to_string(),
            install_steps: default_install_steps(),
        }
    }

    fn write_test_manifest(
        root: &std::path::Path,
        id: &str,
        kind: &str,
        min_host_version: &str,
        entry: &str,
    ) {
        let manifest = serde_json::json!({
            "id": id,
            "name": id,
            "version": "1.0.0",
            "kind": kind,
            "runtime": "node",
            "entry": entry,
            "minHostVersion": min_host_version
        });
        std::fs::write(
            root.join("plugin.json"),
            serde_json::to_vec(&manifest).unwrap(),
        )
        .unwrap();
    }

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
