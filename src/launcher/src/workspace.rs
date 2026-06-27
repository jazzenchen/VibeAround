use std::path::{Path, PathBuf};

use anyhow::Context;

pub fn resolve_workspace_path(workspace: Option<PathBuf>) -> anyhow::Result<PathBuf> {
    let path = match workspace {
        Some(path) => path,
        None => std::env::current_dir().context("resolve current directory")?,
    };
    canonical_workspace_path(&path)
}

pub fn canonical_workspace_path(path: &Path) -> anyhow::Result<PathBuf> {
    let canonical = std::fs::canonicalize(path)
        .with_context(|| format!("workspace does not exist: {}", path.display()))?;
    if !canonical.is_dir() {
        anyhow::bail!("workspace is not a directory: {}", canonical.display());
    }
    Ok(strip_windows_unc_prefix(canonical))
}

fn strip_windows_unc_prefix(path: PathBuf) -> PathBuf {
    #[cfg(target_os = "windows")]
    {
        let value = path.to_string_lossy();
        if let Some(rest) = value.strip_prefix(r"\\?\") {
            if rest.len() >= 2 && rest.as_bytes()[1] == b':' {
                return PathBuf::from(rest.to_string());
            }
        }
        path
    }
    #[cfg(not(target_os = "windows"))]
    {
        path
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_existing_workspace() {
        let dir = temp_dir();
        std::fs::create_dir_all(&dir).expect("create temp workspace");

        let workspace = resolve_workspace_path(Some(dir.clone())).expect("resolve workspace");
        let _ = std::fs::remove_dir_all(&dir);

        assert!(workspace.is_absolute());
    }

    #[test]
    fn rejects_missing_workspace() {
        let dir = temp_dir();

        let error = resolve_workspace_path(Some(dir.clone()))
            .unwrap_err()
            .to_string();

        assert!(error.contains("workspace does not exist"));
    }

    #[test]
    fn rejects_file_workspace() {
        let dir = temp_dir();
        std::fs::create_dir_all(&dir).expect("create temp dir");
        let file = dir.join("file.txt");
        std::fs::write(&file, "not a workspace").expect("write temp file");

        let error = resolve_workspace_path(Some(file)).unwrap_err().to_string();
        let _ = std::fs::remove_dir_all(&dir);

        assert!(error.contains("workspace is not a directory"));
    }

    fn temp_dir() -> PathBuf {
        std::env::temp_dir().join(format!("va-launch-workspace-test-{}", uuid::Uuid::new_v4()))
    }
}
