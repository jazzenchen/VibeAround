//! Small archive helpers for app-managed downloads.
//!
//! VibeAround only extracts archives from trusted release endpoints, but the
//! extraction path still rejects absolute paths and `..` components so a bad
//! archive cannot write outside the intended target directory.

use std::io::{Cursor, Read};
use std::path::{Component, Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

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

pub fn github_head_archive_url(github_url: &str) -> Option<String> {
    let parsed = Url::parse(github_url).ok()?;
    let host = parsed.host_str()?.to_ascii_lowercase();
    if host != "github.com" {
        return None;
    }
    let mut segments = parsed.path_segments()?;
    let owner = segments.next()?.trim();
    let repo = segments.next()?.trim().trim_end_matches(".git");
    if owner.is_empty() || repo.is_empty() {
        return None;
    }
    Some(format!(
        "https://github.com/{owner}/{repo}/archive/HEAD.zip"
    ))
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
        let Some(enclosed) = file.enclosed_name().map(PathBuf::from) else {
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
    if target_dir.exists() {
        std::fs::remove_dir_all(target_dir)
            .with_context(|| format!("removing {}", target_dir.display()))?;
    }
    std::fs::rename(staging_dir, target_dir).with_context(|| {
        format!(
            "moving {} to {}",
            staging_dir.display(),
            target_dir.display()
        )
    })?;
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
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    Ok(parent.join(format!(".{file_name}.{label}.{unique}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_github_archive_urls() {
        assert_eq!(
            github_head_archive_url("https://github.com/acme/demo.git").as_deref(),
            Some("https://github.com/acme/demo/archive/HEAD.zip")
        );
        assert!(github_head_archive_url("https://gitlab.com/acme/demo").is_none());
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
}
