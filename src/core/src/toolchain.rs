//! VibeAround-managed portable toolchain.
//!
//! Managed mode keeps Node.js and selected helper tools under
//! `~/.vibearound/runtime` and exposes them to child processes through
//! `process::env::child_env()`. Scans are local-only; installers perform the
//! network work and update a small manifest once the extracted tool is usable.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{anyhow, bail, Context};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::archive::{self, ArchiveFormat};

const DEFAULT_NODE_INDEX_URL: &str = "https://nodejs.org/dist/index.json";
const DEFAULT_NODE_DIST_BASE: &str = "https://nodejs.org/dist";
const NODE_MANIFEST_NAME: &str = "current.json";
const GIT_MANIFEST_NAME: &str = "current.json";
const GIT_FOR_WINDOWS_STABLE_RELEASE_API: &str =
    "https://api.github.com/repos/git-for-windows/git/releases/latest";

#[derive(Debug, Clone)]
pub struct NodeSource {
    pub index_url: String,
    pub dist_base: String,
}

impl Default for NodeSource {
    fn default() -> Self {
        Self {
            index_url: DEFAULT_NODE_INDEX_URL.to_string(),
            dist_base: DEFAULT_NODE_DIST_BASE.to_string(),
        }
    }
}

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

#[derive(Debug, Clone)]
struct NodeRelease {
    version: String,
    archive_name: String,
    archive_url: String,
    shasums_url: String,
    format: ArchiveFormat,
}

#[derive(Debug, Clone)]
struct NodeTarget {
    file_key: &'static str,
    archive_platform: &'static str,
    suffix: &'static str,
    format: ArchiveFormat,
}

#[derive(Debug, Deserialize)]
struct NodeIndexEntry {
    version: String,
    #[serde(default)]
    lts: serde_json::Value,
    #[serde(default)]
    files: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct GitHubRelease {
    tag_name: String,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    prerelease: bool,
    #[serde(default)]
    assets: Vec<GitHubReleaseAsset>,
}

#[derive(Debug, Deserialize)]
struct GitHubReleaseAsset {
    name: String,
    browser_download_url: String,
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

pub async fn ensure_node_lts<F, C>(
    source: &NodeSource,
    mut on_log: F,
    is_cancelled: C,
) -> anyhow::Result<ManagedToolStatus>
where
    F: FnMut(String),
    C: Fn() -> bool,
{
    let release = latest_node_lts_release(source).await?;
    if is_cancelled() {
        bail!("install cancelled");
    }

    let current = managed_node_status(None).await;
    if current.ready && current.version.as_deref() == Some(release.version.as_str()) {
        return Ok(current);
    }

    on_log(format!("Downloading Node.js {}", release.version));
    let bytes = archive::download_bytes(&release.archive_url).await?;
    if is_cancelled() {
        bail!("install cancelled");
    }

    on_log("Verifying Node.js archive".to_string());
    verify_node_archive(&release, &bytes).await?;
    if is_cancelled() {
        bail!("install cancelled");
    }

    let install_dir = node_version_dir(&release.version);
    if node_executable_in(&install_dir).exists() {
        write_runtime_manifest(&node_manifest_path(), &release.version, &install_dir)?;
        return Ok(managed_node_status(None).await);
    }

    let staging_dir = archive::staging_dir_for(&install_dir, "node")?;
    archive::recreate_dir(&staging_dir)?;
    on_log(format!("Extracting Node.js {}", release.version));
    archive::extract_bytes_strip_root(bytes, release.format, &staging_dir).await?;
    if !node_executable_in(&staging_dir).exists() {
        bail!(
            "Node.js archive did not contain {}",
            node_executable_in(&staging_dir).display()
        );
    }

    archive::atomic_replace_dir(&staging_dir, &install_dir)?;
    write_runtime_manifest(&node_manifest_path(), &release.version, &install_dir)?;
    Ok(managed_node_status(None).await)
}

pub async fn ensure_windows_portable_git<F, C>(
    mut on_log: F,
    is_cancelled: C,
) -> anyhow::Result<ManagedToolStatus>
where
    F: FnMut(String),
    C: Fn() -> bool,
{
    if !cfg!(windows) {
        bail!("Managed Portable Git is only enabled on Windows for now");
    }

    let release = latest_stable_git_for_windows_release().await?;
    let current = managed_git_status().await;
    if current.ready
        && current
            .version
            .as_deref()
            .is_some_and(|version| version.contains(release.tag_name.trim_start_matches('v')))
    {
        return Ok(current);
    }
    if is_cancelled() {
        bail!("install cancelled");
    }

    let asset = select_portable_git_asset(&release)
        .ok_or_else(|| anyhow!("PortableGit asset not found in {}", release.tag_name))?;
    on_log(format!("Downloading {}", asset.name));
    let bytes = archive::download_bytes(&asset.browser_download_url).await?;
    if is_cancelled() {
        bail!("install cancelled");
    }

    let download_dir = runtime_dir().join("downloads");
    std::fs::create_dir_all(&download_dir)
        .with_context(|| format!("creating {}", download_dir.display()))?;
    let installer = download_dir.join(&asset.name);
    std::fs::write(&installer, bytes)
        .with_context(|| format!("writing {}", installer.display()))?;

    let install_dir = git_version_dir(&release.tag_name);
    let staging_dir = archive::staging_dir_for(&install_dir, "git")?;
    archive::recreate_dir(&staging_dir)?;
    on_log(format!("Extracting {}", asset.name));

    let output = crate::process::env::silent_command(&installer)
        .arg("-y")
        .arg(format!("-o{}", staging_dir.display()))
        .output()
        .await
        .with_context(|| format!("running {}", installer.display()))?;
    if !output.status.success() {
        bail!(
            "PortableGit extractor failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    if !git_executable_in(&staging_dir).exists() {
        bail!(
            "PortableGit archive did not contain {}",
            git_executable_in(&staging_dir).display()
        );
    }

    archive::atomic_replace_dir(&staging_dir, &install_dir)?;
    write_runtime_manifest(&git_manifest_path(), &release.tag_name, &install_dir)?;
    Ok(managed_git_status().await)
}

async fn latest_node_lts_release(source: &NodeSource) -> anyhow::Result<NodeRelease> {
    let target = NodeTarget::current()
        .ok_or_else(|| anyhow!("managed Node.js is not available for this platform"))?;
    let bytes = archive::download_bytes(&source.index_url).await?;
    let entries: Vec<NodeIndexEntry> =
        serde_json::from_slice(&bytes).context("parsing Node.js index")?;
    let entry = latest_node_lts_entry(entries, target.file_key)
        .ok_or_else(|| anyhow!("no Node.js LTS archive found for {}", target.file_key))?;
    let archive_name = target.archive_name(&entry.version);
    let base = source.dist_base.trim_end_matches('/');
    Ok(NodeRelease {
        version: entry.version.clone(),
        archive_url: format!("{base}/{}/{}", entry.version, archive_name),
        shasums_url: format!("{base}/{}/SHASUMS256.txt", entry.version),
        archive_name,
        format: target.format,
    })
}

fn latest_node_lts_entry(entries: Vec<NodeIndexEntry>, file_key: &str) -> Option<NodeIndexEntry> {
    entries
        .into_iter()
        .filter(|entry| is_lts_value(&entry.lts) && entry.files.iter().any(|file| file == file_key))
        .max_by_key(|entry| parse_version_triplet(&entry.version).unwrap_or((0, 0, 0)))
}

async fn verify_node_archive(release: &NodeRelease, bytes: &[u8]) -> anyhow::Result<()> {
    let shasums = archive::download_bytes(&release.shasums_url).await?;
    let shasums = String::from_utf8(shasums).context("reading Node.js SHASUMS256.txt")?;
    let expected = shasums
        .lines()
        .filter_map(|line| {
            let mut parts = line.split_whitespace();
            let digest = parts.next()?;
            let name = parts.next()?;
            (name == release.archive_name).then_some(digest)
        })
        .next()
        .ok_or_else(|| anyhow!("checksum for {} not found", release.archive_name))?;
    let actual = format!("{:x}", Sha256::digest(bytes));
    if actual != expected {
        bail!(
            "checksum mismatch for {}: expected {}, got {}",
            release.archive_name,
            expected,
            actual
        );
    }
    Ok(())
}

async fn latest_stable_git_for_windows_release() -> anyhow::Result<GitHubRelease> {
    let bytes = archive::download_bytes(GIT_FOR_WINDOWS_STABLE_RELEASE_API).await?;
    let release: GitHubRelease =
        serde_json::from_slice(&bytes).context("parsing Git for Windows release")?;
    if release.draft || release.prerelease {
        bail!("Git for Windows release {} is not stable", release.tag_name);
    }
    Ok(release)
}

fn select_portable_git_asset(release: &GitHubRelease) -> Option<&GitHubReleaseAsset> {
    let arch_marker = if cfg!(target_arch = "aarch64") {
        "arm64"
    } else {
        "64-bit"
    };
    release.assets.iter().find(|asset| {
        let name = asset.name.to_ascii_lowercase();
        name.starts_with("portablegit-") && name.ends_with(".7z.exe") && name.contains(arch_marker)
    })
}

impl NodeTarget {
    fn current() -> Option<Self> {
        match (std::env::consts::OS, std::env::consts::ARCH) {
            ("macos", "aarch64") => Some(Self {
                file_key: "osx-arm64-tar",
                archive_platform: "darwin-arm64",
                suffix: "tar.gz",
                format: ArchiveFormat::TarGz,
            }),
            ("macos", "x86_64") => Some(Self {
                file_key: "osx-x64-tar",
                archive_platform: "darwin-x64",
                suffix: "tar.gz",
                format: ArchiveFormat::TarGz,
            }),
            #[cfg(target_os = "linux")]
            ("linux", "aarch64") => Some(Self {
                file_key: "linux-arm64",
                archive_platform: "linux-arm64",
                suffix: "tar.xz",
                format: ArchiveFormat::TarXz,
            }),
            #[cfg(target_os = "linux")]
            ("linux", "x86_64") => Some(Self {
                file_key: "linux-x64",
                archive_platform: "linux-x64",
                suffix: "tar.xz",
                format: ArchiveFormat::TarXz,
            }),
            ("windows", "aarch64") => Some(Self {
                file_key: "win-arm64-zip",
                archive_platform: "win-arm64",
                suffix: "zip",
                format: ArchiveFormat::Zip,
            }),
            ("windows", "x86_64") => Some(Self {
                file_key: "win-x64-zip",
                archive_platform: "win-x64",
                suffix: "zip",
                format: ArchiveFormat::Zip,
            }),
            _ => None,
        }
    }

    fn archive_name(&self, version: &str) -> String {
        format!("node-{version}-{}.{}", self.archive_platform, self.suffix)
    }
}

fn node_root_dir() -> PathBuf {
    runtime_dir().join("node")
}

fn node_versions_dir() -> PathBuf {
    node_root_dir().join("versions")
}

fn node_manifest_path() -> PathBuf {
    node_root_dir().join(NODE_MANIFEST_NAME)
}

fn node_version_dir(version: &str) -> PathBuf {
    node_versions_dir().join(sanitize_version_path(version))
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

fn git_versions_dir() -> PathBuf {
    git_root_dir().join("versions")
}

fn git_manifest_path() -> PathBuf {
    git_root_dir().join(GIT_MANIFEST_NAME)
}

fn git_version_dir(version: &str) -> PathBuf {
    git_versions_dir().join(sanitize_version_path(version))
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

fn write_runtime_manifest(path: &Path, version: &str, install_dir: &Path) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("creating {}", parent.display()))?;
    }
    let manifest = RuntimeManifest {
        version: version.to_string(),
        install_dir: install_dir.to_path_buf(),
        installed_at_unix_ms: now_unix_ms(),
    };
    let json = serde_json::to_string_pretty(&manifest).context("serializing runtime manifest")?;
    std::fs::write(path, json).with_context(|| format!("writing {}", path.display()))?;
    Ok(())
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

fn is_lts_value(value: &serde_json::Value) -> bool {
    !matches!(
        value,
        serde_json::Value::Bool(false) | serde_json::Value::Null
    )
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

fn sanitize_version_path(version: &str) -> String {
    version
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '.' | '-' | '_') {
                ch
            } else {
                '_'
            }
        })
        .collect()
}

fn now_unix_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or(0)
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

    #[test]
    fn compares_node_versions() {
        assert!(version_at_least("v22.12.0", "22.0.0"));
        assert!(version_at_least("24.1.0", "22.0.0"));
        assert!(!version_at_least("v20.19.0", "22.0.0"));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn node_archive_name_uses_platform_fragment() {
        let target = NodeTarget {
            file_key: "linux-x64",
            archive_platform: "linux-x64",
            suffix: "tar.xz",
            format: ArchiveFormat::TarXz,
        };
        assert_eq!(
            target.archive_name("v24.11.1"),
            "node-v24.11.1-linux-x64.tar.xz"
        );
    }

    #[test]
    fn latest_node_lts_entry_does_not_depend_on_index_order() {
        let entries = vec![
            NodeIndexEntry {
                version: "v20.19.4".to_string(),
                lts: serde_json::Value::String("Iron".to_string()),
                files: vec!["linux-x64".to_string()],
            },
            NodeIndexEntry {
                version: "v24.1.0".to_string(),
                lts: serde_json::Value::Bool(false),
                files: vec!["linux-x64".to_string()],
            },
            NodeIndexEntry {
                version: "v22.18.0".to_string(),
                lts: serde_json::Value::String("Jod".to_string()),
                files: vec!["linux-x64".to_string()],
            },
            NodeIndexEntry {
                version: "v22.19.0".to_string(),
                lts: serde_json::Value::String("Jod".to_string()),
                files: vec!["linux-arm64".to_string()],
            },
        ];

        let selected = latest_node_lts_entry(entries, "linux-x64").expect("latest LTS entry");
        assert_eq!(selected.version, "v22.18.0");
    }

    #[test]
    fn selects_portable_git_asset_for_x64() {
        let release = GitHubRelease {
            tag_name: "v2.50.0.windows.1".to_string(),
            draft: false,
            prerelease: false,
            assets: vec![GitHubReleaseAsset {
                name: "PortableGit-2.50.0-64-bit.7z.exe".to_string(),
                browser_download_url: "https://example.test/git.exe".to_string(),
            }],
        };
        if cfg!(target_arch = "x86_64") {
            assert!(select_portable_git_asset(&release).is_some());
        }
    }
}
