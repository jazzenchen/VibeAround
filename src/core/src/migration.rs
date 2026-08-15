//! One-time migrations for files under the VibeAround data directory.

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};

const LEGACY_STATE_FILES: [&str; 2] = ["workspaces.jsonl", "workspace-threads.jsonl"];

pub fn run() -> Result<()> {
    run_at(&crate::config::data_dir())
}

fn run_at(data_dir: &Path) -> Result<()> {
    std::fs::create_dir_all(data_dir)
        .with_context(|| format!("create data directory {}", data_dir.display()))?;
    let _lock = crate::file_lock::ExclusiveFileLock::acquire(&data_dir.join("migration.lock"))
        .with_context(|| format!("lock migrations in {}", data_dir.display()))?;

    let changes = legacy_state_changes(data_dir);
    if changes.is_empty() {
        return Ok(());
    }

    let backup_dir = create_backup(data_dir, changes.iter().map(|change| &change.source))?;
    for change in changes {
        apply_state_change(&change)?;
    }
    tracing::info!(backup = ?backup_dir, "completed configuration migration");
    Ok(())
}

struct StateChange {
    source: PathBuf,
    target: PathBuf,
}

fn legacy_state_changes(data_dir: &Path) -> Vec<StateChange> {
    LEGACY_STATE_FILES
        .iter()
        .filter_map(|name| {
            let source = data_dir.join(name);
            source.exists().then(|| StateChange {
                source,
                target: data_dir.join("state").join(name),
            })
        })
        .collect()
}

fn create_backup<'a>(
    data_dir: &Path,
    sources: impl IntoIterator<Item = &'a PathBuf>,
) -> Result<PathBuf> {
    let backup_root = data_dir.join("migration-backups");
    create_private_dir(&backup_root)?;
    let backup_dir = backup_root.join(format!(
        "migration-{}-{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos(),
        std::process::id()
    ));
    create_private_dir(&backup_dir)?;

    for source in sources {
        let relative = source
            .strip_prefix(data_dir)
            .with_context(|| format!("{} is outside {}", source.display(), data_dir.display()))?;
        let target = backup_dir.join(relative);
        if let Some(parent) = target.parent() {
            create_private_dir(parent)?;
        }
        std::fs::copy(source, &target)
            .with_context(|| format!("back up {} to {}", source.display(), target.display()))?;
        make_private_file(&target)?;
    }

    Ok(backup_dir)
}

fn apply_state_change(change: &StateChange) -> Result<()> {
    if change.target.exists() {
        std::fs::remove_file(&change.source)
            .with_context(|| format!("remove migrated {}", change.source.display()))?;
        return Ok(());
    }

    if let Some(parent) = change.target.parent() {
        create_private_dir(parent)?;
    }
    std::fs::rename(&change.source, &change.target).with_context(|| {
        format!(
            "move {} to {}",
            change.source.display(),
            change.target.display()
        )
    })
}

fn create_private_dir(path: &Path) -> Result<()> {
    std::fs::create_dir_all(path).with_context(|| format!("create {}", path.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))
            .with_context(|| format!("set permissions on {}", path.display()))?;
    }
    Ok(())
}

fn make_private_file(path: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
            .with_context(|| format!("set permissions on {}", path.display()))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_dir() -> PathBuf {
        std::env::temp_dir().join(format!(
            "vibearound-migration-{}-{}",
            std::process::id(),
            nanoid::nanoid!(8)
        ))
    }

    #[test]
    fn backs_up_then_moves_legacy_state_files_once() {
        let dir = test_dir();
        std::fs::create_dir_all(dir.join("state")).unwrap();
        std::fs::write(dir.join("workspaces.jsonl"), "legacy-workspaces\n").unwrap();
        std::fs::write(dir.join("workspace-threads.jsonl"), "legacy-threads\n").unwrap();
        std::fs::write(
            dir.join("state/workspace-threads.jsonl"),
            "current-threads\n",
        )
        .unwrap();

        run_at(&dir).unwrap();

        assert!(!dir.join("workspaces.jsonl").exists());
        assert!(!dir.join("workspace-threads.jsonl").exists());
        assert_eq!(
            std::fs::read_to_string(dir.join("state/workspaces.jsonl")).unwrap(),
            "legacy-workspaces\n"
        );
        assert_eq!(
            std::fs::read_to_string(dir.join("state/workspace-threads.jsonl")).unwrap(),
            "current-threads\n"
        );

        let backups = backup_dirs(&dir);
        assert_eq!(backups.len(), 1);
        assert_eq!(
            std::fs::read_to_string(backups[0].join("workspaces.jsonl")).unwrap(),
            "legacy-workspaces\n"
        );
        assert_eq!(
            std::fs::read_to_string(backups[0].join("workspace-threads.jsonl")).unwrap(),
            "legacy-threads\n"
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(&backups[0]).unwrap().permissions().mode() & 0o777,
                0o700
            );
            assert_eq!(
                std::fs::metadata(backups[0].join("workspaces.jsonl"))
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777,
                0o600
            );
        }

        run_at(&dir).unwrap();
        assert_eq!(backup_dirs(&dir).len(), 1);

        std::fs::remove_dir_all(dir).unwrap();
    }

    fn backup_dirs(data_dir: &Path) -> Vec<PathBuf> {
        let mut dirs = std::fs::read_dir(data_dir.join("migration-backups"))
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .filter(|path| path.is_dir())
            .collect::<Vec<_>>();
        dirs.sort();
        dirs
    }
}
