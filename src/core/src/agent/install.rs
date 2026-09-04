//! Auto-install agent binaries: npm ACP agents + native CLIs with install commands.
//!
//! Called from two paths:
//! - Onboarding / pre-install: `install_acp_agents` iterates enabled agents.
//! - Lazy install: `Agent::spawn` falls through here when a binary is missing.

use anyhow::Context;
use tokio::io::{AsyncBufReadExt, BufReader};

use crate::resources;

/// Output captured from an install command.
pub struct InstallOutput {
    pub stdout: String,
    pub stderr: String,
}

#[derive(Debug, PartialEq, Eq)]
struct NpmPackageSpec<'a> {
    package_name: &'a str,
    requested_version: Option<&'a str>,
}

fn npm_package_spec(npm_package: &str) -> NpmPackageSpec<'_> {
    let npm_package = npm_package.trim();
    let version_separator = if npm_package.starts_with('@') {
        npm_package
            .rfind('@')
            .filter(|separator| *separator > 0 && npm_package[..*separator].contains('/'))
    } else {
        npm_package.rfind('@').filter(|separator| *separator > 0)
    };

    if let Some(separator) = version_separator {
        NpmPackageSpec {
            package_name: &npm_package[..separator],
            requested_version: Some(&npm_package[separator + 1..]),
        }
    } else {
        NpmPackageSpec {
            package_name: npm_package,
            requested_version: None,
        }
    }
}

pub fn npm_package_bin_name(npm_package: &str) -> String {
    npm_package_spec(npm_package)
        .package_name
        .rsplit('/')
        .next()
        .unwrap_or(npm_package)
        .to_string()
}

pub fn npm_package_installed(npm_package: &str, bin_name: &str) -> bool {
    npm_package_installed_in_dir(
        npm_package,
        bin_name,
        &crate::process::env::acp_agents_dir(),
    )
}

pub fn npm_package_installed_in_dir(
    npm_package: &str,
    bin_name: &str,
    package_dir: &std::path::Path,
) -> bool {
    crate::process::env::resolve_npm_bin_in_dir(package_dir, bin_name).is_ok()
        && npm_package_version_satisfied_in_dir(npm_package, package_dir)
}

fn npm_package_version_satisfied_in_dir(npm_package: &str, package_dir: &std::path::Path) -> bool {
    let spec = npm_package_spec(npm_package);
    let Some(requested_version) = spec.requested_version else {
        return true;
    };

    let package_json = spec
        .package_name
        .split('/')
        .fold(package_dir.join("node_modules"), |path, segment| {
            path.join(segment)
        })
        .join("package.json");
    let Ok(contents) = std::fs::read_to_string(package_json) else {
        return false;
    };
    let Ok(json) = serde_json::from_str::<serde_json::Value>(&contents) else {
        return false;
    };
    json.get("version").and_then(serde_json::Value::as_str) == Some(requested_version)
}

/// Auto-install an npm ACP agent package into `~/.vibearound/plugins/`.
pub async fn auto_install_npm_agent(npm_package: &str) -> anyhow::Result<()> {
    auto_install_npm_agent_with_output(npm_package)
        .await
        .map(|_| ())
}

/// Like `auto_install_npm_agent` but returns captured stdout/stderr.
pub async fn auto_install_npm_agent_with_output(
    npm_package: &str,
) -> anyhow::Result<InstallOutput> {
    auto_install_npm_agent_with_progress(npm_package, |_| {}).await
}

pub async fn auto_install_npm_agent_with_progress<F>(
    npm_package: &str,
    on_log: F,
) -> anyhow::Result<InstallOutput>
where
    F: FnMut(String),
{
    auto_install_npm_agent_with_progress_and_cancel(npm_package, on_log, || false).await
}

pub async fn auto_install_npm_agent_with_progress_and_cancel<F, C>(
    npm_package: &str,
    mut on_log: F,
    is_cancelled: C,
) -> anyhow::Result<InstallOutput>
where
    F: FnMut(String),
    C: Fn() -> bool,
{
    let plugins_dir = crate::process::env::acp_agents_dir();
    auto_install_npm_package_in_dir_with_progress_and_cancel(
        npm_package,
        &plugins_dir,
        &mut on_log,
        is_cancelled,
    )
    .await
}

pub async fn auto_install_npm_package_in_dir_with_progress_and_cancel<F, C>(
    npm_package: &str,
    package_dir: &std::path::Path,
    mut on_log: F,
    is_cancelled: C,
) -> anyhow::Result<InstallOutput>
where
    F: FnMut(String),
    C: Fn() -> bool,
{
    std::fs::create_dir_all(package_dir).with_context(|| format!("creating {:?}", package_dir))?;

    let pkg_json = package_dir.join("package.json");
    if !pkg_json.exists() {
        let init = serde_json::json!({ "name": "vibearound-managed", "private": true });
        std::fs::write(&pkg_json, serde_json::to_string_pretty(&init).unwrap())
            .context("writing package.json")?;
    }

    let output = npm_command_streaming(
        &npm_install_args(&["install", npm_package]),
        package_dir,
        &mut on_log,
        is_cancelled,
    )
    .await
    .with_context(|| format!("running npm install {}", npm_package))?;

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();

    if !output.status.success() {
        anyhow::bail!("npm install {} failed: {}", npm_package, stderr.trim());
    }
    tracing::info!(
        "[agent] installed {} in {}",
        npm_package,
        package_dir.display()
    );
    Ok(InstallOutput { stdout, stderr })
}

/// Install an npm-backed CLI into the active global npm prefix.
///
/// This is used by Startkit after Node is ready. The command runs through
/// `node npm-cli.js` so it works with either a user Node install or the
/// Startkit-provisioned Node runtime.
pub async fn auto_install_npm_global_package_with_progress_and_cancel<F, C>(
    npm_package: &str,
    mut on_log: F,
    is_cancelled: C,
) -> anyhow::Result<InstallOutput>
where
    F: FnMut(String),
    C: Fn() -> bool,
{
    let cwd = crate::config::data_dir();
    std::fs::create_dir_all(&cwd).with_context(|| format!("creating {:?}", cwd))?;

    let target = npm_global_install_target(npm_package);
    let mut args = vec!["install".to_string(), "-g".to_string(), target.clone()];
    args.extend(crate::process::env::npm_registry_args());

    tracing::info!("[agent] installing global npm CLI: {}", target);
    let output = npm_command_streaming(&args, &cwd, &mut on_log, is_cancelled)
        .await
        .with_context(|| format!("running npm install -g {}", target))?;

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();

    if !output.status.success() {
        anyhow::bail!("npm install -g {} failed: {}", target, stderr.trim());
    }
    let bin_dir = npm_global_bin_dir(&cwd).await?;
    crate::process::env::ensure_user_path_dir(&bin_dir)
        .with_context(|| format!("adding {} to user PATH", bin_dir.display()))?;
    on_log(format!("Added {} to user PATH", bin_dir.display()));
    tracing::info!("[agent] installed global npm CLI {}", target);
    Ok(InstallOutput { stdout, stderr })
}

fn npm_global_install_target(npm_package: &str) -> String {
    let package = npm_package.trim();
    if npm_package_spec(package).requested_version.is_some() {
        package.to_string()
    } else {
        format!("{package}@latest")
    }
}

fn npm_install_args(args: &[&str]) -> Vec<String> {
    let mut out = args
        .iter()
        .map(|arg| (*arg).to_string())
        .collect::<Vec<_>>();
    out.extend(crate::process::env::npm_registry_args());
    out
}

async fn npm_global_bin_dir(cwd: &std::path::Path) -> anyhow::Result<std::path::PathBuf> {
    let args = vec!["prefix".to_string(), "-g".to_string()];
    let output = crate::process::env::npm_process(&args, cwd)
        .await?
        .output()
        .await
        .context("running npm prefix -g")?;
    if !output.status.success() {
        anyhow::bail!(
            "npm prefix -g failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let prefix = stdout
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .ok_or_else(|| anyhow::anyhow!("npm prefix -g returned no output"))?;
    let prefix = std::path::PathBuf::from(prefix);
    Ok(if cfg!(windows) {
        prefix
    } else {
        prefix.join("bin")
    })
}

async fn npm_command_streaming<F>(
    args: &[String],
    cwd: &std::path::Path,
    on_log: &mut F,
    is_cancelled: impl Fn() -> bool,
) -> std::io::Result<std::process::Output>
where
    F: FnMut(String),
{
    let mut command = crate::process::env::npm_process(args, cwd).await?;
    let mut child = crate::process::spawn_tree_killable(&mut command)?;
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

/// Install a native agent CLI by running its official install command.
pub async fn auto_install_agent_cmd(install_cmd: &str, agent: &str) -> anyhow::Result<()> {
    auto_install_agent_cmd_with_output(install_cmd, agent)
        .await
        .map(|_| ())
}

/// Like `auto_install_agent_cmd` but returns captured stdout/stderr.
pub async fn auto_install_agent_cmd_with_output(
    install_cmd: &str,
    agent: &str,
) -> anyhow::Result<InstallOutput> {
    tracing::info!("[agent] running install for {}: {}", agent, install_cmd);

    let (program, args) = install_command_invocation(install_cmd);
    let output = crate::process::env::command(program)
        .args(args)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .output()
        .await
        .with_context(|| format!("running install cmd for {}", agent))?;

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();

    if !output.status.success() {
        anyhow::bail!("install {} failed: {}", agent, stderr.trim());
    }

    tracing::info!("[agent] installed {}", agent);
    Ok(InstallOutput { stdout, stderr })
}

fn install_command_invocation(install_cmd: &str) -> (&'static str, Vec<String>) {
    if cfg!(windows) {
        (
            "powershell.exe",
            vec![
                "-NoProfile".to_string(),
                "-ExecutionPolicy".to_string(),
                "Bypass".to_string(),
                "-Command".to_string(),
                install_cmd.to_string(),
            ],
        )
    } else {
        ("sh", vec!["-lc".to_string(), install_cmd.to_string()])
    }
}

/// Check if a program is available in PATH.
pub fn is_program_available(program: &str) -> bool {
    crate::process::env::std_command("which")
        .arg(program)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Pre-install all ACP agent packages (npm or binary) for enabled agents.
pub async fn install_acp_agents(settings: &serde_json::Value) {
    if !has_enabled_channels(settings) {
        tracing::info!("[agent] no IM channels enabled; skipping ACP agent preinstall");
        return;
    }

    let all_agents = resources::agent_ids();
    let enabled_agents = super::resolve_enabled_agents(settings, &all_agents);

    for agent_id in &enabled_agents {
        let agent_def = match resources::agent_by_id(agent_id) {
            Some(def) => def,
            None => continue,
        };

        // npm-based agents (Claude ACP, Codex ACP)
        if let Some(npm_pkg) = &agent_def.acp.npm_package {
            let default_bin_name = npm_package_bin_name(npm_pkg);
            let bin_name = agent_def
                .acp
                .bin_name
                .as_deref()
                .unwrap_or(&default_bin_name);
            if npm_package_installed(npm_pkg, bin_name) {
                continue;
            }
            tracing::info!("[agent] installing ACP agent: {}", npm_pkg);
            if let Err(e) = auto_install_npm_agent(npm_pkg).await {
                tracing::info!("[agent] npm install {} error: {}", npm_pkg, e);
            }
        }
        // Native binary agents with install command (Cursor, Kiro)
        else if let Some(install_cmd) = native_agent_install_command(agent_id, agent_def) {
            let config = crate::config::ensure_loaded();
            let detected = crate::agent_availability::resolve_agent_availability(
                agent_id,
                crate::agent_availability::AgentAvailabilityRequest {
                    scan_policy: crate::agent_availability::AgentScanPolicy::RefreshIfMissing,
                    toolchain_mode: config.toolchain_mode.as_str(),
                    candidate_preference:
                        crate::agent_availability::AgentCandidatePreference::ToolchainMode,
                    include_configured_version: true,
                },
            )
            .await
            .ok()
            .and_then(|availability| availability.selected);
            if detected.is_some() {
                continue;
            }
            if config.toolchain_mode.is_managed() {
                tracing::info!(
                    "[agent] skipping native install for {} in managed toolchain mode",
                    agent_id
                );
                continue;
            }
            if is_program_available(&agent_def.acp.program) {
                continue;
            }
            if let Err(e) = auto_install_agent_cmd(&install_cmd, agent_id).await {
                tracing::info!("[agent] install {} error: {}", agent_id, e);
            }
        }
    }
}

fn native_agent_install_command(agent_id: &str, agent_def: &resources::AgentDef) -> Option<String> {
    crate::agent_detection::source_command_template(agent_id, "native", "install")
        .or_else(|| agent_def.acp.install_cmd.clone())
}

fn has_enabled_channels(settings: &serde_json::Value) -> bool {
    settings
        .get("channels")
        .and_then(|v| v.as_object())
        .map(|channels| !channels.is_empty())
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::{
        install_command_invocation, native_agent_install_command, npm_global_install_target,
        npm_package_bin_name, npm_package_spec, NpmPackageSpec,
    };
    use crate::resources;

    #[test]
    fn parses_scoped_npm_package_specs() {
        assert_eq!(
            npm_package_spec("@agentclientprotocol/codex-acp@1.1.14"),
            NpmPackageSpec {
                package_name: "@agentclientprotocol/codex-acp",
                requested_version: Some("1.1.14"),
            }
        );
        assert_eq!(
            npm_package_spec("@agentclientprotocol/claude-agent-acp@0.66.0"),
            NpmPackageSpec {
                package_name: "@agentclientprotocol/claude-agent-acp",
                requested_version: Some("0.66.0"),
            }
        );
    }

    #[test]
    fn derives_default_bin_name_from_package_name() {
        assert_eq!(
            npm_package_bin_name("@agentclientprotocol/codex-acp@1.1.14"),
            "codex-acp"
        );
        assert_eq!(npm_package_bin_name("plain-agent@1.2.3"), "plain-agent");
    }

    #[test]
    fn global_install_target_adds_latest_when_unpinned() {
        assert_eq!(
            npm_global_install_target("@anthropic-ai/claude-code"),
            "@anthropic-ai/claude-code@latest"
        );
        assert_eq!(
            npm_global_install_target("@openai/codex@1.2.3"),
            "@openai/codex@1.2.3"
        );
    }

    #[test]
    fn native_install_invocation_uses_platform_shell() {
        let (program, args) = install_command_invocation("echo hello");
        if cfg!(windows) {
            assert_eq!(program, "powershell.exe");
            assert!(args.contains(&"-Command".to_string()));
            assert_eq!(args.last().map(String::as_str), Some("echo hello"));
        } else {
            assert_eq!(program, "sh");
            assert_eq!(args, vec!["-lc".to_string(), "echo hello".to_string()]);
        }
    }

    #[test]
    fn native_agent_install_command_prefers_source_catalog() {
        let cursor = resources::agent_by_id("cursor").expect("cursor agent");
        let command = native_agent_install_command("cursor", cursor).expect("install command");
        if cfg!(windows) {
            assert!(command.contains("install.ps1"));
            assert!(!command.contains("| bash"));
        } else {
            assert!(command.contains("cursor.com/install"));
        }
    }
}
