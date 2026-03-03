//! Codex ACP subprocess spawner — launches `npx @zed-industries/codex-acp` and returns stdio streams.
//! The ACP client logic is handled by the shared `AcpBackend` in `mod.rs`.
//!
//! Requires Node.js 18+ with npx in PATH.
//! See: https://github.com/zed-industries/codex-acp

use std::path::Path;

/// Spawn `npx @zed-industries/codex-acp` and return (stdout_as_read, stdin_as_write) streams.
/// npx auto-downloads the package on first run; no global install needed.
pub fn spawn_codex_process(
    cwd: &Path,
) -> Result<(tokio::io::DuplexStream, tokio::io::DuplexStream), String> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    eprintln!("[codex-acp] spawning npx @zed-industries/codex-acp in {:?}", cwd);
    let mut child = tokio::process::Command::new("npx")
        .args(["@zed-industries/codex-acp"])
        .current_dir(cwd)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::inherit())
        .kill_on_drop(true)
        .spawn()
        .map_err(|_| "Failed to run npx @zed-industries/codex-acp. Is Node.js installed?\nSee: https://github.com/zed-industries/codex-acp".to_string())?;
    eprintln!("[codex-acp] process spawned pid={:?}", child.id());

    let child_stdout = child
        .stdout
        .take()
        .ok_or("No stdout from codex-acp process")?;
    let child_stdin = child
        .stdin
        .take()
        .ok_or("No stdin from codex-acp process")?;

    // Bridge child stdout → duplex read side
    let (client_read, mut bridge_write) = tokio::io::duplex(64 * 1024);
    tokio::task::spawn_local(async move {
        let mut stdout = child_stdout;
        let mut buf = [0u8; 8192];
        loop {
            match stdout.read(&mut buf).await {
                Ok(0) => break,
                Ok(n) => {
                    if bridge_write.write_all(&buf[..n]).await.is_err() {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
        drop(child);
    });

    // Bridge duplex write side → child stdin
    let (mut bridge_read, client_write) = tokio::io::duplex(64 * 1024);
    tokio::task::spawn_local(async move {
        let mut stdin = child_stdin;
        let mut buf = [0u8; 8192];
        loop {
            match bridge_read.read(&mut buf).await {
                Ok(0) => break,
                Ok(n) => {
                    if stdin.write_all(&buf[..n]).await.is_err() {
                        break;
                    }
                    let _ = stdin.flush().await;
                }
                Err(_) => break,
            }
        }
    });

    Ok((client_read, client_write))
}
