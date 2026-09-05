//! Process lease: no supervised child outlives the daemon, however the
//! daemon dies.
//!
//! A child in its own process group is not killed by its parent's death,
//! so a daemon that panics, gets `SIGKILL`ed, or is force-quit would leave
//! every plugin, agent, tunnel, and search provider running against a port
//! nobody listens on. The lease closes that gap with a kernel-held handle
//! instead of daemon-side bookkeeping.
//!
//! **Unix.** One tiny `sh` reaper per supervisor holds the read end of a
//! pipe whose write end only the daemon owns. Right after a spawn
//! succeeds the supervisor writes `add <pgid>` into the pipe; after it has
//! reaped the child it writes `del <pgid>`. When the daemon exits for any
//! reason the kernel closes the pipe, the reaper reads EOF, `SIGTERM`s
//! every leased process group, waits a second, `SIGKILL`s the survivors,
//! and exits. The reaper is its own process group so nothing aimed at the
//! daemon's group reaches it.
//!
//! **Windows.** A Job Object with kill-on-close. Each child is assigned
//! right after it is spawned; everything it starts inherits membership.
//! The daemon holds the only handle, so its death closes the job and the
//! kernel terminates every member.
//!
//! Deliberately not covered, on both platforms: the moment between the
//! spawn and the registration (about a millisecond) — a daemon killed
//! exactly then leaves that one child behind, and a kill at that instant
//! is either a bug to fix or the user's own doing. On Unix, the reaper
//! itself being killed by hand: from then on new children run uncovered,
//! with a warning in the log.
//!
//! Orderly stops still go through the supervisor (`SIGTERM`, then
//! `SIGKILL`, per child); the lease only ever acts after the daemon is gone.

#[cfg(unix)]
mod imp {
    use std::io::Write;
    use std::os::fd::OwnedFd;
    use std::os::unix::process::CommandExt;
    use std::process::{Command, Stdio};

    use tokio::process::Child;

    /// POSIX `sh`. Reads `add <pgid>` / `del <pgid>` lines until EOF, then
    /// signals whatever is still leased. `HUP`/`INT` are ignored so a
    /// closing terminal or Ctrl-C aimed at the daemon cannot take the
    /// reaper down before it has done its job. Written to run under
    /// whatever `/bin/sh` is: the list lives in the positional parameters
    /// (zsh outside sh emulation does not word-split a variable), and
    /// `kill -SIG -pgid` carries no `--` (dash's builtin rejects it).
    const REAPER_SCRIPT: &str = r#"
trap '' HUP INT
while IFS= read -r line; do
  case "$line" in
    "add "*) set -- "$@" "${line#add }" ;;
    "del "*) x="${line#del }"; n=$#; while [ "$n" -gt 0 ]; do p=$1; shift; [ "$p" = "$x" ] || set -- "$@" "$p"; n=$((n-1)); done ;;
  esac
done
for p in "$@"; do kill -TERM "-$p" 2>/dev/null; done
[ $# -gt 0 ] && sleep 1
for p in "$@"; do kill -KILL "-$p" 2>/dev/null; done
"#;

    pub struct Lease {
        /// Write end of the reaper's stdin (blocking, close-on-exec).
        pipe: std::fs::File,
    }

    impl Lease {
        /// Start the reaper. A daemon that cannot run `sh` cannot run any
        /// of its children either, so this is fatal.
        // The reaper outlives this process by design, so it is never
        // waited on; it only ever exits after we are gone.
        #[allow(clippy::zombie_processes)]
        pub fn new() -> Self {
            let mut reaper = Command::new("sh")
                .arg("-c")
                .arg(REAPER_SCRIPT)
                .stdin(Stdio::piped())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .process_group(0)
                .spawn()
                .expect("process lease: failed to start the reaper (is `sh` on PATH?)");
            // The reaper must outlive us: its handle is dropped without
            // waiting and it is never killed.
            let stdin = reaper.stdin.take().expect("reaper stdin is piped");
            Self {
                pipe: std::fs::File::from(OwnedFd::from(stdin)),
            }
        }

        /// Lease the process group rooted at `child`, spawned as its own
        /// group leader (see `kill::prepare_tree_root`), so pgid == pid.
        /// Call right after `spawn` succeeds.
        pub fn attach(&self, child: &Child) {
            if let Some(pid) = child.id() {
                self.write(format!("add {pid}\n"));
            }
        }

        /// The group has been reaped by the supervisor; the reaper must not
        /// touch its (possibly recycled) pgid later.
        pub fn release(&self, pid: u32) {
            self.write(format!("del {pid}\n"));
        }

        /// One `write(2)` per line: short pipe writes are atomic, so owner
        /// tasks on different threads never interleave.
        fn write(&self, line: String) {
            if let Err(error) = (&self.pipe).write_all(line.as_bytes()) {
                tracing::warn!(
                    "[lease] reaper unreachable ({error}); children spawned from now on are not covered"
                );
            }
        }
    }

    #[cfg(test)]
    mod tests {
        use std::process::Stdio;
        use std::time::Duration;
        use tokio::process::Command;

        /// Ignores SIGTERM (the disposition survives exec) so only the
        /// reaper's SIGKILL fallback can end it. Its own group leader.
        fn stubborn_sleeper() -> tokio::process::Child {
            let mut command = Command::new("sh");
            command
                .args(["-c", "trap '' TERM; exec sleep 30"])
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null());
            crate::process::kill::prepare_tree_root(&mut command);
            command.spawn().expect("spawn sleeper")
        }

        /// `/bin/sh` is bash on macOS, dash on Debian and Ubuntu, sometimes
        /// zsh or ksh: the script must kill under every one of them. Shells
        /// not installed here are skipped. This is also the test of the
        /// lease itself: feed the script one `add`, close the pipe, and the
        /// group must be gone.
        #[tokio::test]
        async fn reaper_script_kills_under_every_shell_present() {
            use std::io::Write;
            use std::os::unix::process::CommandExt;

            let mut checked = Vec::new();
            for shell in ["sh", "dash", "bash", "zsh", "ksh", "ash", "mksh"] {
                let Ok(mut reaper) = std::process::Command::new(shell)
                    .arg("-c")
                    .arg(super::REAPER_SCRIPT)
                    .stdin(Stdio::piped())
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .process_group(0)
                    .spawn()
                else {
                    continue;
                };
                let mut sleeper = stubborn_sleeper();
                let pid = sleeper.id().expect("pid");
                let mut stdin = reaper.stdin.take().expect("reaper stdin");
                stdin
                    .write_all(format!("add {pid}\n").as_bytes())
                    .expect("lease line");
                drop(stdin);

                let status = tokio::time::timeout(Duration::from_secs(5), sleeper.wait())
                    .await
                    .unwrap_or_else(|_| {
                        let _ = sleeper.start_kill();
                        panic!("{shell}: reaper did not kill the leased group");
                    })
                    .expect("wait");
                assert!(!status.success(), "{shell}: {status}");
                let _ = reaper.wait();
                checked.push(shell);
            }
            assert!(checked.contains(&"sh"), "no shell to test the reaper with");
        }
    }
}

#[cfg(windows)]
mod imp {
    use std::ffi::c_void;
    use std::io;

    use tokio::process::Child;
    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
    use windows_sys::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
        SetInformationJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
        JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    };

    pub struct Lease {
        /// The only handle to the job.
        job: HANDLE,
    }

    // SAFETY: a job handle is a kernel object reference; the Win32 job
    // calls made through it are safe from any thread.
    unsafe impl Send for Lease {}
    unsafe impl Sync for Lease {}

    impl Lease {
        /// Create the job. Like the Unix reaper, failing to set this up is
        /// fatal rather than silently running uncovered.
        pub fn new() -> Self {
            let job = create_job().expect("process lease: failed to create the job object");
            Self { job }
        }

        /// Put `child` — and, transitively, everything it starts — into the
        /// job. Call right after `spawn` succeeds.
        pub fn attach(&self, child: &Child) {
            let Some(process) = child.raw_handle() else {
                return;
            };
            // SAFETY: both handles are live kernel objects this process owns.
            if unsafe { AssignProcessToJobObject(self.job, process as HANDLE) } == 0 {
                tracing::warn!(
                    "[lease] AssignProcessToJobObject failed for pid {:?} ({}); it will outlive a daemon crash",
                    child.id(),
                    io::Error::last_os_error()
                );
            }
        }

        /// Job membership ends with the process; nothing to undo.
        pub fn release(&self, _pid: u32) {}
    }

    impl Drop for Lease {
        fn drop(&mut self) {
            // SAFETY: closing the last handle ends the job's members, which
            // is exactly the lease semantics.
            unsafe { CloseHandle(self.job) };
        }
    }

    fn create_job() -> io::Result<HANDLE> {
        // SAFETY: no security attributes (the handle is not inheritable,
        // so children cannot keep the job alive after the daemon is gone)
        // and no name.
        let job = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
        if job.is_null() {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: zeroed is a valid value for this plain C struct.
        let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { std::mem::zeroed() };
        info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        // SAFETY: `info` is the struct the information class expects, and
        // the length matches it.
        let ok = unsafe {
            SetInformationJobObject(
                job,
                JobObjectExtendedLimitInformation,
                &info as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION as *const c_void,
                std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            )
        };
        if ok == 0 {
            let error = io::Error::last_os_error();
            // SAFETY: closing the handle we just created.
            unsafe { CloseHandle(job) };
            return Err(error);
        }
        Ok(job)
    }

    #[cfg(test)]
    mod tests {
        use super::Lease;
        use std::process::Stdio;
        use std::time::Duration;
        use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, RefreshKind, System};
        use tokio::process::Command;

        fn processes() -> System {
            let mut system = System::new_with_specifics(
                RefreshKind::nothing().with_processes(ProcessRefreshKind::everything()),
            );
            system.refresh_processes(ProcessesToUpdate::All, true);
            system
        }

        #[tokio::test]
        async fn dropping_the_lease_ends_attached_children_and_their_descendants() {
            use tokio::io::AsyncWriteExt;

            let lease = Lease::new();
            // cmd blocks on `set /p` until it reads a line, so ping is only
            // started after the attach below — inside the job.
            let mut command = Command::new("cmd");
            command
                .args(["/C", "set /p _= & ping -t 127.0.0.1 >NUL"])
                .stdin(Stdio::piped())
                .stdout(Stdio::null())
                .stderr(Stdio::null());
            let mut child = command.spawn().expect("spawn cmd");
            lease.attach(&child);
            let cmd_pid = child.id().expect("pid");
            let mut stdin = child.stdin.take().expect("cmd stdin");
            stdin.write_all(b"\n").await.expect("release cmd");

            // Wait for cmd to start ping, so a grandchild exists to check.
            let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
            let grandchildren = loop {
                let system = processes();
                let found: Vec<u32> = system
                    .processes()
                    .values()
                    .filter(|process| process.parent() == Some(Pid::from_u32(cmd_pid)))
                    .filter(|process| {
                        process
                            .name()
                            .to_string_lossy()
                            .to_ascii_lowercase()
                            .contains("ping")
                    })
                    .map(|process| process.pid().as_u32())
                    .collect();
                if !found.is_empty() {
                    break found;
                }
                assert!(
                    tokio::time::Instant::now() < deadline,
                    "cmd never started ping"
                );
                tokio::time::sleep(Duration::from_millis(50)).await;
            };

            drop(lease);

            // Waiting successfully proves the attached root was reaped. Windows
            // does not guarantee a non-zero exit code when closing a kill-on-close
            // job, so the process tree disappearing is the contract to assert.
            let _status = tokio::time::timeout(Duration::from_secs(5), child.wait())
                .await
                .expect("closing the job did not end the attached child")
                .expect("wait");
            let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
            loop {
                let system = processes();
                let alive: Vec<u32> = grandchildren
                    .iter()
                    .copied()
                    .filter(|pid| system.process(Pid::from_u32(*pid)).is_some())
                    .collect();
                if alive.is_empty() {
                    break;
                }
                assert!(
                    tokio::time::Instant::now() < deadline,
                    "descendants outlived the job: {alive:?}"
                );
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
        }
    }
}

pub use imp::Lease;

impl Default for Lease {
    fn default() -> Self {
        Self::new()
    }
}
