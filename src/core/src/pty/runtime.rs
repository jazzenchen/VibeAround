//! Portable PTY runtime: spawn a shell or tool and bridge stdin/stdout for terminal clients.
//! Child is wrapped in Mutex so we can poll try_wait() from a thread and send run state to the frontend.

use anyhow::Context;
use portable_pty::{native_pty_system, CommandBuilder, PtySize};
use std::io::{Read, Write};
use std::path::Path;
use std::sync::{self, Arc, Mutex};
use tokio::sync::mpsc;

use crate::proc_log;
use crate::process::registry::ProcessKind;

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

fn set_pty_env(c: &mut CommandBuilder, theme: Option<&str>, extra_env: &[(String, String)]) {
    for (key, val) in crate::process::env::child_env() {
        c.env(key, val);
    }
    // Codex.app launches this process with NO_COLOR=1/TERM=dumb in some
    // contexts. A web xterm PTY is color-capable, so clear the inherited
    // opt-out before applying our terminal defaults.
    c.env_remove("NO_COLOR");
    let pty_env = &crate::resources::PTY_ENV;
    for (key, val) in &pty_env.env {
        c.env(key, val);
    }
    if let Some(t) = theme {
        if let Some(theme_def) = pty_env.themes.get(t) {
            c.env("COLOR_THEME", t);
            c.env("COLORFGBG", &theme_def.colorfgbg);
        }
    }
    for (key, val) in extra_env {
        c.env(key, val);
    }
    c.env_remove("NO_COLOR");
}

fn bash_wrapper(
    script: &str,
    theme: Option<&str>,
    extra_env: &[(String, String)],
) -> CommandBuilder {
    let mut wrap = CommandBuilder::new("bash");
    wrap.arg("-c");
    wrap.arg(script);
    set_pty_env(&mut wrap, theme, extra_env);
    wrap.env_remove("TMUX");
    wrap
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\"'\"'"))
}

/// Exec string for each tool when wrapping with cd/tmux.
fn tool_exec_argv(tool: PtyTool, tmux_session: Option<&str>) -> String {
    if let Some(name) = tmux_session {
        let session = shell_quote(name);
        let detach = crate::config::ensure_loaded().tmux_detach_others;
        return if detach {
            format!(
                "tmux has-session -t {session} 2>/dev/null && exec tmux attach -d -t {session} || exec tmux new-session -s {session}"
            )
        } else {
            format!(
                "tmux has-session -t {session} 2>/dev/null && exec tmux attach -t {session} || exec tmux new-session -s {session}"
            )
        };
    }

    let Some(agent_id) = tool.agent_id() else {
        return "bash -l".to_string();
    };
    crate::resources::agent_by_id(agent_id)
        .map(|a| a.pty.command.clone())
        .unwrap_or_else(|| agent_id.to_string())
}

fn command_for_tool(
    tool: PtyTool,
    cwd: Option<&Path>,
    tmux_session: Option<&str>,
    theme: Option<&str>,
    command_override: Option<&str>,
    extra_env: &[(String, String)],
) -> CommandBuilder {
    if let Some(command) = command_override {
        let line = if let Some(dir) = cwd {
            format!(
                "cd {} && exec {}",
                shell_quote(&dir.to_string_lossy()),
                command
            )
        } else {
            format!("exec {}", command)
        };
        return bash_wrapper(&line, theme, extra_env);
    }

    if let Some(dir) = cwd {
        #[cfg(unix)]
        {
            let exec = tool_exec_argv(tool, tmux_session);
            let line = format!(
                "cd {} && exec {}",
                shell_quote(&dir.to_string_lossy()),
                exec
            );
            return bash_wrapper(&line, theme, extra_env);
        }
        #[cfg(not(unix))]
        let _ = dir;
    }

    if tmux_session.is_some() {
        let exec = tool_exec_argv(tool, tmux_session);
        return bash_wrapper(&exec, theme, extra_env);
    }

    let mut c = match tool {
        PtyTool::Generic => {
            let mut cmd = shell_command();
            set_pty_env(&mut cmd, theme, extra_env);
            return cmd;
        }
        PtyTool::Claude => {
            let mut cmd = CommandBuilder::new("claude");
            cmd.arg("code");
            cmd
        }
        PtyTool::Gemini => CommandBuilder::new("gemini"),
        PtyTool::Codex => CommandBuilder::new("codex"),
        PtyTool::Pi => CommandBuilder::new("pi"),
        PtyTool::OpenCode => CommandBuilder::new("opencode"),
        PtyTool::Cursor => CommandBuilder::new("cursor"),
        PtyTool::Kiro => CommandBuilder::new("kiro-cli"),
        PtyTool::QwenCode => CommandBuilder::new("qwen"),
    };
    set_pty_env(&mut c, theme, extra_env);
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
    Pi,
    Gemini,
    OpenCode,
    Cursor,
    Kiro,
    #[serde(rename = "qwen-code")]
    QwenCode,
}

impl PtyTool {
    pub fn agent_id(self) -> Option<&'static str> {
        match self {
            PtyTool::Generic => None,
            PtyTool::Claude => Some("claude"),
            PtyTool::Gemini => Some("gemini"),
            PtyTool::Codex => Some("codex"),
            PtyTool::Pi => Some("pi"),
            PtyTool::OpenCode => Some("opencode"),
            PtyTool::Cursor => Some("cursor"),
            PtyTool::Kiro => Some("kiro"),
            PtyTool::QwenCode => Some("qwen-code"),
        }
    }

    pub fn from_agent_id(agent_id: &str) -> Option<Self> {
        match agent_id {
            "claude" => Some(PtyTool::Claude),
            "gemini" => Some(PtyTool::Gemini),
            "codex" => Some(PtyTool::Codex),
            "pi" => Some(PtyTool::Pi),
            "opencode" => Some(PtyTool::OpenCode),
            "cursor" => Some(PtyTool::Cursor),
            "kiro" => Some(PtyTool::Kiro),
            "qwen-code" => Some(PtyTool::QwenCode),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pty_tool_agent_ids_round_trip() {
        for tool in [
            PtyTool::Claude,
            PtyTool::Gemini,
            PtyTool::Codex,
            PtyTool::Pi,
            PtyTool::OpenCode,
            PtyTool::Cursor,
            PtyTool::Kiro,
            PtyTool::QwenCode,
        ] {
            let agent_id = tool.agent_id().expect("tool should map to an agent");
            assert_eq!(PtyTool::from_agent_id(agent_id), Some(tool));
        }
        assert_eq!(PtyTool::Generic.agent_id(), None);
        assert_eq!(PtyTool::from_agent_id("missing"), None);
    }

    #[test]
    fn shell_quote_handles_single_quotes() {
        assert_eq!(shell_quote("plain"), "'plain'");
        assert_eq!(shell_quote("team's session"), "'team'\"'\"'s session'");
    }
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
        let pty_env = &crate::resources::PTY_ENV;
        let theme_def = pty_env.themes.get(theme)?;
        let (fg, bg) = (theme_def.fg.as_str(), theme_def.bg.as_str());
        let osc10 = format!(
            "\x1b]10;rgb:{r}{r}/{g}{g}/{b}{b}\x1b\\",
            r = &fg[0..2],
            g = &fg[2..4],
            b = &fg[4..6],
        )
        .into_bytes();
        let osc11 = format!(
            "\x1b]11;rgb:{r}{r}/{g}{g}/{b}{b}\x1b\\",
            r = &bg[0..2],
            g = &bg[2..4],
            b = &bg[4..6],
        )
        .into_bytes();
        Some(Self {
            osc10,
            osc11,
            writer,
        })
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
) -> anyhow::Result<(
    PtyBridge,
    mpsc::Receiver<Vec<u8>>,
    ResizeSender,
    mpsc::Receiver<PtyRunState>,
)> {
    spawn_pty_with_command(
        tool,
        cwd,
        tmux_session,
        theme,
        initial_size,
        None,
        Vec::new(),
    )
}

pub(super) fn spawn_pty_with_command(
    tool: PtyTool,
    cwd: Option<std::path::PathBuf>,
    tmux_session: Option<String>,
    theme: Option<String>,
    initial_size: Option<(u16, u16)>,
    command_override: Option<String>,
    extra_env: Vec<(String, String)>,
) -> anyhow::Result<(
    PtyBridge,
    mpsc::Receiver<Vec<u8>>,
    ResizeSender,
    mpsc::Receiver<PtyRunState>,
)> {
    let pty_system = native_pty_system();
    let (cols, rows) = initial_size.unwrap_or((80, 24));
    let pair = pty_system
        .openpty(PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        })
        .context("Failed to open PTY")?;

    let cmd = command_for_tool(
        tool,
        cwd.as_deref(),
        tmux_session.as_deref(),
        theme.as_deref(),
        command_override.as_deref(),
        &extra_env,
    );
    let child = pair
        .slave
        .spawn_command(cmd)
        .context("Failed to spawn PTY child process")?;
    let child_pid = child.process_id();
    let tool_label = format!("{:?}", tool).to_lowercase();
    proc_log!(
        info,
        kind = ProcessKind::Pty,
        label = tool_label,
        pid = child_pid,
        event = "spawned",
        cwd = ?cwd,
        tmux = ?tmux_session
    );

    let mut reader = pair
        .master
        .try_clone_reader()
        .context("Failed to clone PTY reader")?;
    let writer = pair
        .master
        .take_writer()
        .context("Failed to take PTY writer")?;
    let master = pair.master;

    let (tx, rx) = mpsc::channel::<Vec<u8>>(256);
    let (resize_tx, resize_rx) = sync::mpsc::channel::<(u16, u16)>();
    let (state_tx, state_rx) = mpsc::channel::<PtyRunState>(10);

    let child = Arc::new(Mutex::new(child));
    let writer = Arc::new(std::sync::Mutex::new(writer));

    let osc_responder = theme
        .as_deref()
        .and_then(|t| OscColorResponder::new(t, Arc::clone(&writer)));

    std::thread::Builder::new()
        .name(format!("pty-{:?}-reader", tool))
        .spawn(move || {
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
        })
        .expect("failed to spawn PTY reader thread");

    std::thread::Builder::new()
        .name(format!("pty-{:?}-resize", tool))
        .spawn(move || {
            while let Ok((cols, rows)) = resize_rx.recv() {
                let size = PtySize {
                    cols,
                    rows,
                    pixel_width: 0,
                    pixel_height: 0,
                };
                let _ = master.resize(size);
            }
        })
        .expect("failed to spawn PTY resize thread");

    let child_poll = Arc::clone(&child);
    let poll_label = format!("{:?}", tool).to_lowercase();
    std::thread::Builder::new()
        .name(format!("pty-{:?}-poll", tool))
        .spawn(move || {
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
                    proc_log!(
                        info,
                        kind = ProcessKind::Pty,
                        label = poll_label,
                        event = "exited",
                        exit_code = code
                    );
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
        })
        .expect("failed to spawn PTY poll thread");

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
