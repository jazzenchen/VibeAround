//! Portable PTY: spawn a shell and bridge stdin/stdout for xterm ↔ CLI.
//! Child is wrapped in Mutex so we can poll try_wait() from a thread and send run state to the frontend
//! (running vs exited + exit_code). With direct spawning (e.g. claude/gemini/codex), the same mechanism
//! would report which tool is running and when it exits.

use portable_pty::{native_pty_system, CommandBuilder, PtySize};
use std::io::{Read, Write};
use std::path::Path;
use std::sync::{self, Arc, Mutex};
use tokio::sync::mpsc;

/// Shell command: login shell on Unix, cmd on Windows.
/// Injects TERM and COLORTERM so the PTY session is seen as a modern 256/truecolor terminal (matches xterm.js).
#[cfg(unix)]
fn shell_command() -> CommandBuilder {
    let mut c = CommandBuilder::new("bash");
    c.arg("-l");
    c.env("TERM", "xterm-256color");
    c.env("COLORTERM", "truecolor");
    c
}

#[cfg(windows)]
fn shell_command() -> CommandBuilder {
    let mut c = CommandBuilder::new("cmd.exe");
    c.env("TERM", "xterm-256color");
    c.env("COLORTERM", "truecolor");
    c
}

/// Exec string for each tool when wrapping with cd (bash -c "cd ... && exec ...").
/// Claude code runs with acceptEdits so file writes are auto-approved in headless/PTY.
fn tool_exec_argv(tool: PtyTool, tmux_session: Option<&str>) -> String {
    if let Some(name) = tmux_session {
        let escaped = name.replace('\'', "'\"'\"'");
        let detach = crate::config::ensure_loaded().tmux_detach_others;
        return if detach {
            format!("tmux attach -d -t '{}'", escaped)
        } else {
            format!("tmux attach -t '{}'", escaped)
        };
    }
    match tool {
        PtyTool::Generic => "bash -l".to_string(),
        PtyTool::Claude => "claude code --permission-mode acceptEdits".to_string(),
        PtyTool::Gemini => "gemini".to_string(),
        PtyTool::Codex => "codex".to_string(),
    }
}

/// Build command for direct spawn by tool. Generic = login shell; others = CLI (claude code, gemini, codex).
/// If cwd is Some, wraps in a shell that `cd`s there then execs the tool (so PTY runs in that directory).
/// If tmux_session is Some, spawns `tmux new-session -A -s <name>` instead of the tool directly.
fn command_for_tool(tool: PtyTool, cwd: Option<&Path>, tmux_session: Option<&str>) -> CommandBuilder {
    if let Some(dir) = cwd {
        #[cfg(unix)]
        {
            let path = dir.to_string_lossy();
            let escaped = path.replace('\'', "'\"'\"'");
            let exec = tool_exec_argv(tool, tmux_session);
            let line = format!("cd '{}' && exec {}", escaped, exec);
            let mut wrap = CommandBuilder::new("bash");
            wrap.arg("-c");
            wrap.arg(line);
            wrap.env("TERM", "xterm-256color");
            wrap.env("COLORTERM", "truecolor");
            // Unset TMUX to avoid "sessions should be nested with care" when attaching from inside tmux.
            wrap.env_remove("TMUX");
            return wrap;
        }
        #[cfg(not(unix))]
        let _ = dir;
    }

    // tmux mode without cwd
    if tmux_session.is_some() {
        let exec = tool_exec_argv(tool, tmux_session);
        let mut wrap = CommandBuilder::new("bash");
        wrap.arg("-c");
        wrap.arg(exec);
        wrap.env("TERM", "xterm-256color");
        wrap.env("COLORTERM", "truecolor");
        wrap.env_remove("TMUX");
        return wrap;
    }

    let mut c = match tool {
        PtyTool::Generic => return shell_command(),
        PtyTool::Claude => {
            let mut cmd = CommandBuilder::new("claude");
            cmd.arg("code");
            cmd
        }
        PtyTool::Gemini => CommandBuilder::new("gemini"),
        PtyTool::Codex => CommandBuilder::new("codex"),
    };
    c.env("TERM", "xterm-256color");
    c.env("COLORTERM", "truecolor");
    c
}

/// Run state of the PTY child (shell or direct-spawned tool). Sent to frontend for UI (tool theme + status).
#[derive(Debug, Clone, serde::Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum PtyRunState {
    /// Process is still running. With direct spawn, `tool` can be claude/gemini/codex; with shell wrapper it is Generic.
    Running { tool: PtyTool },
    /// Process has exited. Frontend can show status "stopped" or "error" and optionally display exit_code.
    Exited { tool: PtyTool, exit_code: u32 },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PtyTool {
    Generic,
    Claude,
    Gemini,
    Codex,
}

/// PTY bridge: writer for stdin; reader runs in a thread. Resize via `resize_tx`. Child kept so process stays alive; a separate thread polls try_wait().
pub struct PtyBridge {
    pub writer: Arc<std::sync::Mutex<Box<dyn Write + Send>>>,
    child: Arc<Mutex<Box<dyn portable_pty::Child + Send + Sync>>>,
}

/// Sender to request PTY resize (cols, rows). Send from WebSocket handler; a dedicated thread runs master.resize().
pub type ResizeSender = sync::mpsc::Sender<(u16, u16)>;

/// Spawn a process in a PTY: either shell (Generic) or direct CLI (Claude/Gemini/Codex). Returns bridge, PTY stdout receiver, resize sender, and state receiver (async).
/// If `cwd` is Some, the process runs in that directory (via a shell wrapper on Unix).
/// If `tmux_session` is Some, spawns `tmux new-session -A -s <name>` (attach-or-create) instead of the tool directly.
pub fn spawn_pty(
    tool: PtyTool,
    cwd: Option<std::path::PathBuf>,
    tmux_session: Option<String>,
) -> Result<(PtyBridge, mpsc::Receiver<Vec<u8>>, ResizeSender, mpsc::Receiver<PtyRunState>), Box<dyn std::error::Error + Send + Sync>> {
    let pty_system = native_pty_system();
    let pair = pty_system.openpty(PtySize {
        rows: 24,
        cols: 80,
        pixel_width: 0,
        pixel_height: 0,
    })?;

    let cmd = command_for_tool(tool, cwd.as_deref(), tmux_session.as_deref());
    let child = pair.slave.spawn_command(cmd)?;

    let mut reader = pair.master.try_clone_reader()?;
    let writer = pair.master.take_writer()?;
    let master = pair.master;

    let (tx, rx) = mpsc::channel::<Vec<u8>>(256);
    let (resize_tx, resize_rx) = sync::mpsc::channel::<(u16, u16)>();
    let (state_tx, state_rx) = mpsc::channel::<PtyRunState>(10);

    let child = Arc::new(Mutex::new(child));

    // Blocking thread: read PTY stdout and send to async side.
    std::thread::spawn(move || {
        let mut buf = [0u8; 4096];
        loop {
            match reader.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => {
                    if tx.blocking_send(buf[..n].to_vec()).is_err() {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
    });

    // Dedicated thread: resize PTY when the client sends (cols, rows) (e.g. from xterm-addon-fit).
    std::thread::spawn(move || {
        while let Ok((cols, rows)) = resize_rx.recv() {
            let size = PtySize {
                cols,
                rows,
                pixel_width: 0,
                pixel_height: 0,
            };
            let _ = master.resize(size);
        }
    });

    // Poll child.try_wait(); send Running once, then Exited when process ends. Frontend uses this for tool/status UI.
    let child_poll = Arc::clone(&child);
    std::thread::spawn(move || {
        let mut sent_running = false;
        loop {
            let exit_status = {
                let mut guard = match child_poll.lock() {
                    Ok(g) => g,
                    Err(_) => break,
                };
                match guard.try_wait() {
                    Ok(None) => None,
                    Ok(Some(s)) => Some(s.exit_code()),
                    Err(_) => break,
                }
            };
            if let Some(code) = exit_status {
                let _ = state_tx.blocking_send(PtyRunState::Exited {
                    tool,
                    exit_code: code,
                });
                break;
            }
            if !sent_running {
                sent_running = true;
                let _ = state_tx.blocking_send(PtyRunState::Running { tool });
            }
            std::thread::sleep(std::time::Duration::from_secs(1));
        }
    });

    let bridge = PtyBridge {
        writer: Arc::new(std::sync::Mutex::new(writer)),
        child,
    };
    Ok((bridge, rx, resize_tx, state_rx))
}

impl PtyBridge {
    /// Kill the PTY child process. Call when WebSocket disconnects to avoid leaving orphan processes.
    pub fn kill(&self) -> Result<(), std::io::Error> {
        let mut guard = self
            .child
            .lock()
            .map_err(|_| std::io::Error::new(std::io::ErrorKind::Other, "child mutex poisoned"))?;
        guard.kill()
    }
}

/// List active tmux sessions (name only). Returns empty vec if tmux is not installed or no sessions exist.
pub fn list_tmux_sessions() -> Vec<String> {
    let output = std::process::Command::new("tmux")
        .args(["list-sessions", "-F", "#{session_name}"])
        .output();
    match output {
        Ok(o) if o.status.success() => {
            String::from_utf8_lossy(&o.stdout)
                .lines()
                .filter(|l| !l.is_empty())
                .map(|l| l.to_string())
                .collect()
        }
        _ => vec![],
    }
}

/// Check whether tmux is available on this system.
pub fn tmux_available() -> bool {
    std::process::Command::new("tmux")
        .arg("-V")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}
