//! Blocking cross-process locks for short file transactions.

use std::fs::{File, OpenOptions};
use std::path::Path;

pub(crate) struct ExclusiveFileLock {
    _file: File,
}

impl ExclusiveFileLock {
    pub(crate) fn acquire(path: &Path) -> std::io::Result<Self> {
        let mut options = OpenOptions::new();
        options.read(true).write(true).create(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let file = options.open(path)?;
        lock(&file)?;
        Ok(Self { _file: file })
    }
}

#[cfg(unix)]
fn lock(file: &File) -> std::io::Result<()> {
    use std::os::fd::AsRawFd;

    loop {
        let result = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX) };
        if result == 0 {
            return Ok(());
        }
        let error = std::io::Error::last_os_error();
        if error.kind() != std::io::ErrorKind::Interrupted {
            return Err(error);
        }
    }
}

#[cfg(windows)]
fn lock(file: &File) -> std::io::Result<()> {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Storage::FileSystem::{LockFileEx, LOCKFILE_EXCLUSIVE_LOCK};
    use windows_sys::Win32::System::IO::OVERLAPPED;

    let mut overlapped = unsafe { std::mem::zeroed::<OVERLAPPED>() };
    let result = unsafe {
        LockFileEx(
            file.as_raw_handle() as _,
            LOCKFILE_EXCLUSIVE_LOCK,
            0,
            u32::MAX,
            u32::MAX,
            &mut overlapped,
        )
    };
    if result == 0 {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(())
    }
}

#[cfg(not(any(unix, windows)))]
compile_error!("VibeAround file locks require a Unix or Windows target");
