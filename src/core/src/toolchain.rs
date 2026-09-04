//! VibeAround-managed portable runtime toolchain.
//!
//! The portable toolchain option keeps Node.js and selected helper tools under
//! `~/.vibearound/runtime` and exposes them to child processes through
//! `process::env::child_env()`. Scans are local-only; installers perform the
//! network work and update a small manifest once the extracted tool is usable.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use anyhow::Context;
use serde::{Deserialize, Serialize};


const NODE_MANIFEST_NAME: &str = "current.json";
const GIT_MANIFEST_NAME: &str = "current.json";

#[derive(Debug, Clone)]
pub struct ManagedToolStatus {
    pub installed: bool,
    pub ready: bool,
    pub version: Option<String>,
    pub path: Option<PathBuf>,
    pub message: Option<String>,
}

impl ManagedToolStatus {
    fn missing(message: impl Into<String>) -> Self {
        Self {
            installed: false,
            ready: false,
            version: None,
            path: None,
            message: Some(message.into()),
        }
    }

    fn broken(path: PathBuf, message: impl Into<String>) -> Self {
        Self {
            installed: true,
            ready: false,
            version: None,
            path: Some(path),
            message: Some(message.into()),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct RuntimeManifest {
    version: String,
    install_dir: PathBuf,
    installed_at_unix_ms: u128,
}






pub fn runtime_dir() -> PathBuf {
    crate::config::data_dir().join("runtime")
}

pub fn managed_node_bin_dir() -> Option<PathBuf> {
    let manifest = read_runtime_manifest(&node_manifest_path()).ok()?;
    let bin_dir = node_bin_dir_in(&manifest.install_dir);
    node_executable_in(&manifest.install_dir)
        .exists()
        .then_some(bin_dir)
}

pub fn managed_git_bin_dir() -> Option<PathBuf> {
    let manifest = read_runtime_manifest(&git_manifest_path()).ok()?;
    let bin_dir = git_bin_dir_in(&manifest.install_dir);
    git_executable_in(&manifest.install_dir)
        .exists()
        .then_some(bin_dir)
}

pub fn managed_node_executable() -> Option<PathBuf> {
    let manifest = read_runtime_manifest(&node_manifest_path()).ok()?;
    let executable = node_executable_in(&manifest.install_dir);
    executable.exists().then_some(executable)
}

pub fn managed_git_executable() -> Option<PathBuf> {
    let manifest = read_runtime_manifest(&git_manifest_path()).ok()?;
    let executable = git_executable_in(&manifest.install_dir);
    executable.exists().then_some(executable)
}

pub fn prepend_managed_tool_paths(env: &mut HashMap<String, String>) {
    if let Some(path) = managed_git_bin_dir() {
        prepend_path(env, path);
    }
    if let Some(path) = managed_node_bin_dir() {
        prepend_path(env, path);
    }
}

pub async fn managed_node_status(min_version: Option<&str>) -> ManagedToolStatus {
    let Some(executable) = managed_node_executable() else {
        return ManagedToolStatus::missing("Managed Node.js is not installed");
    };
    let Some(version) = command_version(&executable, &["--version"]).await else {
        return ManagedToolStatus::broken(executable, "Managed Node.js did not report a version");
    };
    if let Some(min_version) = min_version {
        if !version_at_least(&version, min_version) {
            return ManagedToolStatus {
                installed: true,
                ready: false,
                version: Some(version.clone()),
                path: Some(executable),
                message: Some(format!(
                    "Managed Node.js {version} is older than {min_version}"
                )),
            };
        }
    }
    ManagedToolStatus {
        installed: true,
        ready: true,
        version: Some(version.clone()),
        path: Some(executable),
        message: Some(format!("Managed Node.js {version} is ready")),
    }
}

pub async fn managed_git_status() -> ManagedToolStatus {
    if !cfg!(windows) {
        return ManagedToolStatus::missing(
            "Managed Portable Git is only enabled on Windows for now",
        );
    }
    let Some(executable) = managed_git_executable() else {
        return ManagedToolStatus::missing("Managed Portable Git is not installed");
    };
    let Some(version) = command_version(&executable, &["--version"]).await else {
        return ManagedToolStatus::broken(
            executable,
            "Managed Portable Git did not report a version",
        );
    };
    ManagedToolStatus {
        installed: true,
        ready: true,
        version: Some(version.clone()),
        path: Some(executable),
        message: Some(format!("Managed Portable Git {version} is ready")),
    }
}

fn node_root_dir() -> PathBuf {
    runtime_dir().join("node")
}

fn node_manifest_path() -> PathBuf {
    node_root_dir().join(NODE_MANIFEST_NAME)
}

fn node_bin_dir_in(install_dir: &Path) -> PathBuf {
    if cfg!(windows) {
        install_dir.to_path_buf()
    } else {
        install_dir.join("bin")
    }
}

fn node_executable_in(install_dir: &Path) -> PathBuf {
    if cfg!(windows) {
        install_dir.join("node.exe")
    } else {
        install_dir.join("bin").join("node")
    }
}

fn git_root_dir() -> PathBuf {
    runtime_dir().join("git")
}

fn git_manifest_path() -> PathBuf {
    git_root_dir().join(GIT_MANIFEST_NAME)
}

fn git_bin_dir_in(install_dir: &Path) -> PathBuf {
    if cfg!(windows) {
        install_dir.join("cmd")
    } else {
        install_dir.join("bin")
    }
}

fn git_executable_in(install_dir: &Path) -> PathBuf {
    if cfg!(windows) {
        install_dir.join("cmd").join("git.exe")
    } else {
        install_dir.join("bin").join("git")
    }
}


fn read_runtime_manifest(path: &Path) -> anyhow::Result<RuntimeManifest> {
    let raw =
        std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    serde_json::from_str(&raw).with_context(|| format!("parsing {}", path.display()))
}

async fn command_version(path: &Path, args: &[&str]) -> Option<String> {
    let mut command = crate::process::env::silent_command(path);
    command.args(args);
    let output = tokio::time::timeout(std::time::Duration::from_secs(8), command.output())
        .await
        .ok()?
        .ok()?;
    if !output.status.success() {
        return None;
    }
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .map(str::to_string)
}

fn version_at_least(current: &str, minimum: &str) -> bool {
    let Some(current) = parse_version_triplet(current) else {
        return false;
    };
    let Some(minimum) = parse_version_triplet(minimum) else {
        return true;
    };
    current >= minimum
}

fn parse_version_triplet(value: &str) -> Option<(u64, u64, u64)> {
    let mut numbers = value
        .trim()
        .trim_start_matches('v')
        .split(|ch: char| !ch.is_ascii_digit())
        .filter(|part| !part.is_empty())
        .filter_map(|part| part.parse::<u64>().ok());
    Some((
        numbers.next()?,
        numbers.next().unwrap_or(0),
        numbers.next().unwrap_or(0),
    ))
}

fn prepend_path(env: &mut HashMap<String, String>, path: PathBuf) {
    if !path.exists() {
        return;
    }
    let current = crate::process::env::path_value(env).unwrap_or_default();
    let path_text = path.to_string_lossy().to_string();
    let mut parts = std::env::split_paths(&current).collect::<Vec<_>>();
    let exists = parts.iter().any(|part| {
        let value = part.to_string_lossy();
        if cfg!(windows) {
            value.eq_ignore_ascii_case(&path_text)
        } else {
            value == path_text
        }
    });
    if exists {
        return;
    }
    parts.insert(0, path);
    match std::env::join_paths(parts) {
        Ok(joined) => {
            crate::process::env::set_path_value(env, joined.to_string_lossy().to_string())
        }
        Err(_) => {
            let separator = if cfg!(windows) { ';' } else { ':' };
            let value = if current.is_empty() {
                path_text
            } else {
                format!("{path_text}{separator}{current}")
            };
            crate::process::env::set_path_value(env, value);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "va-toolchain-test-{name}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("creating scratch dir");
        dir
    }




    /// The runtime manifest is written by the startkit install scripts and only
    /// read here, so the contract worth testing is that this parses exactly what
    /// they emit — not a Rust round-trip.
    #[test]
    fn parses_a_manifest_written_by_an_install_script() {
        let dir = std::env::temp_dir().join(format!(
            "va-toolchain-contract-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("creating scratch dir");
        let path = dir.join(NODE_MANIFEST_NAME);

        // Byte-for-byte what install-managed-node.sh prints.
        std::fs::write(
            &path,
            "{\n  \"version\": \"v24.20.0\",\n  \"install_dir\": \"/Users/a b/.vibearound/runtime/node/versions/v24.20.0\",\n  \"installed_at_unix_ms\": 1788518243000\n}\n",
        )
        .expect("writing script-shaped manifest");

        let manifest = read_runtime_manifest(&path).expect("parsing script output");
        assert_eq!(manifest.version, "v24.20.0");
        assert_eq!(
            manifest.install_dir,
            PathBuf::from("/Users/a b/.vibearound/runtime/node/versions/v24.20.0")
        );
        assert_eq!(manifest.installed_at_unix_ms, 1_788_518_243_000);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn compares_node_versions() {
        assert!(version_at_least("v22.12.0", "22.0.0"));
        assert!(version_at_least("24.1.0", "22.0.0"));
        assert!(!version_at_least("v20.19.0", "22.0.0"));
    }




}
