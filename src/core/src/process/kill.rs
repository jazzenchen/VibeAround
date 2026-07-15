use std::io;
use std::process::ExitStatus;

use tokio::process::{Child, ChildStderr, ChildStdout, Command};

pub struct TreeKillableChild {
    child: Child,
    root_pid: Option<u32>,
}

pub fn spawn_tree_killable(command: &mut Command) -> io::Result<TreeKillableChild> {
    prepare_tree_root(command);
    let child = command.spawn()?;
    let root_pid = child.id();
    Ok(TreeKillableChild { child, root_pid })
}

pub(crate) async fn wait_for_exit_within(
    child: &mut Child,
    duration: std::time::Duration,
) -> io::Result<Option<ExitStatus>> {
    match tokio::time::timeout(duration, child.wait()).await {
        Ok(status) => status.map(Some),
        Err(_) => Ok(None),
    }
}

impl TreeKillableChild {
    pub fn take_stdout(&mut self) -> Option<ChildStdout> {
        self.child.stdout.take()
    }

    pub fn take_stderr(&mut self) -> Option<ChildStderr> {
        self.child.stderr.take()
    }

    pub fn try_wait(&mut self) -> io::Result<Option<ExitStatus>> {
        self.child.try_wait()
    }

    pub async fn terminate_tree(&mut self) -> io::Result<()> {
        terminate_child_tree(&mut self.child, self.root_pid).await
    }
}

#[cfg(unix)]
pub(crate) fn prepare_tree_root(command: &mut Command) {
    command.process_group(0);
}

#[cfg(not(unix))]
pub(crate) fn prepare_tree_root(_command: &mut Command) {}

#[cfg(unix)]
pub(crate) async fn terminate_child_tree(
    child: &mut Child,
    root_pid: Option<u32>,
) -> io::Result<()> {
    let Some(root_pid) = root_pid else {
        child.start_kill()?;
        let _ = child.wait().await;
        return Ok(());
    };

    let term_result = signal_process_group(root_pid, libc::SIGTERM);
    let exited = wait_for_exit_within(child, std::time::Duration::from_millis(400)).await?;

    // Signal the whole group even if the root has already exited: helpers can
    // remain alive in the same process group after their parent is reaped.
    let kill_result = signal_process_group(root_pid, libc::SIGKILL);
    if exited.is_none() {
        let _ = child.wait().await;
    }
    kill_result.or(term_result)
}

#[cfg(windows)]
pub(crate) async fn terminate_child_tree(
    child: &mut Child,
    root_pid: Option<u32>,
) -> io::Result<()> {
    if let Some(root_pid) = root_pid {
        let pid = root_pid.to_string();
        let _ = crate::process::env::silent_command("taskkill")
            .args(["/PID", &pid, "/T", "/F"])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .await;
    }

    if child.try_wait()?.is_none() {
        child.start_kill()?;
    }
    let _ = child.wait().await;
    Ok(())
}

#[cfg(not(any(unix, windows)))]
pub(crate) async fn terminate_child_tree(
    child: &mut Child,
    _root_pid: Option<u32>,
) -> io::Result<()> {
    child.start_kill()?;
    let _ = child.wait().await;
    Ok(())
}

#[cfg(unix)]
fn signal_process_group(root_pid: u32, signal: libc::c_int) -> io::Result<()> {
    let pgid = -(root_pid as libc::pid_t);
    let result = unsafe { libc::kill(pgid, signal) };
    if result == 0 {
        return Ok(());
    }

    let error = io::Error::last_os_error();
    if error.raw_os_error() == Some(libc::ESRCH) {
        Ok(())
    } else {
        Err(error)
    }
}

#[cfg(test)]
mod tests {
    #[cfg(unix)]
    use super::*;

    #[cfg(unix)]
    #[tokio::test]
    async fn terminate_tree_stops_unix_grandchild() -> io::Result<()> {
        use std::process::Stdio;
        use tokio::io::{AsyncBufReadExt, BufReader};

        let mut command = Command::new("sh");
        command
            .arg("-c")
            .arg("sleep 30 & echo $!; wait")
            .stdout(Stdio::piped())
            .stderr(Stdio::null());

        let mut child = spawn_tree_killable(&mut command)?;
        let stdout = child.take_stdout().expect("tree child stdout");
        let mut lines = BufReader::new(stdout).lines();
        let grandchild_pid = tokio::time::timeout(std::time::Duration::from_secs(2), async {
            let line = lines
                .next_line()
                .await?
                .ok_or_else(|| io::Error::other("missing grandchild pid"))?;
            line.trim()
                .parse::<u32>()
                .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
        })
        .await
        .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "missing grandchild pid"))??;

        child.terminate_tree().await?;
        wait_until_process_exits(grandchild_pid).await;
        assert!(!process_exists(grandchild_pid));
        Ok(())
    }

    #[cfg(unix)]
    async fn wait_until_process_exits(pid: u32) {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
        while process_exists(pid) && std::time::Instant::now() < deadline {
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
    }

    #[cfg(unix)]
    fn process_exists(pid: u32) -> bool {
        let result = unsafe { libc::kill(pid as libc::pid_t, 0) };
        if result == 0 {
            return true;
        }
        io::Error::last_os_error().raw_os_error() != Some(libc::ESRCH)
    }
}
