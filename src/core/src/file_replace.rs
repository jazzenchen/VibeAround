//! Atomic replacement for user-private files.

use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

/// Write a private sibling file, flush it, then atomically replace `path`.
///
/// The file contents are flushed before replacement. The containing directory
/// is not synced, so this function does not promise crash-durable directory
/// metadata.
pub fn write_private(path: &Path, contents: impl AsRef<[u8]>) -> Result<()> {
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    std::fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;

    let temp = unique_sibling(parent);
    let result = write_and_replace(&temp, path, contents.as_ref());
    if result.is_err() {
        let _ = std::fs::remove_file(&temp);
    }
    result
}

fn unique_sibling(parent: &Path) -> PathBuf {
    parent.join(format!(
        ".vibearound-write-{}-{}",
        std::process::id(),
        nanoid::nanoid!(12)
    ))
}

fn write_and_replace(temp: &Path, target: &Path, contents: &[u8]) -> Result<()> {
    let mut file = create_private_file(temp)
        .with_context(|| format!("create private file {}", temp.display()))?;
    file.write_all(contents)
        .with_context(|| format!("write {}", temp.display()))?;
    file.sync_all()
        .with_context(|| format!("flush {}", temp.display()))?;
    drop(file);

    replace(temp, target).with_context(|| format!("replace {}", target.display()))
}

fn create_private_file(path: &Path) -> std::io::Result<File> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options.open(path)
}

#[cfg(not(windows))]
fn replace(temp: &Path, target: &Path) -> std::io::Result<()> {
    std::fs::rename(temp, target)
}

#[cfg(windows)]
fn replace(temp: &Path, target: &Path) -> std::io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use std::time::Duration;
    use windows_sys::Win32::Foundation::{
        ERROR_ACCESS_DENIED, ERROR_LOCK_VIOLATION, ERROR_SHARING_VIOLATION,
    };
    use windows_sys::Win32::Storage::FileSystem::{
        MoveFileExW, MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH,
    };

    fn wide(path: &Path) -> Vec<u16> {
        path.as_os_str().encode_wide().chain(Some(0)).collect()
    }

    let temp = wide(temp);
    let target = wide(target);
    for attempt in 0..20 {
        let result = unsafe {
            MoveFileExW(
                temp.as_ptr(),
                target.as_ptr(),
                MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
            )
        };
        if result != 0 {
            return Ok(());
        }

        let error = std::io::Error::last_os_error();
        let transient = matches!(
            error.raw_os_error(),
            Some(code)
                if code == ERROR_ACCESS_DENIED as i32
                    || code == ERROR_SHARING_VIOLATION as i32
                    || code == ERROR_LOCK_VIOLATION as i32
        );
        if !transient || attempt == 19 {
            return Err(error);
        }

        // Concurrent MoveFileExW calls can briefly observe the destination as
        // delete-pending. Let that replacement finish, then retry our unique
        // source file with a small bounded backoff.
        std::thread::sleep(Duration::from_millis(1 << attempt.min(4)));
    }
    unreachable!()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_dir(label: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "vibearound-file-replace-{label}-{}-{}",
            std::process::id(),
            nanoid::nanoid!(8)
        ))
    }

    #[cfg(unix)]
    #[test]
    fn temporary_file_is_private_when_created() {
        use std::os::unix::fs::PermissionsExt;

        let dir = test_dir("private-temp");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("temp");
        let file = create_private_file(&path).unwrap();

        assert_eq!(file.metadata().unwrap().permissions().mode() & 0o777, 0o600);

        drop(file);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn replaces_existing_contents() {
        let dir = test_dir("replace");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("settings.json");
        std::fs::write(&path, b"first").unwrap();

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
        }

        write_private(&path, b"second").unwrap();

        assert_eq!(std::fs::read(&path).unwrap(), b"second");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn concurrent_writers_leave_one_complete_file_and_no_temps() {
        let dir = test_dir("concurrent");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("settings.json");
        let handles = (0..8)
            .map(|value| {
                let path = path.clone();
                std::thread::spawn(move || write_private(&path, value.to_string()))
            })
            .collect::<Vec<_>>();

        for handle in handles {
            handle.join().unwrap().unwrap();
        }

        let value = std::fs::read_to_string(&path).unwrap();
        assert!(value.parse::<u8>().is_ok_and(|value| value < 8));
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 1);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
