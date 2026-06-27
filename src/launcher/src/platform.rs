use std::borrow::Cow;
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::LazyLock;

use serde::{Deserialize, Serialize};

use crate::{ExecutionPlan, TerminalChoice};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LaunchHandle {
    pub script_path: PathBuf,
}

const DESKTOP_LAUNCH_TOML: &str = include_str!("../../resources/desktop-launch.toml");

#[derive(Debug, Deserialize)]
struct DesktopLaunchTemplates {
    macos: MacosTemplates,
    #[cfg_attr(not(target_os = "windows"), allow(dead_code))]
    windows: WindowsTemplates,
}

#[derive(Debug, Deserialize)]
struct MacosTemplates {
    app_probe: String,
}

#[derive(Debug, Deserialize)]
struct WindowsTemplates {
    #[cfg_attr(not(target_os = "windows"), allow(dead_code))]
    process_probe: String,
}

static TEMPLATES: LazyLock<DesktopLaunchTemplates> = LazyLock::new(|| {
    toml::from_str(DESKTOP_LAUNCH_TOML).expect("Failed to parse desktop-launch.toml")
});

#[cfg(target_os = "macos")]
pub fn spawn(plan: &ExecutionPlan) -> anyhow::Result<LaunchHandle> {
    macos::spawn(plan)
}

#[cfg(target_os = "windows")]
pub fn spawn(plan: &ExecutionPlan) -> anyhow::Result<LaunchHandle> {
    windows::spawn(plan)
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
pub fn spawn(plan: &ExecutionPlan) -> anyhow::Result<LaunchHandle> {
    linux::spawn(plan)
}

fn build_bash_script(plan: &ExecutionPlan) -> String {
    let mut out = String::new();
    out.push_str("#!/bin/bash\n");
    out.push_str("rm -- \"$0\"\n");
    out.push_str("set -e\n");
    out.push_str(&format!(
        "echo \"# VibeAround launch: {}\"\n",
        plan.window_label.replace('"', "'")
    ));

    let mut seen = HashSet::new();
    for (key, value) in &plan.env {
        if !seen.insert(key.as_str()) || !is_valid_env_key(key) {
            continue;
        }
        let escaped = shell_escape::unix::escape(Cow::Borrowed(value.as_str()));
        out.push_str(&format!("export {key}={escaped}\n"));
    }
    append_bash_color_env(&mut out);

    let workspace = plan.workspace.to_string_lossy();
    let cwd = shell_escape::unix::escape(Cow::Borrowed(workspace.as_ref()));
    out.push_str(&format!("cd {cwd}\n"));

    let command = command_with_unix_args(&plan.command, &plan.args);
    let command = macos_open_command_with_env(&command, plan);
    if let Some(app_name) = &plan.macos_app_probe {
        append_macos_app_launch(&mut out, &command, app_name);
    } else if !plan.cleanup_paths.is_empty() {
        out.push_str(&format!("{command}\n"));
        out.push_str("status=$?\n");
        append_bash_cleanup_paths(&mut out, &plan.cleanup_paths);
        out.push_str("exit \"$status\"\n");
    } else {
        out.push_str(&format!("exec {command}\n"));
    }
    out
}

fn append_macos_app_launch(out: &mut String, command: &str, app_name: &str) {
    let app_script = format!(
        "application \"{}\" is running",
        app_name.replace('\\', "\\\\").replace('"', "\\\"")
    );
    let app_script = shell_escape::unix::escape(Cow::Owned(app_script));
    out.push_str(&render_template(
        &TEMPLATES.macos.app_probe,
        &[("command", command), ("app_script", &app_script)],
    ));
}

fn command_with_unix_args(command: &str, args: &[String]) -> String {
    command_words_with_args(command, args)
        .iter()
        .map(|word| shell_escape::unix::escape(Cow::Borrowed(word.as_str())))
        .collect::<Vec<_>>()
        .join(" ")
}

fn command_words_with_args(command: &str, args: &[String]) -> Vec<String> {
    let mut words = split_command_words(command);
    words.extend(args.iter().cloned());
    words
}

fn split_command_words(command: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut current = String::new();
    let mut chars = command.chars().peekable();
    let mut quote: Option<char> = None;

    while let Some(ch) = chars.next() {
        match quote {
            Some(q) if ch == q => quote = None,
            Some('"') if ch == '\\' => {
                if matches!(chars.peek(), Some('"') | Some('\\')) {
                    let next = chars.next().expect("peeked next char");
                    current.push(next);
                } else {
                    current.push(ch);
                }
            }
            Some(_) => current.push(ch),
            None if ch == '\'' || ch == '"' => quote = Some(ch),
            None if ch.is_whitespace() => {
                if !current.is_empty() {
                    words.push(std::mem::take(&mut current));
                }
            }
            None => current.push(ch),
        }
    }

    if !current.is_empty() {
        words.push(current);
    }
    words
}

fn macos_open_command_with_env(command: &str, plan: &ExecutionPlan) -> String {
    if !cfg!(target_os = "macos") || plan.env.is_empty() {
        return command.to_string();
    }
    let Some(rest) = command.trim_start().strip_prefix("open ") else {
        return command.to_string();
    };

    let mut out = String::from("open");
    for (key, value) in &plan.env {
        if !is_valid_env_key(key) {
            continue;
        }
        let arg = format!("{key}={value}");
        out.push_str(" --env ");
        out.push_str(&shell_escape::unix::escape(Cow::Owned(arg)));
    }
    out.push(' ');
    out.push_str(rest);
    out
}

fn append_bash_color_env(out: &mut String) {
    out.push_str("unset NO_COLOR\n");
    out.push_str(
        "if [ -z \"${TERM:-}\" ] || [ \"$TERM\" = \"dumb\" ]; then export TERM=xterm-256color; fi\n",
    );
    out.push_str("export COLORTERM=${COLORTERM:-truecolor}\n");
    out.push_str("export CLICOLOR=${CLICOLOR:-1}\n");
}

fn append_bash_cleanup_paths(out: &mut String, paths: &[PathBuf]) {
    for path in paths {
        let path = path.to_string_lossy();
        let escaped = shell_escape::unix::escape(Cow::Borrowed(path.as_ref()));
        out.push_str(&format!("rm -f -- {escaped}\n"));
    }
}

fn is_valid_env_key(key: &str) -> bool {
    !key.is_empty()
        && key
            .chars()
            .next()
            .is_some_and(|ch| ch.is_ascii_alphabetic() || ch == '_')
        && key
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
}

#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
fn windows_process_probe_script(process_name: &str) -> String {
    render_template(
        &TEMPLATES.windows.process_probe,
        &[("process_name", process_name)],
    )
}

fn render_template(template: &str, replacements: &[(&str, &str)]) -> String {
    let mut out = template.to_string();
    for (key, value) in replacements {
        out = out.replace(&format!("{{{key}}}"), value);
    }
    out
}

#[cfg(target_os = "macos")]
mod macos {
    use std::os::unix::fs::PermissionsExt;

    use anyhow::{bail, Context};

    use super::*;

    pub fn spawn(plan: &ExecutionPlan) -> anyhow::Result<LaunchHandle> {
        let script_path = std::env::temp_dir().join(format!(
            "vibearound-launch-{}.command",
            uuid::Uuid::new_v4()
        ));
        std::fs::write(&script_path, build_bash_script(plan))
            .with_context(|| format!("write launch script {}", script_path.display()))?;
        std::fs::set_permissions(&script_path, std::fs::Permissions::from_mode(0o700))
            .with_context(|| format!("chmod launch script {}", script_path.display()))?;

        let app_name = match plan.terminal {
            TerminalChoice::Terminal => "Terminal",
            TerminalChoice::Iterm2 => "iTerm",
            other => bail!("terminal '{}' is not supported on macOS", other.id()),
        };
        let status = std::process::Command::new("open")
            .arg("-a")
            .arg(app_name)
            .arg(&script_path)
            .status()
            .with_context(|| format!("invoke `open -a {app_name}`"))?;
        if !status.success() {
            let _ = std::fs::remove_file(&script_path);
            bail!(
                "`open -a {}` failed (exit {:?}). Make sure {0}.app is installed and try again.",
                app_name,
                status.code()
            );
        }
        Ok(LaunchHandle { script_path })
    }
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
mod linux {
    use std::ffi::OsString;
    use std::os::unix::fs::PermissionsExt;
    use std::path::{Path, PathBuf};
    use std::process::{Command, Stdio};

    use anyhow::{bail, Context};

    use super::*;

    pub fn spawn(plan: &ExecutionPlan) -> anyhow::Result<LaunchHandle> {
        let script_path = write_launch_script(plan)?;
        if let Err(error) = spawn_terminal(plan.terminal, &script_path) {
            let _ = std::fs::remove_file(&script_path);
            return Err(error);
        }
        Ok(LaunchHandle { script_path })
    }

    fn write_launch_script(plan: &ExecutionPlan) -> anyhow::Result<PathBuf> {
        let script_path =
            std::env::temp_dir().join(format!("vibearound-launch-{}.sh", uuid::Uuid::new_v4()));
        std::fs::write(&script_path, build_bash_script(plan))
            .with_context(|| format!("write launch script {}", script_path.display()))?;
        std::fs::set_permissions(&script_path, std::fs::Permissions::from_mode(0o700))
            .with_context(|| format!("chmod launch script {}", script_path.display()))?;
        Ok(script_path)
    }

    fn spawn_terminal(choice: TerminalChoice, script_path: &Path) -> anyhow::Result<()> {
        let candidates = terminal_invocations(choice, script_path)?;
        let mut missing = Vec::new();
        for candidate in candidates {
            let mut command = Command::new(candidate.program);
            command
                .args(&candidate.args)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null());
            match command.spawn() {
                Ok(_) => return Ok(()),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    missing.push(candidate.program);
                }
                Err(error) => {
                    return Err(error)
                        .with_context(|| format!("launch Linux terminal '{}'", candidate.program));
                }
            }
        }
        bail!(
            "No supported Linux terminal command found for '{}'. Tried: {}.",
            choice.id(),
            missing.join(", ")
        )
    }

    #[derive(Debug, Clone, PartialEq, Eq)]
    struct TerminalInvocation {
        program: &'static str,
        args: Vec<OsString>,
    }

    fn terminal_invocations(
        choice: TerminalChoice,
        script_path: &Path,
    ) -> anyhow::Result<Vec<TerminalInvocation>> {
        let script = script_path.as_os_str().to_owned();
        let invocations = match choice {
            TerminalChoice::SystemTerminal => vec![
                invocation("xdg-terminal-exec", [script.clone()]),
                invocation(
                    "x-terminal-emulator",
                    [OsString::from("-e"), script.clone()],
                ),
                invocation("gnome-terminal", [OsString::from("--"), script.clone()]),
                invocation("konsole", [OsString::from("-e"), script.clone()]),
                invocation(
                    "xfce4-terminal",
                    [OsString::from("--execute"), script.clone()],
                ),
                invocation("kitty", [script.clone()]),
                invocation("alacritty", [OsString::from("-e"), script.clone()]),
                invocation(
                    "wezterm",
                    [
                        OsString::from("start"),
                        OsString::from("--"),
                        script.clone(),
                    ],
                ),
                invocation("xterm", [OsString::from("-e"), script.clone()]),
            ],
            TerminalChoice::GnomeTerminal => {
                vec![invocation("gnome-terminal", [OsString::from("--"), script])]
            }
            TerminalChoice::Konsole => vec![invocation("konsole", [OsString::from("-e"), script])],
            TerminalChoice::XfceTerminal => vec![invocation(
                "xfce4-terminal",
                [OsString::from("--execute"), script],
            )],
            TerminalChoice::Xterm => vec![invocation("xterm", [OsString::from("-e"), script])],
            TerminalChoice::Kitty => vec![invocation("kitty", [script])],
            TerminalChoice::Alacritty => {
                vec![invocation("alacritty", [OsString::from("-e"), script])]
            }
            TerminalChoice::WezTerm => vec![invocation(
                "wezterm",
                [OsString::from("start"), OsString::from("--"), script],
            )],
            other => bail!("terminal '{}' is not supported on Linux", other.id()),
        };
        Ok(invocations)
    }

    fn invocation<const N: usize>(
        program: &'static str,
        args: [OsString; N],
    ) -> TerminalInvocation {
        TerminalInvocation {
            program,
            args: args.into(),
        }
    }
}

#[cfg(target_os = "windows")]
mod windows {
    use anyhow::{bail, Context};

    use super::*;

    pub fn spawn(plan: &ExecutionPlan) -> anyhow::Result<LaunchHandle> {
        if plan.terminal != TerminalChoice::PowerShell {
            bail!(
                "terminal '{}' is not supported on Windows",
                plan.terminal.id()
            );
        }
        let script_path =
            std::env::temp_dir().join(format!("vibearound-launch-{}.ps1", uuid::Uuid::new_v4()));
        std::fs::write(&script_path, build_powershell_script(plan))
            .with_context(|| format!("write launch script {}", script_path.display()))?;
        let mut args = vec!["-ExecutionPolicy", "Bypass"];
        if plan.windows_process_probe.is_none() {
            args.push("-NoExit");
        }
        args.push("-File");
        let script_arg = script_path.to_string_lossy();
        args.push(&script_arg);
        let status = std::process::Command::new("powershell.exe")
            .args(args)
            .status()
            .context("open PowerShell")?;
        if !status.success() {
            let _ = std::fs::remove_file(&script_path);
            bail!("PowerShell launch failed with exit {:?}", status.code());
        }
        Ok(LaunchHandle { script_path })
    }

    fn build_powershell_script(plan: &ExecutionPlan) -> String {
        let mut out = String::new();
        out.push_str(&format!(
            "$Host.UI.RawUI.WindowTitle = {}\n",
            powershell_single_quoted(&format!("VibeAround - {}", plan.window_label))
        ));
        out.push_str(&format!(
            "Write-Host '# VibeAround launch: {}'\n",
            plan.window_label.replace('\'', "''")
        ));
        for (key, value) in &plan.env {
            if is_valid_env_key(key) {
                out.push_str(&format!(
                    "$env:{key} = {}\n",
                    powershell_single_quoted(value)
                ));
            }
        }
        out.push_str("Remove-Item Env:NO_COLOR -ErrorAction SilentlyContinue\n");
        out.push_str(
            "if (-not $env:TERM -or $env:TERM -eq 'dumb') { $env:TERM = 'xterm-256color' }\n",
        );
        out.push_str("if (-not $env:COLORTERM) { $env:COLORTERM = 'truecolor' }\n");
        out.push_str("if (-not $env:CLICOLOR) { $env:CLICOLOR = '1' }\n");
        out.push_str(&format!(
            "Set-Location -LiteralPath {}\n",
            powershell_single_quoted(&plan.workspace.to_string_lossy())
        ));
        out.push_str(&powershell_command_block(&plan.command, &plan.args));
        out.push('\n');
        if let Some(process_name) = &plan.windows_process_probe {
            out.push_str(&windows_process_probe_script(&powershell_single_quoted(
                process_name,
            )));
        }
        for path in &plan.cleanup_paths {
            out.push_str(&format!(
                "Remove-Item -LiteralPath {} -Force -ErrorAction SilentlyContinue\n",
                powershell_single_quoted(&path.to_string_lossy())
            ));
        }
        out.push_str("$scriptPath = $MyInvocation.MyCommand.Path\n");
        out.push_str("if ($scriptPath) { Remove-Item -LiteralPath $scriptPath -Force -ErrorAction SilentlyContinue }\n");
        out
    }

    fn powershell_command_block(command: &str, args: &[String]) -> String {
        let argv = command_words_with_args(command, args);
        let Some((program, program_args)) = argv.split_first() else {
            return String::new();
        };
        let mut out = String::new();
        out.push_str(&format!(
            "$vaCommand = {}\n",
            powershell_single_quoted(program)
        ));
        out.push_str("$vaArgs = @(\n");
        for arg in program_args {
            out.push_str("  ");
            out.push_str(&powershell_single_quoted(arg));
            out.push('\n');
        }
        out.push_str(")\n& $vaCommand @vaArgs");
        out
    }

    fn powershell_single_quoted(value: &str) -> String {
        format!("'{}'", value.replace('\'', "''"))
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::path::PathBuf;

    use super::*;

    fn plan(env: BTreeMap<String, String>, command: &str, args: Vec<String>) -> ExecutionPlan {
        ExecutionPlan {
            agent: "codex".to_string(),
            profile_id: None,
            launch_target: None,
            terminal: TerminalChoice::Terminal,
            command: command.to_string(),
            args,
            workspace: PathBuf::from("/tmp/work dir"),
            env,
            cleanup_paths: Vec::new(),
            window_label: "Test".to_string(),
            macos_app_probe: None,
            windows_process_probe: None,
        }
    }

    #[test]
    fn build_bash_script_escapes_env_and_args() {
        let mut env = BTreeMap::new();
        env.insert(
            "ANTHROPIC_API_KEY".to_string(),
            "hi$(touch /tmp/pwned)".to_string(),
        );
        let script = build_bash_script(&plan(
            env,
            "codex",
            vec!["resume".to_string(), "session id".to_string()],
        ));

        assert!(script.contains("'hi$(touch /tmp/pwned)'"));
        assert!(script.contains("exec codex resume 'session id'"));
        assert!(!script.contains("$(touch /tmp/pwned)\n"));
    }

    #[test]
    fn build_bash_script_self_deletes_before_running() {
        let script = build_bash_script(&plan(BTreeMap::new(), "codex", Vec::new()));
        let lines: Vec<&str> = script.lines().collect();
        assert_eq!(lines[0], "#!/bin/bash");
        assert_eq!(lines[1], "rm -- \"$0\"");
    }

    #[test]
    fn build_bash_script_waits_for_macos_app_probe() {
        let mut plan = plan(BTreeMap::new(), "open -a Codex", Vec::new());
        plan.macos_app_probe = Some("Codex".to_string());

        let script = build_bash_script(&plan);

        assert!(script.contains("open -a Codex\nstatus=$?"));
        assert!(script.contains("osascript -e 'application \"Codex\" is running'"));
        assert!(script.contains("exit \"$status\""));
    }

    #[test]
    fn windows_process_probe_template_inserts_process_name() {
        let script = windows_process_probe_script("'Codex'");

        assert!(script.contains("Get-Process -Name 'Codex'"));
        assert!(script.contains("Start-Sleep -Milliseconds 500"));
    }
}
