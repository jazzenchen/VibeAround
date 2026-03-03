//! Portable PTY: spawn a shell and bridge stdin/stdout for xterm ↔ CLI.
//! Child is wrapped in Mutex so we can poll try_wait() from a thread and send run state to the frontend
//! (running vs exited + exit_code). With direct spawning (e.g. claude/gemini/codex), the same mechanism
//! would report which tool is running and when it exits.

use portable_pty::{native_pty_system, CommandBuilder, PtySize};
use std::io::{Read, Write};
use std::path::Path;
use std::sync::{self, Arc, Mutex};
use tokio::sync::mpsc;

/// Shell command: login shell on Unix, cmd on Windows. Caller must set PTY env (set_pty_env).
#[cfg(unix)]
fn shell_command() -> CommandBuilder {
    let mut c = CommandBuilder::new("bash");
    c.arg("-l");
    c
}

#[cfg(windows)]
fn shell_command() -> CommandBuilder {
    let mut c = CommandBuilder::new("cmd.exe");
    c
}

/// Exec string for each tool when wrapping with cd (bash -c "cd ... && exec ...").
/// Claude code runs with acceptEdits so file writes are auto-approved in headless/PTY.
/// For tmux: if session exists, attach (with -d when tmux_detach_others); otherwise create new session.
fn tool_exec_argv(tool: PtyTool, tmux_session: Option<&str>) -> String {
    if let Some(name) = tmux_session {
        let escaped = name.replace('\'', "'\"'\"'");
        let detach = crate::config::ensure_loaded().tmux_detach_others;
        return if detach {
            format!(
                "tmux has-session -t '{}' 2>/dev/null && exec tmux attach -d -t '{}' || exec tmux new-session -s '{}'",
                escaped, escaped, escaped
            )
        } else {
            format!(
                "tmux has-session -t '{}' 2>/dev/null && exec tmux attach -t '{}' || exec tmux new-session -s '{}'",
                escaped, escaped, escaped
            )
        };
    }
    match tool {
        PtyTool::Generic => "bash -l".to_string(),
        PtyTool::Claude => "claude code --permission-mode acceptEdits".to_string(),
        PtyTool::Gemini => "gemini".to_string(),
        PtyTool::Codex => "codex".to_string(),
        PtyTool::OpenCode => "opencode".to_string(),
    }
}

/// Set standard PTY env: TERM, COLORTERM, COLORFGBG, and COLOR_THEME for light/dark hint.
/// These env vars are a secondary signal; the primary mechanism is OscColorResponder
/// which intercepts OSC 10/11 queries in the PTY reader thread.
fn set_pty_env(c: &mut CommandBuilder, theme: Option<&str>) {
    c.env("TERM", "xterm-256color");
    c.env("COLORTERM", "truecolor");
    if let Some(t) = theme {
        match t {
            "light" | "dark" => {
                c.env("COLOR_THEME", t);
                // COLORFGBG: de-facto standard "fg;bg" ANSI color index (0=black, 15=white).
                c.env("COLORFGBG", if t == "light" { "0;15" } else { "15;0" });
            }
            _ => {}
        }
    }
}

/// Bash wrapper for `bash -c "<script>"` with PTY env and TMUX unset.
fn bash_wrapper(script: &str, theme: Option<&str>) -> CommandBuilder {
    let mut wrap = CommandBuilder::new("bash");
    wrap.arg("-c");
    wrap.arg(script);
    set_pty_env(&mut wrap, theme);
    wrap.env_remove("TMUX");
    wrap
}

/// Build command for direct spawn by tool. Generic = login shell; others = CLI.
/// If cwd is Some, wraps in a shell that `cd`s there then execs the tool.
/// If tmux_session is Some, spawns tmux attach-or-create instead of the tool directly.
fn command_for_tool(
    tool: PtyTool,
    cwd: Option<&Path>,
    tmux_session: Option<&str>,
    theme: Option<&str>,
) -> CommandBuilder {
    if let Some(dir) = cwd {
        #[cfg(unix)]
        {
            let path = dir.to_string_lossy();
            let escaped = path.replace('\'', "'\"'\"'");
            let exec = tool_exec_argv(tool, tmux_session);
            let line = format!("cd '{}' && exec {}", escaped, exec);
            return bash_wrapper(&line, theme);
        }
        #[cfg(not(unix))]
        let _ = dir;
    }

    if tmux_session.is_some() {
        let exec = tool_exec_argv(tool, tmux_session);
        return bash_wrapper(&exec, theme);
    }

    let mut c = match tool {
        PtyTool::Generic => {
            let mut cmd = shell_command();
            set_pty_env(&mut cmd, theme);
            return cmd;
        }
        PtyTool::Claude => {
            let mut cmd = CommandBuilder::new("claude");
            cmd.arg("code");
            cmd
        }
        PtyTool::Gemini => CommandBuilder::new("gemini"),
        PtyTool::Codex => CommandBuilder::new("codex"),
        PtyTool::OpenCode => CommandBuilder::new("opencode"),
    };
    set_pty_env(&mut c, theme);
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
    Codex,
    Gemini,
    OpenCode,
}

/// PTY bridge: writer for stdin; reader runs in a thread. Resize via `resize_tx`. Child kept so process stays alive; a separate thread polls try_wait().
pub struct PtyBridge {
    pub writer: Arc<std::sync::Mutex<Box<dyn Write + Send>>>,
    child: Arc<Mutex<Box<dyn portable_pty::Child + Send + Sync>>>,
}

/// Sender to request PTY resize (cols, rows). Send from WebSocket handler; a dedicated thread runs master.resize().
pub type ResizeSender = sync::mpsc::Sender<(u16, u16)>;

/// OSC 10/11 color query interceptor for TUI programs (opencode, gemini, etc.).
///
/// Many TUI apps send OSC 10 (foreground) / OSC 11 (background) queries to detect
/// terminal light/dark mode. In our architecture the PTY output goes through a
/// WebSocket to xterm.js in the browser, so the round-trip can be too slow (especially
/// via ngrok) and the query times out before the reply arrives.
///
/// This struct sits in the PTY reader thread and pattern-matches OSC queries in the
/// raw byte stream, writing the response directly back into the PTY master — zero
/// network latency, the child process gets an instant reply.
struct OscColorResponder {
    /// Pre-built OSC 10 response bytes, e.g. "\x1b]10;rgb:1e1e/2929/3b3b\x1b\\"
    osc10: Vec<u8>,
    /// Pre-built OSC 11 response bytes.
    osc11: Vec<u8>,
    /// Shared PTY writer to send responses back to the child process.
    writer: Arc<std::sync::Mutex<Box<dyn Write + Send>>>,
}

impl OscColorResponder {
    /// Build a responder from theme name. Returns None if theme is not "light"/"dark".
    fn new(theme: &str, writer: Arc<std::sync::Mutex<Box<dyn Write + Send>>>) -> Option<Self> {
        let (fg, bg) = match theme {
            // fg/bg as hex pairs matching the xterm.js theme in TerminalView.tsx.
            "light" => ("1e293b", "ffffff"),
            "dark"  => ("c8c8d8", "0d0d0d"),
            _ => return None,
        };
        // X11 color format: rgb:RR00/GG00/BB00 (16-bit per channel, we duplicate the 8-bit value).
        let osc10 = format!(
            "\x1b]10;rgb:{r}{r}/{g}{g}/{b}{b}\x1b\\",
            r = &fg[0..2], g = &fg[2..4], b = &fg[4..6],
        ).into_bytes();
        let osc11 = format!(
            "\x1b]11;rgb:{r}{r}/{g}{g}/{b}{b}\x1b\\",
            r = &bg[0..2], g = &bg[2..4], b = &bg[4..6],
        ).into_bytes();
        Some(Self { osc10, osc11, writer })
    }

    /// Scan a chunk of PTY output for OSC 10/11 queries and reply instantly.
    /// Both ST (\x1b\\) and BEL (\x07) terminators are matched.
    fn intercept(&self, chunk: &[u8]) {
        const OSC10_ST:  &[u8] = b"\x1b]10;?\x1b\\";
        const OSC10_BEL: &[u8] = b"\x1b]10;?\x07";
        const OSC11_ST:  &[u8] = b"\x1b]11;?\x1b\\";
        const OSC11_BEL: &[u8] = b"\x1b]11;?\x07";

        let has = |needle: &[u8]| chunk.windows(needle.len()).any(|w| w == needle);

        if has(OSC10_ST) || has(OSC10_BEL) {
            if let Ok(mut w) = self.writer.lock() {
                let _ = w.write_all(&self.osc10);
                let _ = w.flush();
            }
        }
        if has(OSC11_ST) || has(OSC11_BEL) {
            if let Ok(mut w) = self.writer.lock() {
                let _ = w.write_all(&self.osc11);
                let _ = w.flush();
            }
        }
    }
}

/// Spawn a process in a PTY. Returns bridge, PTY stdout receiver, resize sender, and state receiver.
/// `theme`: "dark"/"light" — sets COLORFGBG env hint for programs that don't query OSC 10/11.
pub fn spawn_pty(
    tool: PtyTool,
    cwd: Option<std::path::PathBuf>,
    tmux_session: Option<String>,
    theme: Option<String>,
) -> Result<(PtyBridge, mpsc::Receiver<Vec<u8>>, ResizeSender, mpsc::Receiver<PtyRunState>), Box<dyn std::error::Error + Send + Sync>> {
    let pty_system = native_pty_system();
    let pair = pty_system.openpty(PtySize {
        rows: 24,
        cols: 80,
        pixel_width: 0,
        pixel_height: 0,
    })?;

    let cmd = command_for_tool(
        tool,
        cwd.as_deref(),
        tmux_session.as_deref(),
        theme.as_deref(),
    );
    let child = pair.slave.spawn_command(cmd)?;

    let mut reader = pair.master.try_clone_reader()?;
    let writer = pair.master.take_writer()?;
    let master = pair.master;

    let (tx, rx) = mpsc::channel::<Vec<u8>>(256);
    let (resize_tx, resize_rx) = sync::mpsc::channel::<(u16, u16)>();
    let (state_tx, state_rx) = mpsc::channel::<PtyRunState>(10);

    let child = Arc::new(Mutex::new(child));
    let writer = Arc::new(std::sync::Mutex::new(writer));

    // Build OSC responder so the reader thread can reply to color queries instantly.
    let osc_responder = theme.as_deref()
        .and_then(|t| OscColorResponder::new(t, Arc::clone(&writer)));

    // Blocking thread: read PTY stdout, intercept OSC color queries, forward to frontend.
    std::thread::spawn(move || {
        let mut buf = [0u8; 4096];
        loop {
            match reader.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => {
                    let chunk = &buf[..n];
                    if let Some(ref resp) = osc_responder {
                        resp.intercept(chunk);
                    }
                    if tx.blocking_send(chunk.to_vec()).is_err() {
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
        writer,
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
