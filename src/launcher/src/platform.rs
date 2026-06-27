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
    use std::path::{Path, PathBuf};

    use anyhow::{bail, Context};

    use super::*;

    pub fn spawn(plan: &ExecutionPlan) -> anyhow::Result<LaunchHandle> {
        if plan.terminal != TerminalChoice::PowerShell {
            bail!(
                "terminal '{}' is not supported on Windows",
                plan.terminal.id()
            );
        }
        let script_path = write_launch_script(plan)?;
        let no_exit = if plan.windows_process_probe.is_some() {
            ""
        } else {
            "-NoExit "
        };
        let params = format!(
            "-ExecutionPolicy Bypass {no_exit}-File {}",
            quote_windows_process_arg(&script_path.to_string_lossy())
        );
        open::with(params, "powershell.exe").context("open PowerShell")?;
        Ok(LaunchHandle { script_path })
    }

    fn write_launch_script(plan: &ExecutionPlan) -> anyhow::Result<PathBuf> {
        let (command, args) = normalize_windows_launch_command(
            &plan.command,
            &plan.args,
            plan.windows_executable_path.as_deref(),
        );
        let script_path =
            std::env::temp_dir().join(format!("vibearound-launch-{}.ps1", uuid::Uuid::new_v4()));
        std::fs::write(&script_path, build_powershell_script(plan, &command, &args))
            .with_context(|| format!("write launch script {}", script_path.display()))?;
        common::auth::set_owner_only(&script_path).ok();
        Ok(script_path)
    }

    fn build_powershell_script(plan: &ExecutionPlan, command: &str, args: &[String]) -> String {
        let mut out = String::new();
        let (env, args) = normalize_windows_claude_profile_launch(plan, command, args);
        out.push_str(&format!(
            "$Host.UI.RawUI.WindowTitle = {}\n",
            powershell_single_quoted(&format!("VibeAround - {}", plan.window_label))
        ));
        out.push_str(&format!(
            "Write-Host '# VibeAround launch: {}'\n",
            plan.window_label.replace('\'', "''")
        ));
        for (key, value) in &env {
            if is_valid_env_key(key) {
                out.push_str(&format!(
                    "$env:{key} = {}\n",
                    powershell_single_quoted(value)
                ));
            }
        }
        append_powershell_launch_path(&mut out);
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
        out.push_str(&powershell_command_block(command, &args));
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

    fn append_powershell_launch_path(out: &mut String) {
        let env = common::process::env::child_env();
        let Some(path) = common::process::env::path_value(&env) else {
            return;
        };
        if path.trim().is_empty() {
            return;
        }
        out.push_str(&format!(
            "$env:Path = {}\n",
            powershell_single_quoted(&path)
        ));
    }

    fn normalize_windows_claude_profile_launch(
        plan: &ExecutionPlan,
        command: &str,
        args: &[String],
    ) -> (Vec<(String, String)>, Vec<String>) {
        let mut env: Vec<(String, String)> = plan
            .env
            .iter()
            .map(|(key, value)| (key.clone(), value.clone()))
            .collect();
        if !is_claude_launch_command(command) {
            return (env, args.to_vec());
        }

        let profile_model = env_value(&env, "ANTHROPIC_MODEL").map(str::to_string);
        let args = match profile_model.as_deref() {
            Some(model) => replace_or_append_model_arg(command, args, model),
            None => args.to_vec(),
        };

        if profile_owns_anthropic_env(&env) {
            env.retain(|(key, _)| !is_claude_model_override_env(key));
        }

        (env, args)
    }

    fn is_claude_launch_command(command: &str) -> bool {
        command_words_with_args(command, &[])
            .first()
            .is_some_and(|program| command_stem_eq(program, "claude"))
    }

    fn replace_or_append_model_arg(command: &str, args: &[String], model: &str) -> Vec<String> {
        let command_words = command_words_with_args(command, &[]);
        let mut args = args.to_vec();
        replace_or_append_model_arg_words(&mut args, model);

        if has_model_arg(&command_words) && !has_model_arg(&args) {
            args.push("--model".to_string());
            args.push(model.to_string());
        }

        args
    }

    fn has_model_arg(args: &[String]) -> bool {
        args.iter()
            .any(|arg| arg == "--model" || arg.starts_with("--model="))
    }

    fn replace_or_append_model_arg_words(args: &mut Vec<String>, model: &str) {
        let mut out = Vec::with_capacity(args.len() + 2);
        let mut replaced = false;
        let mut index = 0;

        while index < args.len() {
            let arg = &args[index];
            if arg == "--model" {
                out.push(arg.clone());
                out.push(model.to_string());
                replaced = true;
                index += if index + 1 < args.len() { 2 } else { 1 };
                continue;
            }
            if arg.starts_with("--model=") {
                out.push(format!("--model={model}"));
                replaced = true;
                index += 1;
                continue;
            }
            out.push(arg.clone());
            index += 1;
        }

        if !replaced {
            out.push("--model".to_string());
            out.push(model.to_string());
        }
        *args = out;
    }

    fn profile_owns_anthropic_env(env: &[(String, String)]) -> bool {
        [
            "ANTHROPIC_BASE_URL",
            "ANTHROPIC_MODEL",
            "ANTHROPIC_API_KEY",
            "ANTHROPIC_AUTH_TOKEN",
        ]
        .iter()
        .any(|key| env_value(env, key).is_some())
    }

    fn env_value<'a>(env: &'a [(String, String)], key: &str) -> Option<&'a str> {
        env.iter()
            .find(|(existing, value)| existing == key && !value.is_empty())
            .map(|(_, value)| value.as_str())
    }

    fn is_claude_model_override_env(key: &str) -> bool {
        matches!(
            key,
            "ANTHROPIC_DEFAULT_HAIKU_MODEL"
                | "ANTHROPIC_DEFAULT_OPUS_MODEL"
                | "ANTHROPIC_DEFAULT_SONNET_MODEL"
                | "ANTHROPIC_MODEL"
                | "ANTHROPIC_SMALL_FAST_MODEL"
                | "CLAUDE_CODE_SUBAGENT_MODEL"
        )
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

    fn normalize_windows_launch_command(
        command: &str,
        args: &[String],
        executable_path: Option<&Path>,
    ) -> (String, Vec<String>) {
        let argv = command_words_with_args(command, args);
        let Some((program, program_args)) = argv.split_first() else {
            return (command.to_string(), args.to_vec());
        };

        if let Some((desktop_command, desktop_args)) =
            normalize_windows_desktop_app_launch(program, program_args, executable_path)
        {
            return (desktop_command, desktop_args);
        }

        if !is_windows_npm_cli_launch(program) {
            return (command.to_string(), args.to_vec());
        }

        let Some(program_path) = find_windows_command(program) else {
            return (command.to_string(), args.to_vec());
        };
        let Some(ext) = program_path
            .extension()
            .and_then(|value| value.to_str())
            .map(|value| value.to_ascii_lowercase())
        else {
            return (command.to_string(), args.to_vec());
        };
        if ext != "cmd" && ext != "ps1" {
            return (command.to_string(), args.to_vec());
        }

        let Some(js_entry) = npm_shim_js_entry(&program_path) else {
            return (command.to_string(), args.to_vec());
        };

        let mut rewritten_args = Vec::with_capacity(program_args.len() + 1);
        rewritten_args.push(js_entry.to_string_lossy().into_owned());
        rewritten_args.extend(program_args.iter().cloned());
        ("node".to_string(), rewritten_args)
    }

    fn is_windows_npm_cli_launch(program: &str) -> bool {
        command_stem_eq(program, "claude") || command_stem_eq(program, "codex")
    }

    fn normalize_windows_desktop_app_launch(
        program: &str,
        args: &[String],
        executable_path: Option<&Path>,
    ) -> Option<(String, Vec<String>)> {
        if !program.eq_ignore_ascii_case("Start-Process") {
            return None;
        }
        let (target, rest) = args.split_first()?;
        let app = windows_desktop_app_kind(target)?;
        if let Some(app_path) = executable_path
            .filter(|path| path.exists())
            .map(Path::to_path_buf)
        {
            let mut out = Vec::with_capacity(rest.len() + 2);
            out.push("-FilePath".to_string());
            out.push(app_path.to_string_lossy().into_owned());
            out.extend(rest.iter().cloned());
            return Some(("Start-Process".to_string(), out));
        }

        if let Some(app_id) = executable_path.and_then(windows_start_app_id_from_path) {
            let mut out = Vec::with_capacity(rest.len() + 1);
            out.push(format!(r"shell:AppsFolder\{app_id}"));
            out.extend(rest.iter().cloned());
            return Some(("explorer.exe".to_string(), out));
        }

        if let Some(app_id) = windows_start_app_id(app) {
            let mut out = Vec::with_capacity(rest.len() + 1);
            out.push(format!(r"shell:AppsFolder\{app_id}"));
            out.extend(rest.iter().cloned());
            return Some(("explorer.exe".to_string(), out));
        }

        let app_path = find_windows_desktop_app_exe(app)?;
        let mut out = Vec::with_capacity(rest.len() + 2);
        out.push("-FilePath".to_string());
        out.push(app_path.to_string_lossy().into_owned());
        out.extend(rest.iter().cloned());
        Some(("Start-Process".to_string(), out))
    }

    fn windows_start_app_id_from_path(path: &Path) -> Option<String> {
        let value = path.to_string_lossy();
        let value = value.trim();
        if value.is_empty() || value.contains('\\') || value.contains('/') || !value.contains('!') {
            return None;
        }
        Some(value.to_string())
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum WindowsDesktopApp {
        Claude,
        Codex,
    }

    fn windows_desktop_app_kind(target: &str) -> Option<WindowsDesktopApp> {
        if command_stem_eq(target, "claude") {
            Some(WindowsDesktopApp::Claude)
        } else if command_stem_eq(target, "codex") {
            Some(WindowsDesktopApp::Codex)
        } else {
            None
        }
    }

    fn windows_start_app_id(app: WindowsDesktopApp) -> Option<String> {
        let app_name = match app {
            WindowsDesktopApp::Claude => "Claude",
            WindowsDesktopApp::Codex => "Codex",
        };
        let script = format!(
            "$app = Get-StartApps -Name {} | Select-Object -First 1; if ($app) {{ $app.AppID }}",
            powershell_single_quoted(app_name)
        );
        let output = common::process::env::std_command("powershell.exe")
            .args(["-NoProfile", "-Command", &script])
            .output()
            .ok()?;
        if !output.status.success() {
            return None;
        }
        String::from_utf8_lossy(&output.stdout)
            .lines()
            .map(str::trim)
            .find(|line| !line.is_empty())
            .map(str::to_string)
    }

    fn find_windows_desktop_app_exe(app: WindowsDesktopApp) -> Option<PathBuf> {
        let mut candidates = match app {
            WindowsDesktopApp::Claude => claude_desktop_exe_candidates(),
            WindowsDesktopApp::Codex => codex_desktop_exe_candidates(),
        };
        candidates.sort_by_key(|path| {
            std::fs::metadata(path)
                .and_then(|metadata| metadata.modified())
                .ok()
        });
        candidates.into_iter().rev().find(|path| path.exists())
    }

    fn claude_desktop_exe_candidates() -> Vec<PathBuf> {
        let mut paths = Vec::new();
        if let Ok(localappdata) = std::env::var("LOCALAPPDATA") {
            let localappdata = Path::new(&localappdata);
            paths.push(
                localappdata
                    .join("Programs")
                    .join("Claude")
                    .join("Claude.exe"),
            );
            paths.push(
                localappdata
                    .join("Anthropic")
                    .join("Claude")
                    .join("Claude.exe"),
            );
            paths.push(localappdata.join("Claude").join("Claude.exe"));
            paths.extend(versioned_child_exe_candidates(
                &localappdata.join("AnthropicClaude"),
                "Claude.exe",
            ));
        }
        paths
    }

    fn codex_desktop_exe_candidates() -> Vec<PathBuf> {
        let mut paths = Vec::new();
        if let Ok(localappdata) = std::env::var("LOCALAPPDATA") {
            let localappdata = Path::new(&localappdata);
            paths.push(
                localappdata
                    .join("Programs")
                    .join("Codex")
                    .join("Codex.exe"),
            );
            paths.push(localappdata.join("OpenAI").join("Codex").join("Codex.exe"));
        }
        paths
    }

    fn versioned_child_exe_candidates(parent: &Path, exe_name: &str) -> Vec<PathBuf> {
        let mut paths = Vec::new();
        let Ok(entries) = std::fs::read_dir(parent) else {
            return paths;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                paths.push(path.join(exe_name));
            }
        }
        paths
    }

    fn command_stem_eq(command: &str, expected: &str) -> bool {
        let file_name = command
            .rsplit(['\\', '/'])
            .next()
            .unwrap_or(command)
            .trim_matches('"');
        let stem = file_name
            .rsplit_once('.')
            .map(|(stem, _)| stem)
            .unwrap_or(file_name);
        stem.eq_ignore_ascii_case(expected)
    }

    fn find_windows_command(program: &str) -> Option<PathBuf> {
        let program = program.trim_matches('"');
        let path = Path::new(program);
        if path.is_absolute() || program.contains('\\') || program.contains('/') {
            return existing_windows_command_path(path);
        }

        let env = common::process::env::child_env();
        let path_var = common::process::env::path_value(&env)?;
        for dir in std::env::split_paths(&path_var) {
            if let Some(candidate) = existing_windows_command_path(&dir.join(program)) {
                return Some(candidate);
            }
        }
        None
    }

    fn existing_windows_command_path(base: &Path) -> Option<PathBuf> {
        if base.extension().is_some() {
            return base.exists().then(|| base.to_path_buf());
        }

        for ext in [".ps1", ".cmd", ".exe", ".com", ".bat"] {
            let candidate = base.with_extension(ext.trim_start_matches('.'));
            if candidate.exists() {
                return Some(candidate);
            }
        }
        None
    }

    fn npm_shim_js_entry(shim_path: &Path) -> Option<PathBuf> {
        let body = std::fs::read_to_string(shim_path).ok()?;
        let token = extract_npm_shim_js_token(&body)?;
        let base_dir = shim_path.parent()?;
        let candidate = expand_npm_shim_js_token(base_dir, &token);
        candidate.exists().then_some(candidate)
    }

    fn extract_npm_shim_js_token(body: &str) -> Option<String> {
        for line in body.lines() {
            let mut rest = line;
            while let Some(start) = rest.find('"') {
                rest = &rest[start + 1..];
                let Some(end) = rest.find('"') else {
                    break;
                };
                let token = &rest[..end];
                if let Some(js_pos) = token.to_ascii_lowercase().find(".js") {
                    return Some(token[..js_pos + 3].to_string());
                }
                rest = &rest[end + 1..];
            }
        }
        None
    }

    fn expand_npm_shim_js_token(base_dir: &Path, token: &str) -> PathBuf {
        let normalized = token.replace('\\', "/");
        for prefix in ["%dp0%/", "%~dp0/", "$basedir/"] {
            if let Some(rest) = normalized.strip_prefix(prefix) {
                let mut path = base_dir.to_path_buf();
                for segment in rest.split('/') {
                    path.push(segment);
                }
                return path;
            }
        }
        PathBuf::from(token)
    }

    fn quote_windows_process_arg(value: &str) -> String {
        if !value.is_empty() && !value.chars().any(|ch| ch.is_whitespace() || ch == '"') {
            return value.to_string();
        }

        let mut out = String::with_capacity(value.len() + 2);
        out.push('"');
        let mut pending_backslashes = 0usize;
        for ch in value.chars() {
            match ch {
                '\\' => pending_backslashes += 1,
                '"' => {
                    for _ in 0..(pending_backslashes * 2 + 1) {
                        out.push('\\');
                    }
                    out.push('"');
                    pending_backslashes = 0;
                }
                other => {
                    for _ in 0..pending_backslashes {
                        out.push('\\');
                    }
                    pending_backslashes = 0;
                    out.push(other);
                }
            }
        }
        for _ in 0..(pending_backslashes * 2) {
            out.push('\\');
        }
        out.push('"');
        out
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
            windows_executable_path: None,
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
