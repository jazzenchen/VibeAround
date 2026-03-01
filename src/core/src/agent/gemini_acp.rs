//! Gemini subprocess spawner — launches `gemini --experimental-acp` and returns stdio streams.
//! The ACP client logic is handled by the shared `AcpBackend` in `mod.rs`.

use std::path::Path;

/// Spawn `gemini --experimental-acp` and return (stdout_as_read, stdin_as_write) streams
/// wrapped as `DuplexStream`-compatible types.
///
/// Since Gemini speaks ACP natively over stdin/stdout, we return the child's
/// stdout (for reading) and stdin (for writing) directly as `DuplexStream` via
/// a bridging task.
pub fn spawn_gemini_process(
    cwd: &Path,
) -> Result<(tokio::io::DuplexStream, tokio::io::DuplexStream), String> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let mut child = tokio::process::Command::new("gemini")
        .arg("--experimental-acp")
        .current_dir(cwd)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .map_err(|e| format!("Failed to spawn gemini: {}", e))?;

    let child_stdout = child
        .stdout
        .take()
        .ok_or("No stdout from gemini process")?;
    let child_stdin = child
        .stdin
        .take()
        .ok_or("No stdin from gemini process")?;

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
        // Keep child alive until this task ends
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
