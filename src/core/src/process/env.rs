//! Environment utilities for distributed desktop app.
//!
//! When launched from Finder, the process inherits a minimal environment
//! that lacks user-configured variables (API keys, PATH with NVM, etc.).
//! This module probes the user's login shell once at startup to capture
//! their full environment, caches it, and exposes helpers that create
//! child process Commands with the enriched environment.
//!
//! On Windows, GUI apps already inherit the full registry-based environment,
//! so only well-known PATH directories are appended as a safety net.

use std::collections::HashMap;
use std::sync::OnceLock;

#[cfg(windows)]
use std::os::windows::process::CommandExt;

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x08000000;

/// Cached full environment from the user's login shell.
static ENRICHED_ENV: OnceLock<HashMap<String, String>> = OnceLock::new();

/// Return the enriched environment.  Probed once on first call, cached forever.
pub fn enriched_env() -> &'static HashMap<String, String> {
    ENRICHED_ENV.get_or_init(|| {
        let result = probe_enriched_env();
        tracing::info!(
            "[env] enriched environment ({} vars, PATH has {} entries)",
            result.len(),
            result
                .get("PATH")
                .map(|p| p.matches(':').count() + 1)
                .unwrap_or(0)
        );
        result
    })
}

/// Return the environment VibeAround should pass to child processes.
///
/// This is the cached shell environment plus the Startkit-managed toolchain
/// paths unless the user explicitly selected `system` mode.
pub fn child_env() -> HashMap<String, String> {
    let mut env = enriched_env().clone();
    if vibearound_managed_paths_enabled() {
        prepend_vibearound_managed_paths(&mut env);
    }
    env
}

/// Create a `tokio::process::Command` with the enriched environment pre-set.
/// Drop-in replacement for `tokio::process::Command::new(program)`.
pub fn command(program: &str) -> tokio::process::Command {
    let mut cmd = tokio::process::Command::new(program);
    hide_windows_console(&mut cmd);
    cmd.env_clear();
    cmd.envs(child_env());
    cmd
}

/// Create a `std::process::Command` with the enriched environment pre-set.
pub fn std_command(program: &str) -> std::process::Command {
    let mut cmd = std::process::Command::new(program);
    hide_windows_console_std(&mut cmd);
    cmd.env_clear();
    cmd.envs(child_env());
    cmd
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

/// Resolve the JS entry point for a pre-installed npm ACP agent binary.
///
/// Looks up `~/.vibearound/plugins/node_modules/.bin/<bin_name>`.
/// On Unix the `.bin/` entries are symlinks to the actual JS file — we
/// follow the symlink.  On Windows npm creates `.cmd` wrappers; we parse
/// them to extract the JS path.
pub fn resolve_acp_agent_bin(bin_name: &str) -> anyhow::Result<std::path::PathBuf> {
    let bin_dir = acp_agents_dir().join("node_modules").join(".bin");

    #[cfg(unix)]
    {
        let bin_path = bin_dir.join(bin_name);
        if !bin_path.exists() {
            anyhow::bail!(
                "ACP agent binary '{}' not found at {:?}. Run onboarding to install it.",
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
        // On Windows, npm creates <name>.cmd; parse it to find the JS entry
        let cmd_path = bin_dir.join(format!("{}.cmd", bin_name));
        if !cmd_path.exists() {
            anyhow::bail!(
                "ACP agent binary '{}' not found at {:?}. Run onboarding to install it.",
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
        if let Some(shell_env) = probe_unix_login_shell_env() {
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

/// Prepend binaries installed by Startkit into the app-managed toolchain.
///
/// Startkit installs Node.js, npm global CLIs, and helper binaries under
/// `~/.vibearound` instead of mutating system directories. Every child
/// process launched by VibeAround should see those tools first.
fn prepend_vibearound_managed_paths(env: &mut HashMap<String, String>) {
    let sep = if cfg!(windows) { ';' } else { ':' };
    let home = crate::config::data_dir();
    let candidates = [
        home.join("bin"),
        home.join("runtime").join("node").join("bin"),
        home.join("runtime").join("node"),
        home.join("npm-global").join("bin"),
        home.join("npm-global"),
    ];

    let current = env.get("PATH").cloned().unwrap_or_default();
    let mut parts: Vec<String> = current
        .split(sep)
        .filter(|part| !part.trim().is_empty())
        .map(String::from)
        .collect();

    for candidate in candidates.iter().rev() {
        let value = candidate.to_string_lossy().to_string();
        let exists = if cfg!(windows) {
            parts.iter().any(|part| part.eq_ignore_ascii_case(&value))
        } else {
            parts.iter().any(|part| part == &value)
        };
        if !exists {
            parts.insert(0, value);
        }
    }

    env.insert("PATH".to_string(), parts.join(&sep.to_string()));
}

fn vibearound_managed_paths_enabled() -> bool {
    let path = crate::config::data_dir().join("settings.json");
    let Ok(contents) = std::fs::read_to_string(path) else {
        return true;
    };
    let Ok(json) = serde_json::from_str::<serde_json::Value>(&contents) else {
        return true;
    };
    json.get("startkit")
        .and_then(|value| value.get("toolchain_mode"))
        .and_then(serde_json::Value::as_str)
        != Some("system")
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
    let current = env.get("PATH").cloned().unwrap_or_default();
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
    env.insert("PATH".to_string(), parts.join(sep));
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
