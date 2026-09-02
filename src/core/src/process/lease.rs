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
//! Two things are deliberately not covered. The moment between the fork
//! and the `add` write (about a millisecond): a daemon killed exactly then
//! leaves that one child behind — a kill at that instant is either a bug
//! to fix or the user's own doing, not something to engineer around. And
//! the reaper itself being killed by hand: from then on new children run
//! uncovered, with a warning in the log.
//!
//! **Windows (interim).** No kernel-held lease yet: a pid roster killed
//! from the exit handler with `taskkill /T`, plus the startup orphan sweep
//! for crashes — the behaviour VibeAround had before the lease. The real
//! fix is a Job Object with kill-on-close, to be built and verified on a
//! Windows machine.
//!
//! Orderly stops still go through the supervisor (`SIGTERM`, then
//! `SIGKILL`, per child); the lease only ever acts after the daemon is gone.

#[cfg(unix)]
mod imp {
    use std::io::{self, Write};
    use std::os::fd::{AsRawFd, OwnedFd};
    use std::process::Stdio;

    use tokio::process::{Child, Command};

    /// POSIX `sh`. Reads `add <pgid>` / `del <pgid>` lines until EOF, then
    /// signals whatever is still leased. `HUP`/`INT` are ignored so a
    /// closing terminal or Ctrl-C aimed at the daemon cannot take the
    /// reaper down before it has done its job.
    const REAPER_SCRIPT: &str = r#"
trap '' HUP INT
pg=''
while IFS= read -r line; do
  case "$line" in
    "add "*) pg="$pg ${line#add }" ;;
    "del "*) x="${line#del }"; new=''; for p in $pg; do [ "$p" = "$x" ] || new="$new $p"; done; pg="$new" ;;
  esac
done
for p in $pg; do kill -TERM -- "-$p" 2>/dev/null; done
[ -n "$pg" ] && sleep 1
for p in $pg; do kill -KILL -- "-$p" 2>/dev/null; done
"#;

    pub struct Lease {
        /// Blocking, close-on-exec write end of the reaper's stdin.
        pipe: std::fs::File,
        #[cfg_attr(not(test), allow(dead_code))]
        reaper_pid: u32,
    }

    impl Lease {
        /// Start the reaper. A daemon that cannot run `sh` cannot run any
        /// of its children either, so this is fatal.
        pub fn new() -> Self {
            spawn_reaper().expect("process lease: failed to start the reaper (is `sh` on PATH?)")
        }

        /// Lease whose lines go to `fd` instead of a reaper. Test-only:
        /// lets tests observe exactly what would have been leased.
        #[cfg(test)]
        pub(crate) fn with_fd(fd: OwnedFd) -> Self {
            Self {
                pipe: std::fs::File::from(fd),
                reaper_pid: 0,
            }
        }

        #[cfg(test)]
        pub(crate) fn reaper_pid(&self) -> Option<u32> {
            (self.reaper_pid != 0).then_some(self.reaper_pid)
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

    fn spawn_reaper() -> io::Result<Lease> {
        let mut command = Command::new("sh");
        command
            .arg("-c")
            .arg(REAPER_SCRIPT)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .process_group(0);
        let mut reaper = command.spawn()?;
        let fd = reaper
            .stdin
            .take()
            .expect("reaper stdin is piped")
            .into_owned_fd()?;
        set_blocking(&fd)?;
        // The reaper must outlive us: the handle is dropped without
        // waiting (tokio reaps it if it ever exits) and never killed.
        let reaper_pid = reaper.id().unwrap_or(0);
        Ok(Lease {
            pipe: std::fs::File::from(fd),
            reaper_pid,
        })
    }

    /// tokio put the pipe into non-blocking mode; the lease writes are
    /// plain blocking writes of a few bytes.
    fn set_blocking(fd: &OwnedFd) -> io::Result<()> {
        let raw = fd.as_raw_fd();
        // SAFETY: fcntl on an fd we own.
        let flags = unsafe { libc::fcntl(raw, libc::F_GETFL) };
        if flags < 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: as above.
        if unsafe { libc::fcntl(raw, libc::F_SETFL, flags & !libc::O_NONBLOCK) } < 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }

    #[cfg(test)]
    mod tests {
        use super::Lease;
        use std::io::Read;
        use std::os::fd::OwnedFd;
        use std::os::unix::net::UnixStream;
        use std::process::Stdio;
        use std::time::Duration;
        use tokio::process::Command;

        /// Ignores SIGTERM (the disposition survives exec) so only the
        /// reaper's SIGKILL fallback can end it.
        fn stubborn_sleeper(lease: &Lease) -> tokio::process::Child {
            let mut command = Command::new("sh");
            command
                .args(["-c", "trap '' TERM; exec sleep 30"])
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null());
            crate::process::kill::prepare_tree_root(&mut command);
            let child = command.spawn().expect("spawn sleeper");
            lease.attach(&child);
            child
        }

        #[tokio::test]
        async fn dropping_the_lease_kills_leased_groups() {
            let lease = Lease::new();
            let mut sleeper = stubborn_sleeper(&lease);
            tokio::time::sleep(Duration::from_millis(100)).await;

            drop(lease);

            let status = tokio::time::timeout(Duration::from_secs(5), sleeper.wait())
                .await
                .expect("reaper did not kill the leased group after the lease closed")
                .expect("wait");
            assert!(!status.success(), "{status}");
        }

        #[tokio::test]
        async fn released_groups_outlive_the_lease() {
            let lease = Lease::new();
            let mut sleeper = stubborn_sleeper(&lease);
            lease.release(sleeper.id().expect("pid"));
            tokio::time::sleep(Duration::from_millis(100)).await;

            drop(lease);

            let still_running = tokio::time::timeout(Duration::from_millis(1500), sleeper.wait())
                .await
                .is_err();
            let _ = sleeper.start_kill();
            let _ = sleeper.wait().await;
            assert!(still_running, "reaper killed a released group");
        }

        #[tokio::test]
        async fn attach_and_release_write_one_line_each() {
            let (ours, theirs) = UnixStream::pair().expect("socket pair");
            let lease = Lease::with_fd(OwnedFd::from(theirs));
            let mut command = Command::new("sh");
            command.args(["-c", "exit 3"]).stdin(Stdio::null());
            crate::process::kill::prepare_tree_root(&mut command);
            let mut child = command.spawn().expect("spawn");
            lease.attach(&child);
            let pid = child.id().expect("pid");
            let status = child.wait().await.expect("wait");
            assert_eq!(status.code(), Some(3));
            lease.release(pid);
            drop(lease);

            let lines = tokio::time::timeout(
                Duration::from_secs(5),
                tokio::task::spawn_blocking(move || {
                    let mut reader = ours;
                    let mut text = String::new();
                    reader.read_to_string(&mut text).expect("read lease lines");
                    text
                }),
            )
            .await
            .expect("lease lines before EOF")
            .expect("reader");
            assert_eq!(lines, format!("add {pid}\ndel {pid}\n"));
        }
    }
}

#[cfg(windows)]
mod imp {
    use std::collections::HashSet;

    use parking_lot::Mutex;
    use tokio::process::Child;

    /// Interim Windows lease: the pid roster VibeAround had before the
    /// lease, killed from the exit handler. A crash still relies on the
    /// startup orphan sweep. See the module docs for the Job Object plan.
    pub struct Lease {
        pids: Mutex<HashSet<u32>>,
    }

    impl Lease {
        pub fn new() -> Self {
            Self {
                pids: Mutex::new(HashSet::new()),
            }
        }

        pub fn attach(&self, child: &Child) {
            if let Some(pid) = child.id() {
                self.pids.lock().insert(pid);
            }
        }

        pub fn release(&self, pid: u32) {
            self.pids.lock().remove(&pid);
        }

        /// Synchronously `taskkill /T /F` every live child. Needs no
        /// runtime, so the Tauri `RunEvent::Exit` handler can call it.
        pub fn kill_all(&self) {
            let pids = std::mem::take(&mut *self.pids.lock());
            tracing::info!("[lease] kill_all: {} child(ren)", pids.len());
            for pid in pids {
                let _ = std::process::Command::new("taskkill")
                    .args(["/PID", &pid.to_string(), "/T", "/F"])
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null())
                    .status();
            }
        }
    }
}

#[cfg(not(any(unix, windows)))]
mod imp {
    use tokio::process::Child;

    pub struct Lease;

    impl Lease {
        pub fn new() -> Self {
            Self
        }
        pub fn attach(&self, _child: &Child) {}
        pub fn release(&self, _pid: u32) {}
    }
}

pub use imp::Lease;

impl Default for Lease {
    fn default() -> Self {
        Self::new()
    }
}
