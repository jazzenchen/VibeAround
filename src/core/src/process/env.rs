//! Environment utilities for distributed desktop app.
//!
//! When launched from Finder, the process inherits a minimal environment
//! that lacks user-configured variables (API keys, PATH with NVM, etc.).
//! This module probes the user's login shell once at startup to capture
//! their full environment, caches it, and exposes helpers that create
//! child process Commands with the enriched environment.
//!
//! On Windows, the cached process environment is supplemented with well-known
//! PATH directories. Agent detection separately reloads the Machine and User
//! registry environments on every scan so newly installed tools are visible
//! without restarting VibeAround.

use std::collections::HashMap;
use std::ffi::OsStr;
use std::path::Path;
use std::sync::OnceLock;

#[cfg(unix)]
use std::io::IsTerminal;

#[cfg(windows)]
use std::os::windows::process::CommandExt;

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x08000000;

const NPM_REGISTRY_GLOBAL: &str = "https://registry.npmjs.org";
const NPM_REGISTRY_CN: &str = "https://registry.npmmirror.com";

/// Cached full environment from the user's login shell.
static ENRICHED_ENV: OnceLock<HashMap<String, String>> = OnceLock::new();

/// Return the enriched environment.  Probed once on first call, cached forever.
pub fn enriched_env() -> &'static HashMap<String, String> {
    ENRICHED_ENV.get_or_init(|| {
        let result = probe_enriched_env();
        tracing::info!(
            "[env] enriched environment ({} vars, PATH has {} entries)",
            result.len(),
            path_value(&result)
                .map(|p| p.matches(path_separator()).count() + 1)
                .unwrap_or(0)
        );
        result
    })
}

/// Return the environment VibeAround should pass to child processes.
///
/// This is the cached shell environment captured from the user's login shell.
/// When enabled, the portable Node/Git paths are prepended so app-owned runtime
/// tools win over user PATH entries without modifying the user's shell profile.
/// VibeAround-managed package binaries are controlled separately by
/// `toolchain_mode`.
pub fn child_env() -> HashMap<String, String> {
    let mut env = enriched_env().clone();
    let config = crate::config::ensure_loaded();
    if config.portable_toolchain {
        crate::toolchain::prepend_managed_tool_paths(&mut env);
    }
    if config.toolchain_mode.is_managed() {
        prepend_managed_npm_bin_path(&mut env);
    }
    env
}

/// Create a `tokio::process::Command` with the enriched environment pre-set.
/// Drop-in replacement for `tokio::process::Command::new(program)`.
pub fn command(program: impl AsRef<OsStr>) -> tokio::process::Command {
    let mut cmd = silent_command(program);
    cmd.env_clear();
    cmd.envs(child_env());
    cmd
}

/// Create a `std::process::Command` with the enriched environment pre-set.
pub fn std_command(program: impl AsRef<OsStr>) -> std::process::Command {
    let mut cmd = silent_std_command(program);
    cmd.env_clear();
    cmd.envs(child_env());
    cmd
}

/// Create a command without altering its environment, but suppress transient
/// console windows for short-lived helper processes on Windows.
pub fn silent_command(program: impl AsRef<OsStr>) -> tokio::process::Command {
    let mut cmd = tokio::process::Command::new(program);
    hide_windows_console(&mut cmd);
    cmd
}

/// Synchronous variant of [`silent_command`].
pub fn silent_std_command(program: impl AsRef<OsStr>) -> std::process::Command {
    let mut cmd = std::process::Command::new(program);
    hide_windows_console_std(&mut cmd);
    cmd
}

/// Return npm registry flags for installs started from VibeAround.
///
/// Startkit lets users choose a download source during onboarding. Native
/// Startkit scripts get the source through `STARTKIT_NPM_REGISTRY`; Rust-side
/// npm installs for ACP adapters and channel plugins need the same policy.
pub fn npm_registry_args() -> Vec<String> {
    npm_registry_url()
        .map(|registry| vec!["--registry".to_string(), registry])
        .unwrap_or_default()
}

pub fn npm_registry_url() -> Option<String> {
    for key in ["STARTKIT_NPM_REGISTRY", "VIBEAROUND_NPM_REGISTRY"] {
        if let Ok(value) = std::env::var(key) {
            let value = value.trim();
            if !value.is_empty() {
                return Some(value.to_string());
            }
        }
    }

    let path = crate::config::data_dir().join("settings.json");
    let contents = std::fs::read_to_string(path).ok()?;
    let json = serde_json::from_str::<serde_json::Value>(&contents).ok()?;
    let source = json
        .get("startkit")
        .and_then(|value| value.get("source"))
        .and_then(serde_json::Value::as_str)?;
    npm_registry_for_source(source).map(str::to_string)
}

/// Invoke npm through `node npm-cli.js`.
///
/// This avoids platform-specific wrappers (`npm.cmd` on Windows) and works for
/// both a user-installed Node.js and VibeAround's managed portable Node.js.
pub async fn npm_process(
    args: &[String],
    cwd: &std::path::Path,
) -> std::io::Result<tokio::process::Command> {
    let node_info = command("node")
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

    let candidates = [
        node_dir
            .join("node_modules")
            .join("npm")
            .join("bin")
            .join("npm-cli.js"),
        node_dir.join("../lib/node_modules/npm/bin/npm-cli.js"),
        std::path::PathBuf::from("/opt/homebrew/lib/node_modules/npm/bin/npm-cli.js"),
        std::path::PathBuf::from("/usr/local/lib/node_modules/npm/bin/npm-cli.js"),
    ];
    let npm_cli = candidates
        .iter()
        .find(|path| path.exists())
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

    tracing::info!("[npm] node {} {}", npm_cli.display(), args.join(" "));
    let mut command = command("node");
    command
        .args(&node_args)
        .current_dir(cwd)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    Ok(command)
}

fn npm_registry_for_source(source: &str) -> Option<&'static str> {
    match source {
        "cn" => Some(NPM_REGISTRY_CN),
        "global" => Some(NPM_REGISTRY_GLOBAL),
        _ => None,
    }
}

#[cfg(windows)]
fn hide_windows_console(cmd: &mut tokio::process::Command) {
    cmd.creation_flags(CREATE_NO_WINDOW);
}

#[cfg(not(windows))]
fn hide_windows_console(_: &mut tokio::process::Command) {}

#[cfg(windows)]
fn hide_windows_console_std(cmd: &mut std::process::Command) {
    cmd.creation_flags(CREATE_NO_WINDOW);
}

#[cfg(not(windows))]
fn hide_windows_console_std(_: &mut std::process::Command) {}

/// Directory where npm-based ACP agent packages are installed.
/// Shared with channel plugins at `~/.vibearound/plugins/` so common
/// dependencies (e.g. `@agentclientprotocol/sdk`, `zod`) are deduped.
pub fn acp_agents_dir() -> std::path::PathBuf {
    crate::config::data_dir().join("plugins")
}

pub fn managed_npm_bin_dir() -> std::path::PathBuf {
    acp_agents_dir().join("node_modules").join(".bin")
}

fn prepend_managed_npm_bin_path(env: &mut HashMap<String, String>) {
    let candidate = managed_npm_bin_dir();
    if !candidate.exists() {
        return;
    }

    let sep = path_separator();
    let current = path_value(env).unwrap_or_default();
    let mut parts: Vec<String> = current
        .split(sep)
        .filter(|part| !part.trim().is_empty())
        .map(String::from)
        .collect();

    let value = candidate.to_string_lossy().to_string();
    let exists = if cfg!(windows) {
        parts.iter().any(|part| part.eq_ignore_ascii_case(&value))
    } else {
        parts.iter().any(|part| part == &value)
    };
    if !exists {
        parts.insert(0, value);
    }

    set_path_value(env, parts.join(&sep.to_string()));
}

pub fn ensure_user_path_dir(path: &Path) -> anyhow::Result<()> {
    if !path.exists() {
        std::fs::create_dir_all(path)?;
    }
    ensure_user_path_dir_inner(path)
}

#[cfg(unix)]
fn ensure_user_path_dir_inner(path: &Path) -> anyhow::Result<()> {
    use anyhow::Context;
    use std::io::Write;

    let profile = user_shell_profile_path();
    let expression = shell_path_expression(path);
    let absolute = path.to_string_lossy();

    let existing = std::fs::read_to_string(&profile).unwrap_or_default();
    if existing.contains(expression.as_str()) || existing.contains(absolute.as_ref()) {
        return Ok(());
    }

    if let Some(parent) = profile.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("creating shell profile dir {:?}", parent))?;
    }

    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&profile)
        .with_context(|| format!("opening shell profile {:?}", profile))?;
    writeln!(
        file,
        "\n# VibeAround Startkit PATH\ncase \":$PATH:\" in\n  *:\"{expression}\":*) ;;\n  *) export PATH=\"$PATH:{expression}\" ;;\nesac"
    )
    .with_context(|| format!("updating shell profile {:?}", profile))?;
    Ok(())
}

#[cfg(unix)]
fn user_shell_profile_path() -> std::path::PathBuf {
    let home = crate::config::home_dir();
    let shell = std::env::var("SHELL").unwrap_or_default();
    if shell.contains("zsh") {
        return home.join(".zshrc");
    }
    if shell.contains("bash") {
        return if cfg!(target_os = "macos") {
            home.join(".bash_profile")
        } else {
            home.join(".bashrc")
        };
    }
    home.join(".profile")
}

#[cfg(unix)]
fn shell_path_expression(path: &Path) -> String {
    let home = crate::config::home_dir();
    let value = path
        .strip_prefix(&home)
        .ok()
        .map(|relative| {
            let relative = relative.to_string_lossy();
            if relative.is_empty() {
                "$HOME".to_string()
            } else {
                format!("$HOME/{}", escape_double_quoted_shell_path(&relative))
            }
        })
        .unwrap_or_else(|| escape_double_quoted_shell_path(&path.to_string_lossy()));
    value
}

#[cfg(unix)]
fn escape_double_quoted_shell_path(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('`', "\\`")
        .replace('$', "\\$")
}

#[cfg(windows)]
fn ensure_user_path_dir_inner(path: &Path) -> anyhow::Result<()> {
    use anyhow::Context;

    let path = windows_user_path(path.to_path_buf())
        .to_string_lossy()
        .to_string();
    let script = r#"
$dir = $args[0]
$current = [Environment]::GetEnvironmentVariable('Path', 'User')
if ([string]::IsNullOrWhiteSpace($current)) {
  [Environment]::SetEnvironmentVariable('Path', $dir, 'User')
  exit 0
}
$parts = $current -split ';' | Where-Object { -not [string]::IsNullOrWhiteSpace($_) }
foreach ($part in $parts) {
  if ($part.TrimEnd('\') -ieq $dir.TrimEnd('\')) { exit 0 }
}
[Environment]::SetEnvironmentVariable('Path', ($current.TrimEnd(';') + ';' + $dir), 'User')
"#;
    let output = std_command("powershell.exe")
        .args([
            "-NoProfile",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            script,
        ])
        .arg(path)
        .output()
        .context("updating user PATH")?;
    if !output.status.success() {
        anyhow::bail!(
            "updating user PATH failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(())
}

/// Resolve the JS entry point for a pre-installed npm ACP agent binary.
///
/// Looks up `~/.vibearound/plugins/node_modules/.bin/<bin_name>` and resolves
/// Unix symlinks or Windows npm wrappers to a JavaScript entry point.
pub fn resolve_acp_agent_bin(bin_name: &str) -> anyhow::Result<std::path::PathBuf> {
    resolve_npm_bin_in_dir(&acp_agents_dir(), bin_name)
}

pub fn resolve_npm_bin_in_dir(
    package_dir: &std::path::Path,
    bin_name: &str,
) -> anyhow::Result<std::path::PathBuf> {
    let bin_dir = package_dir.join("node_modules").join(".bin");

    #[cfg(unix)]
    {
        let bin_path = bin_dir.join(bin_name);
        if !bin_path.exists() {
            anyhow::bail!(
                "npm binary '{}' not found at {:?}. Run onboarding to install it.",
                bin_name,
                bin_path
            );
        }
        // Follow symlink to actual JS file
        let resolved = std::fs::canonicalize(&bin_path)
            .map_err(|e| anyhow::anyhow!("cannot resolve symlink {:?}: {}", bin_path, e))?;
        Ok(resolved)
    }

    #[cfg(windows)]
    {
        let cmd_path = bin_dir.join(format!("{}.cmd", bin_name));
        if !cmd_path.exists() {
            anyhow::bail!(
                "npm binary '{}' not found at {:?}. Run onboarding to install it.",
                bin_name,
                cmd_path
            );
        }
        let content = std::fs::read_to_string(&cmd_path)?;
        for js_path in windows_npm_cmd_js_entries(&content, &bin_dir) {
            if js_path.exists() {
                return Ok(windows_user_path(std::fs::canonicalize(&js_path)?));
            }
        }
        anyhow::bail!("could not parse JS entry from {:?}", cmd_path);
    }
}

#[cfg(windows)]
fn windows_npm_cmd_js_entries(content: &str, bin_dir: &std::path::Path) -> Vec<std::path::PathBuf> {
    let mut entries = Vec::new();
    for token in windows_cmd_quoted_tokens(content) {
        let Some(js_pos) = token.to_ascii_lowercase().find(".js") else {
            continue;
        };
        let token = &token[..js_pos + 3];
        entries.push(expand_windows_npm_cmd_js_token(bin_dir, token));
    }
    entries
}

#[cfg(windows)]
fn windows_cmd_quoted_tokens(content: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    for line in content.lines() {
        let mut rest = line;
        while let Some(start) = rest.find('"') {
            rest = &rest[start + 1..];
            let Some(end) = rest.find('"') else {
                break;
            };
            tokens.push(rest[..end].to_string());
            rest = &rest[end + 1..];
        }
    }
    tokens
}

#[cfg(windows)]
fn expand_windows_npm_cmd_js_token(bin_dir: &std::path::Path, token: &str) -> std::path::PathBuf {
    let normalized = token.replace('\\', "/");
    for prefix in ["%dp0%/", "%~dp0/"] {
        if let Some(rest) = normalized.strip_prefix(prefix) {
            let mut path = bin_dir.to_path_buf();
            for segment in rest.split('/') {
                if segment.is_empty() {
                    continue;
                }
                path.push(segment);
            }
            return path;
        }
    }

    std::path::PathBuf::from(token)
}

#[cfg(windows)]
fn windows_user_path(path: std::path::PathBuf) -> std::path::PathBuf {
    let value = path.to_string_lossy();
    if let Some(rest) = value.strip_prefix(r"\\?\UNC\") {
        return std::path::PathBuf::from(format!(r"\\{rest}"));
    }
    if let Some(rest) = value.strip_prefix(r"\\?\") {
        return std::path::PathBuf::from(rest);
    }
    path
}

// ---------------------------------------------------------------------------
// Platform-specific environment probing
// ---------------------------------------------------------------------------

fn probe_enriched_env() -> HashMap<String, String> {
    // Start with the current process environment as baseline
    let mut env: HashMap<String, String> = std::env::vars().collect();

    #[cfg(unix)]
    {
        if stdio_is_terminal() {
            // Terminal-launched server/desktop dev sessions already inherit
            // the user's shell environment. Avoid probing an interactive
            // login shell here: zsh can briefly take terminal foreground
            // control for job management, leaving Ctrl+C pointed at a dead
            // process group after the probe exits.
            enrich_unix_path_fallback(&mut env);
        } else if let Some(shell_env) = probe_unix_login_shell_env() {
            // Shell env takes precedence (has user's full setup)
            env.extend(shell_env);
        } else {
            // Fallback: at least enrich PATH with well-known directories
            enrich_unix_path_fallback(&mut env);
        }
    }

    #[cfg(windows)]
    {
        // Windows GUI apps already inherit the full registry env.
        // Just append well-known node directories to PATH as safety net.
        enrich_windows_path(&mut env);
    }

    env
}

#[cfg(windows)]
pub fn path_value(env: &HashMap<String, String>) -> Option<String> {
    env.iter()
        .find(|(key, _)| key.eq_ignore_ascii_case("PATH"))
        .map(|(_, value)| value.clone())
}

/// Return the PATH that agent detection should scan.
///
/// Windows reloads the Machine and User registry environments on every call so
/// a rescan sees PATH changes made after VibeAround started. Process-only PATH
/// entries are retained after the fresh registry entries. Other platforms keep
/// using the cached enriched shell environment.
pub fn detection_path_value() -> Option<String> {
    #[cfg(windows)]
    {
        windows_registry_path_value().or_else(|| path_value(enriched_env()))
    }

    #[cfg(not(windows))]
    {
        path_value(enriched_env())
    }
}

#[cfg(not(windows))]
pub fn path_value(env: &HashMap<String, String>) -> Option<String> {
    env.get("PATH").cloned()
}

#[cfg(windows)]
pub fn path_env_key() -> &'static str {
    "Path"
}

#[cfg(not(windows))]
pub fn path_env_key() -> &'static str {
    "PATH"
}

#[cfg(windows)]
fn path_separator() -> char {
    ';'
}

#[cfg(not(windows))]
fn path_separator() -> char {
    ':'
}

#[cfg(windows)]
pub fn set_path_value(env: &mut HashMap<String, String>, value: String) {
    env.retain(|key, _| !key.eq_ignore_ascii_case("PATH"));
    env.insert(path_env_key().to_string(), value);
}

#[cfg(not(windows))]
pub fn set_path_value(env: &mut HashMap<String, String>, value: String) {
    env.insert(path_env_key().to_string(), value);
}

#[cfg(unix)]
fn stdio_is_terminal() -> bool {
    std::io::stdin().is_terminal()
        || std::io::stdout().is_terminal()
        || std::io::stderr().is_terminal()
}

/// Probe the user's login shell for their full environment.
#[cfg(unix)]
fn probe_unix_login_shell_env() -> Option<HashMap<String, String>> {
    let shells_to_try: Vec<String> = {
        let mut shells = Vec::new();
        if let Ok(user_shell) = std::env::var("SHELL") {
            shells.push(user_shell);
        }
        shells.push("/bin/zsh".to_string());
        shells.push("/bin/bash".to_string());
        shells
    };

    for shell in &shells_to_try {
        if !std::path::Path::new(shell).exists() {
            continue;
        }
        // Use `env -0` for null-separated output (handles values with newlines)
        let result = std::process::Command::new(shell)
            .args(["-ilc", "env -0"])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .output();

        match result {
            Ok(output) if output.status.success() => {
                let raw = String::from_utf8_lossy(&output.stdout);
                let parsed: HashMap<String, String> = raw
                    .split('\0')
                    .filter(|s| !s.is_empty())
                    .filter_map(|entry| {
                        let (key, value) = entry.split_once('=')?;
                        // Skip shell-internal vars that would pollute child envs
                        if key.starts_with("_=")
                            || key == "SHLVL"
                            || key == "PWD"
                            || key == "OLDPWD"
                            || key == "ZSH_EVAL_CONTEXT"
                        {
                            return None;
                        }
                        Some((key.to_string(), value.to_string()))
                    })
                    .collect();

                if parsed.contains_key("PATH") && parsed.len() > 5 {
                    tracing::info!("[env] probed {} vars from {}", parsed.len(), shell);
                    return Some(parsed);
                }
            }
            Ok(_) => {
                tracing::info!("[env] shell {} exited with non-zero status", shell);
            }
            Err(e) => {
                tracing::info!("[env] failed to probe {}: {}", shell, e);
            }
        }
    }
    None
}

/// Fallback: append well-known Unix paths to PATH if shell probe fails.
#[cfg(unix)]
fn enrich_unix_path_fallback(env: &mut HashMap<String, String>) {
    let current = env.get("PATH").cloned().unwrap_or_default();
    let extras = ["/opt/homebrew/bin", "/usr/local/bin"];
    let mut parts: Vec<&str> = current.split(':').collect();

    for extra in &extras {
        if !parts.contains(extra) && std::path::Path::new(extra).is_dir() {
            parts.push(extra);
        }
    }

    // Probe NVM default
    if let Ok(home) = std::env::var("HOME") {
        let nvm_default = format!("{}/.nvm/alias/default", home);
        if let Ok(version_alias) = std::fs::read_to_string(&nvm_default) {
            let version = version_alias.trim();
            let nvm_versions = format!("{}/.nvm/versions/node", home);
            if let Ok(entries) = std::fs::read_dir(&nvm_versions) {
                for entry in entries.flatten() {
                    let name = entry.file_name();
                    let name_str = name.to_string_lossy();
                    if name_str.starts_with(version) || name_str.contains(version) {
                        let bin = entry.path().join("bin");
                        if bin.is_dir() {
                            let bin_str = bin.to_string_lossy().to_string();
                            if !current.contains(&bin_str) {
                                parts.insert(0, Box::leak(bin_str.into_boxed_str()));
                            }
                        }
                    }
                }
            }
        }
    }

    env.insert("PATH".to_string(), parts.join(":"));
}

/// Windows: append well-known Node.js install directories to PATH.
#[cfg(windows)]
fn enrich_windows_path(env: &mut HashMap<String, String>) {
    let current = path_value(env).unwrap_or_default();
    let sep = ";";
    let mut parts: Vec<String> = current.split(sep).map(String::from).collect();
    let candidates: Vec<String> = vec![
        std::env::var("APPDATA")
            .map(|d| format!("{}\\npm", d))
            .unwrap_or_default(),
        std::env::var("ProgramFiles")
            .map(|d| format!("{}\\nodejs", d))
            .unwrap_or_default(),
        std::env::var("LOCALAPPDATA")
            .map(|d| format!("{}\\Volta\\bin", d))
            .unwrap_or_default(),
    ];
    for candidate in candidates {
        if !candidate.is_empty()
            && !parts.iter().any(|p| p.eq_ignore_ascii_case(&candidate))
            && std::path::Path::new(&candidate).is_dir()
        {
            parts.push(candidate);
        }
    }
    set_path_value(env, parts.join(sep));
}

#[cfg(windows)]
fn windows_registry_path_value() -> Option<String> {
    use winreg::enums::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE};
    use winreg::RegKey;

    const MACHINE_ENV_KEY: &str = r"SYSTEM\CurrentControlSet\Control\Session Manager\Environment";
    const USER_ENV_KEY: &str = r"Environment";

    let machine =
        read_windows_registry_environment(&RegKey::predef(HKEY_LOCAL_MACHINE), MACHINE_ENV_KEY);
    let user = read_windows_registry_environment(&RegKey::predef(HKEY_CURRENT_USER), USER_ENV_KEY);

    if machine.is_empty() && user.is_empty() {
        return None;
    }

    let process = normalize_windows_environment(std::env::vars());
    let machine = normalize_windows_environment(machine);
    let user = normalize_windows_environment(user);

    let mut expansion_env = process.clone();
    expansion_env.remove("PATH");
    for (key, value) in machine.iter().chain(user.iter()) {
        if key != "PATH" {
            expansion_env.insert(key.clone(), value.clone());
        }
    }

    let machine_path = machine
        .get("PATH")
        .map(|value| expand_windows_env_vars(value, &expansion_env));
    if let Some(path) = machine_path.as_ref() {
        expansion_env.insert("PATH".to_string(), path.clone());
    }
    let user_path = user
        .get("PATH")
        .map(|value| expand_windows_env_vars(value, &expansion_env));
    let process_path = process.get("PATH").cloned();

    merge_windows_path_values([
        machine_path.as_deref(),
        user_path.as_deref(),
        process_path.as_deref(),
    ])
}

#[cfg(windows)]
fn read_windows_registry_environment(
    root: &winreg::RegKey,
    subkey: &str,
) -> HashMap<String, String> {
    let Ok(key) = root.open_subkey(subkey) else {
        return HashMap::new();
    };

    key.enum_values()
        .filter_map(Result::ok)
        .filter_map(|(name, _)| {
            key.get_value::<String, _>(&name)
                .ok()
                .map(|value| (name, value))
        })
        .collect()
}

#[cfg(windows)]
fn normalize_windows_environment(
    values: impl IntoIterator<Item = (String, String)>,
) -> HashMap<String, String> {
    values
        .into_iter()
        .map(|(key, value)| (key.to_ascii_uppercase(), value))
        .collect()
}

#[cfg(windows)]
fn expand_windows_env_vars(value: &str, env: &HashMap<String, String>) -> String {
    let mut expanded = value.to_string();
    for _ in 0..8 {
        let next = expand_windows_env_vars_once(&expanded, env);
        if next == expanded {
            break;
        }
        expanded = next;
    }
    expanded
}

#[cfg(windows)]
fn expand_windows_env_vars_once(value: &str, env: &HashMap<String, String>) -> String {
    let mut output = String::with_capacity(value.len());
    let mut remaining = value;

    while let Some(start) = remaining.find('%') {
        output.push_str(&remaining[..start]);
        let after_start = &remaining[start + 1..];
        let Some(end) = after_start.find('%') else {
            output.push_str(&remaining[start..]);
            return output;
        };

        let name = &after_start[..end];
        if let Some(replacement) = env.get(&name.to_ascii_uppercase()) {
            output.push_str(replacement);
        } else {
            output.push('%');
            output.push_str(name);
            output.push('%');
        }
        remaining = &after_start[end + 1..];
    }

    output.push_str(remaining);
    output
}

#[cfg(windows)]
fn merge_windows_path_values<'a>(
    values: impl IntoIterator<Item = Option<&'a str>>,
) -> Option<String> {
    let mut paths = Vec::new();
    let mut seen = std::collections::HashSet::new();

    for value in values.into_iter().flatten() {
        for path in value
            .split(';')
            .map(str::trim)
            .filter(|path| !path.is_empty())
        {
            let key = path.trim_end_matches(['\\', '/']).to_ascii_lowercase();
            if seen.insert(key) {
                paths.push(path.to_string());
            }
        }
    }

    (!paths.is_empty()).then(|| paths.join(";"))
}

#[cfg(test)]
mod registry_tests {
    use super::*;

    #[test]
    fn maps_startkit_sources_to_npm_registries() {
        assert_eq!(npm_registry_for_source("cn"), Some(NPM_REGISTRY_CN));
        assert_eq!(npm_registry_for_source("global"), Some(NPM_REGISTRY_GLOBAL));
        assert_eq!(npm_registry_for_source("custom"), None);
    }
}

#[cfg(windows)]
#[cfg(test)]
mod windows_path_tests {
    use super::*;

    #[test]
    fn path_value_reads_windows_path_case_insensitively() {
        let mut env = HashMap::new();
        env.insert("Path".to_string(), r"C:\Program Files\Git\cmd".to_string());

        assert_eq!(
            path_value(&env).as_deref(),
            Some(r"C:\Program Files\Git\cmd")
        );
    }

    #[test]
    fn set_path_value_replaces_any_existing_path_casing() {
        let mut env = HashMap::new();
        env.insert("PATH".to_string(), "old".to_string());
        env.insert("Path".to_string(), "older".to_string());

        set_path_value(&mut env, "new".to_string());

        assert_eq!(env.len(), 1);
        assert_eq!(env.get("Path").map(String::as_str), Some("new"));
    }

    #[test]
    fn expands_fresh_registry_variables_case_insensitively() {
        let env = normalize_windows_environment([
            ("SystemDrive".to_string(), "D:".to_string()),
            (
                "AgentHome".to_string(),
                r"%SYSTEMDRIVE%\Tools\Agent".to_string(),
            ),
        ]);

        assert_eq!(
            expand_windows_env_vars(r"%agenthome%\bin", &env),
            r"D:\Tools\Agent\bin"
        );
    }

    #[test]
    fn merges_registry_paths_before_unique_process_paths() {
        let merged = merge_windows_path_values([
            Some(r"D:\System\bin;C:\Windows"),
            Some(r"E:\User\bin;d:\system\bin\"),
            Some(r"C:\Windows;F:\Process\bin"),
        ]);

        assert_eq!(
            merged.as_deref(),
            Some(r"D:\System\bin;C:\Windows;E:\User\bin;F:\Process\bin")
        );
    }
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;

    #[test]
    fn parses_npm_cmd_wrappers_that_use_prog_variable() {
        let content = r#"@ECHO off
GOTO start
:find_dp0
SET dp0=%~dp0
EXIT /b
:start
SETLOCAL
CALL :find_dp0

IF EXIST "%dp0%\node.exe" (
  SET "_prog=%dp0%\node.exe"
) ELSE (
  SET "_prog=node"
  SET PATHEXT=%PATHEXT:;.JS;=;%
)

endLocal & goto #_undefined_# 2>NUL || title %COMSPEC% & "%_prog%"  "%dp0%\..\@zed-industries\codex-acp\bin\codex-acp.js" %*
"#;
        let bin_dir = std::path::Path::new(r"C:\Users\jazze\.vibearound\plugins\node_modules\.bin");

        let entries = windows_npm_cmd_js_entries(content, bin_dir);

        assert_eq!(
            entries,
            vec![std::path::PathBuf::from(
                r"C:\Users\jazze\.vibearound\plugins\node_modules\.bin\..\@zed-industries\codex-acp\bin\codex-acp.js"
            )]
        );
    }

    #[test]
    fn parses_legacy_npm_cmd_wrappers_that_call_node_directly() {
        let content = r#"@node "%~dp0\..\pkg\dist\index.js" %*"#;
        let bin_dir = std::path::Path::new(r"C:\va\plugins\node_modules\.bin");

        let entries = windows_npm_cmd_js_entries(content, bin_dir);

        assert_eq!(
            entries,
            vec![std::path::PathBuf::from(
                r"C:\va\plugins\node_modules\.bin\..\pkg\dist\index.js"
            )]
        );
    }

    #[test]
    fn strips_extended_path_prefix_before_passing_entry_to_node() {
        assert_eq!(
            windows_user_path(std::path::PathBuf::from(
                r"\\?\C:\Users\jazze\.vibearound\plugins\entry.js"
            )),
            std::path::PathBuf::from(r"C:\Users\jazze\.vibearound\plugins\entry.js")
        );
        assert_eq!(
            windows_user_path(std::path::PathBuf::from(
                r"\\?\UNC\server\share\plugins\entry.js"
            )),
            std::path::PathBuf::from(r"\\server\share\plugins\entry.js")
        );
    }
}
