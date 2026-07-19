//! Small archive helpers for app-managed downloads.
//!
//! VibeAround only extracts archives from trusted release endpoints, but the
//! extraction path still rejects absolute paths and `..` components so a bad
//! archive cannot write outside the intended target directory.

use std::io::{Cursor, Read};
use std::path::{Component, Path, PathBuf};

use anyhow::{anyhow, Context};
use url::Url;

const USER_AGENT: &str = concat!("VibeAround/", env!("CARGO_PKG_VERSION"));

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArchiveFormat {
    TarGz,
    #[cfg(target_os = "linux")]
    TarXz,
    Zip,
}

pub async fn download_bytes(url: &str) -> anyhow::Result<Vec<u8>> {
    let client = reqwest::Client::builder()
        .user_agent(USER_AGENT)
        .build()
        .context("creating download client")?;
    let response = client
        .get(url)
        .send()
        .await
        .with_context(|| format!("downloading {url}"))?
        .error_for_status()
        .with_context(|| format!("downloading {url}"))?;
    Ok(response
        .bytes()
        .await
        .with_context(|| format!("reading {url}"))?
        .to_vec())
}

pub async fn download_and_extract_strip_root(
    url: &str,
    format: ArchiveFormat,
    target_dir: &Path,
) -> anyhow::Result<()> {
    let bytes = download_bytes(url).await?;
    extract_bytes_strip_root(bytes, format, target_dir).await
}

pub async fn extract_bytes_strip_root(
    bytes: Vec<u8>,
    format: ArchiveFormat,
    target_dir: &Path,
) -> anyhow::Result<()> {
    let target_dir = target_dir.to_path_buf();
    tokio::task::spawn_blocking(move || {
        extract_bytes_strip_root_blocking(&bytes, format, &target_dir)
    })
    .await
    .context("joining archive extractor")?
}

pub fn github_revision_archive_url(github_url: &str, revision: &str) -> Option<String> {
    let (owner, repo) = github_repository(github_url, revision)?;
    Some(format!(
        "https://github.com/{owner}/{repo}/archive/{revision}.zip"
    ))
}

pub fn github_revision_raw_file_url(
    github_url: &str,
    revision: &str,
    file_name: &str,
) -> Option<String> {
    if file_name.is_empty()
        || !file_name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
    {
        return None;
    }
    let (owner, repo) = github_repository(github_url, revision)?;
    Some(format!(
        "https://raw.githubusercontent.com/{owner}/{repo}/{revision}/{file_name}"
    ))
}

fn github_repository(github_url: &str, revision: &str) -> Option<(String, String)> {
    if revision.len() != 40 || !revision.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return None;
    }
    let parsed = Url::parse(github_url).ok()?;
    if parsed.scheme() != "https"
        || !parsed.username().is_empty()
        || parsed.password().is_some()
        || parsed.port().is_some()
        || parsed.query().is_some()
        || parsed.fragment().is_some()
    {
        return None;
    }
    let host = parsed.host_str()?.to_ascii_lowercase();
    if host != "github.com" {
        return None;
    }
    let segments = parsed.path_segments()?.collect::<Vec<_>>();
    if segments.len() != 2 {
        return None;
    }
    let owner = segments[0].trim();
    let repo = segments[1].trim().trim_end_matches(".git");
    if owner.is_empty() || repo.is_empty() || repo == "." || repo == ".." {
        return None;
    }
    Some((owner.to_string(), repo.to_string()))
}

fn extract_bytes_strip_root_blocking(
    bytes: &[u8],
    format: ArchiveFormat,
    target_dir: &Path,
) -> anyhow::Result<()> {
    std::fs::create_dir_all(target_dir)
        .with_context(|| format!("creating {}", target_dir.display()))?;
    match format {
        ArchiveFormat::TarGz => {
            extract_tar_strip_root(flate2::read::GzDecoder::new(Cursor::new(bytes)), target_dir)
        }
        #[cfg(target_os = "linux")]
        ArchiveFormat::TarXz => {
            extract_tar_strip_root(xz2::read::XzDecoder::new(Cursor::new(bytes)), target_dir)
        }
        ArchiveFormat::Zip => extract_zip_strip_root(bytes, target_dir),
    }
}

fn extract_tar_strip_root<R: Read>(reader: R, target_dir: &Path) -> anyhow::Result<()> {
    let mut archive = tar::Archive::new(reader);
    for entry in archive.entries().context("reading tar archive")? {
        let mut entry = entry.context("reading tar entry")?;
        let entry_path = entry.path().context("reading tar entry path")?;
        let Some(relative) = relative_after_archive_root(&entry_path) else {
            continue;
        };
        let destination = target_dir.join(relative);
        if let Some(parent) = destination.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("creating {}", parent.display()))?;
        }
        entry
            .unpack(&destination)
            .with_context(|| format!("extracting {}", destination.display()))?;
    }
    Ok(())
}

fn extract_zip_strip_root(bytes: &[u8], target_dir: &Path) -> anyhow::Result<()> {
    let reader = Cursor::new(bytes);
    let mut archive = zip::ZipArchive::new(reader).context("reading zip archive")?;
    for index in 0..archive.len() {
        let mut file = archive.by_index(index).context("reading zip entry")?;
        let Some(enclosed) = file.enclosed_name() else {
            continue;
        };
        let Some(relative) = relative_after_archive_root(&enclosed) else {
            continue;
        };
        let destination = target_dir.join(relative);
        if file.is_dir() {
            std::fs::create_dir_all(&destination)
                .with_context(|| format!("creating {}", destination.display()))?;
            continue;
        }
        if let Some(parent) = destination.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("creating {}", parent.display()))?;
        }
        let mut output = std::fs::File::create(&destination)
            .with_context(|| format!("creating {}", destination.display()))?;
        std::io::copy(&mut file, &mut output)
            .with_context(|| format!("extracting {}", destination.display()))?;
    }
    Ok(())
}

fn relative_after_archive_root(path: &Path) -> Option<PathBuf> {
    let mut components = path.components();
    match components.next()? {
        Component::Normal(_) | Component::CurDir => {}
        _ => return None,
    }

    let mut relative = PathBuf::new();
    for component in components {
        match component {
            Component::Normal(value) => relative.push(value),
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => return None,
        }
    }
    (!relative.as_os_str().is_empty()).then_some(relative)
}

pub fn recreate_dir(path: &Path) -> anyhow::Result<()> {
    if path.exists() {
        std::fs::remove_dir_all(path).with_context(|| format!("removing {}", path.display()))?;
    }
    std::fs::create_dir_all(path).with_context(|| format!("creating {}", path.display()))?;
    Ok(())
}

pub fn atomic_replace_dir(staging_dir: &Path, target_dir: &Path) -> anyhow::Result<()> {
    let backup_dir = staging_dir_for(target_dir, "backup")?;
    let had_target = target_dir.exists();
    if had_target {
        std::fs::rename(target_dir, &backup_dir).with_context(|| {
            format!(
                "moving current {} to backup {}",
                target_dir.display(),
                backup_dir.display()
            )
        })?;
    }

    if let Err(install_error) = std::fs::rename(staging_dir, target_dir) {
        if had_target {
            std::fs::rename(&backup_dir, target_dir).with_context(|| {
                format!(
                    "install failed ({install_error}); restoring backup {} to {}",
                    backup_dir.display(),
                    target_dir.display()
                )
            })?;
        }
        return Err(install_error).with_context(|| {
            format!(
                "moving {} to {}",
                staging_dir.display(),
                target_dir.display()
            )
        });
    }

    if had_target {
        if let Err(error) = std::fs::remove_dir_all(&backup_dir) {
            tracing::warn!(
                path = %backup_dir.display(),
                %error,
                "installed replacement but could not remove plugin backup"
            );
        }
    }
    Ok(())
}

pub fn staging_dir_for(target_dir: &Path, label: &str) -> anyhow::Result<PathBuf> {
    let parent = target_dir
        .parent()
        .ok_or_else(|| anyhow!("{} has no parent directory", target_dir.display()))?;
    let file_name = target_dir
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("download");
    let unique = uuid::Uuid::new_v4();
    Ok(parent.join(format!(".{file_name}.{label}.{unique}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_pinned_github_archive_urls() {
        let revision = "0123456789abcdef0123456789abcdef01234567";
        assert_eq!(
            github_revision_archive_url("https://github.com/acme/demo.git", revision).as_deref(),
            Some(
                "https://github.com/acme/demo/archive/0123456789abcdef0123456789abcdef01234567.zip"
            )
        );
        assert!(github_revision_archive_url("https://gitlab.com/acme/demo", revision).is_none());
        assert!(
            github_revision_archive_url("https://github.com/acme/demo/tree/main", revision)
                .is_none()
        );
        assert!(github_revision_archive_url("https://github.com/acme/demo", "main").is_none());
        assert_eq!(
            github_revision_raw_file_url(
                "https://github.com/acme/demo.git",
                revision,
                "plugin.json"
            )
            .as_deref(),
            Some("https://raw.githubusercontent.com/acme/demo/0123456789abcdef0123456789abcdef01234567/plugin.json")
        );
        assert!(github_revision_raw_file_url(
            "https://github.com/acme/demo",
            revision,
            "../plugin.json"
        )
        .is_none());
    }

    #[test]
    fn strips_archive_root_and_rejects_escape_paths() {
        assert_eq!(
            relative_after_archive_root(Path::new("repo-main/plugin.json")).as_deref(),
            Some(Path::new("plugin.json"))
        );
        assert!(relative_after_archive_root(Path::new("repo-main/../bad")).is_none());
        assert!(relative_after_archive_root(Path::new("/repo-main/bad")).is_none());
    }

    #[test]
    fn restores_existing_directory_when_replacement_fails() {
        let root = std::env::temp_dir().join(format!("va-dir-replace-{}", uuid::Uuid::new_v4()));
        let target = root.join("plugin");
        std::fs::create_dir_all(&target).unwrap();
        std::fs::write(target.join("marker"), "current").unwrap();

        let error = atomic_replace_dir(&root.join("missing-staging"), &target).unwrap_err();

        assert!(error.to_string().contains("moving"));
        assert_eq!(
            std::fs::read_to_string(target.join("marker")).unwrap(),
            "current"
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn replaces_existing_directory_after_staging_succeeds() {
        let root = std::env::temp_dir().join(format!("va-dir-replace-{}", uuid::Uuid::new_v4()));
        let target = root.join("plugin");
        let staging = root.join("staging");
        std::fs::create_dir_all(&target).unwrap();
        std::fs::create_dir_all(&staging).unwrap();
        std::fs::write(target.join("old"), "old").unwrap();
        std::fs::write(staging.join("new"), "new").unwrap();

        atomic_replace_dir(&staging, &target).unwrap();

        assert!(!target.join("old").exists());
        assert_eq!(std::fs::read_to_string(target.join("new")).unwrap(), "new");
        assert_eq!(std::fs::read_dir(&root).unwrap().count(), 1);
        std::fs::remove_dir_all(root).unwrap();
    }
}
