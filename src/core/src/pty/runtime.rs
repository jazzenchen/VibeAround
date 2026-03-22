//! Portable PTY runtime: spawn a shell or tool and bridge stdin/stdout for terminal clients.
//! Child is wrapped in Mutex so we can poll try_wait() from a thread and send run state to the frontend.

use portable_pty::{native_pty_system, CommandBuilder, PtySize};
use std::io::{Read, Write};
use std::path::Path;
use std::sync::{self, Arc, Mutex};
use tokio::sync::mpsc;

/// Shell command: login shell on Unix, cmd on Windows. Caller must set PTY env.
#[cfg(unix)]
fn shell_command() -> CommandBuilder {
    let mut c = CommandBuilder::new("bash");
    c.arg("-l");
    c
}

#[cfg(windows)]
fn shell_command() -> CommandBuilder {
    let c = CommandBuilder::new("cmd.exe");
    c
}

/// Exec string for each tool when wrapping with cd.
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

fn set_pty_env(c: &mut CommandBuilder, theme: Option<&str>) {
    c.env("TERM", "xterm-256color");
    c.env("COLORTERM", "truecolor");
    if let Some(t) = theme {
        match t {
            "light" | "dark" => {
                c.env("COLOR_THEME", t);
                c.env("COLORFGBG", if t == "light" { "0;15" } else { "15;0" });
            }
            _ => {}
        }
    }
}

fn bash_wrapper(script: &str, theme: Option<&str>) -> CommandBuilder {
    let mut wrap = CommandBuilder::new("bash");
    wrap.arg("-c");
    wrap.arg(script);
    set_pty_env(&mut wrap, theme);
    wrap.env_remove("TMUX");
    wrap
}

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

#[derive(Debug, Clone, serde::Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum PtyRunState {
    Running { tool: PtyTool },
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

pub struct PtyBridge {
    pub writer: Arc<std::sync::Mutex<Box<dyn Write + Send>>>,
    child: Arc<Mutex<Box<dyn portable_pty::Child + Send + Sync>>>,
}

pub type ResizeSender = sync::mpsc::Sender<(u16, u16)>;

struct OscColorResponder {
    osc10: Vec<u8>,
    osc11: Vec<u8>,
    writer: Arc<std::sync::Mutex<Box<dyn Write + Send>>>,
}

impl OscColorResponder {
    fn new(theme: &str, writer: Arc<std::sync::Mutex<Box<dyn Write + Send>>>) -> Option<Self> {
        let (fg, bg) = match theme {
            "light" => ("1e293b", "ffffff"),
            "dark" => ("c8c8d8", "0d0d0d"),
            _ => return None,
        };
        let osc10 = format!(
            "\x1b]10;rgb:{r}{r}/{g}{g}/{b}{b}\x1b\\",
            r = &fg[0..2], g = &fg[2..4], b = &fg[4..6],
        )
        .into_bytes();
        let osc11 = format!(
            "\x1b]11;rgb:{r}{r}/{g}{g}/{b}{b}\x1b\\",
            r = &bg[0..2], g = &bg[2..4], b = &bg[4..6],
        )
        .into_bytes();
        Some(Self { osc10, osc11, writer })
    }

    fn intercept(&self, chunk: &[u8]) {
        const OSC10_ST: &[u8] = b"\x1b]10;?\x1b\\";
        const OSC10_BEL: &[u8] = b"\x1b]10;?\x07";
        const OSC11_ST: &[u8] = b"\x1b]11;?\x1b\\";
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

pub fn spawn_pty(
    tool: PtyTool,
    cwd: Option<std::path::PathBuf>,
    tmux_session: Option<String>,
    theme: Option<String>,
    initial_size: Option<(u16, u16)>,
) -> Result<(PtyBridge, mpsc::Receiver<Vec<u8>>, ResizeSender, mpsc::Receiver<PtyRunState>), Box<dyn std::error::Error + Send + Sync>> {
    let pty_system = native_pty_system();
    let (cols, rows) = initial_size.unwrap_or((80, 24));
    let pair = pty_system.openpty(PtySize {
        rows,
        cols,
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

    let osc_responder = theme
        .as_deref()
        .and_then(|t| OscColorResponder::new(t, Arc::clone(&writer)));

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
                let _ = state_tx.blocking_send(PtyRunState::Exited { tool, exit_code: code });
                break;
            }
            if !sent_running {
                sent_running = true;
                let _ = state_tx.blocking_send(PtyRunState::Running { tool });
            }
            std::thread::sleep(std::time::Duration::from_secs(1));
        }
    });

    let bridge = PtyBridge { writer, child };
    Ok((bridge, rx, resize_tx, state_rx))
}

impl PtyBridge {
    pub fn kill(&self) -> Result<(), std::io::Error> {
        let mut guard = self
            .child
            .lock()
            .map_err(|_| std::io::Error::other("child mutex poisoned"))?;
        guard.kill()
    }
}

pub fn list_tmux_sessions() -> Vec<String> {
    let output = std::process::Command::new("tmux")
        .args(["list-sessions", "-F", "#{session_name}"])
        .output();
    match output {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout)
            .lines()
            .filter(|l| !l.is_empty())
            .map(|l| l.to_string())
            .collect(),
        _ => vec![],
    }
}

pub fn tmux_available() -> bool {
    std::process::Command::new("tmux")
        .arg("-V")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}
