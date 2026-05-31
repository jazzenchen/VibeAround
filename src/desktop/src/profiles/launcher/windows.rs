use std::path::{Path, PathBuf};

use anyhow::{bail, Context};

use super::common::{command_words_with_args, LaunchPlan};
use crate::profiles::terminal::{self, TerminalChoice};

pub(super) fn spawn(plan: LaunchPlan) -> anyhow::Result<()> {
    let choice = terminal::read_preference();
    match choice {
        TerminalChoice::PowerShell => spawn_powershell(plan),
        other => bail!("terminal '{}' is not supported on Windows", other.id()),
    }
}

fn spawn_powershell(plan: LaunchPlan) -> anyhow::Result<()> {
    let script_path = write_powershell_launch_script(&plan)?;
    let params = format!(
        "-ExecutionPolicy Bypass -NoExit -File {}",
        quote_windows_process_arg(&script_path.to_string_lossy())
    );

    // Use ShellExecuteW through the `open` crate instead of Rust `Command`.
    // `Command` inherits all inheritable handles by default on Windows; if a
    // launched CLI keeps the daemon's TCP listener handle alive, VibeAround's
    // next start sees 127.0.0.1:12358 as occupied by a stale PID.
    open::with(params, "powershell.exe").context("open PowerShell")?;
    Ok(())
}

fn write_powershell_launch_script(plan: &LaunchPlan) -> anyhow::Result<PathBuf> {
    let (command, args) = normalize_windows_launch_command(&plan.command, &plan.args);
    let script_path =
        std::env::temp_dir().join(format!("vibearound-launch-{}.ps1", uuid::Uuid::new_v4()));
    let body = build_powershell_script(plan, &command, &args);
    std::fs::write(&script_path, body)
        .with_context(|| format!("write launch script {:?}", script_path))?;
    ::common::auth::set_owner_only(&script_path).ok();
    Ok(script_path)
}

fn build_powershell_script(plan: &LaunchPlan, command: &str, args: &[String]) -> String {
    let mut out = String::new();
    let (env, args) = normalize_windows_claude_profile_launch(plan, command, args);
    out.push_str(&format!(
        "$Host.UI.RawUI.WindowTitle = {}\n",
        powershell_single_quoted(&format!("VibeAround - {}", plan.window_label))
    ));
    out.push_str(&format!(
        "Write-Host '# VibeAround profile: {}'\n",
        plan.window_label.replace('\'', "''")
    ));
    for (k, v) in &env {
        out.push_str(&format!("$env:{} = '{}'\n", k, v.replace('\'', "''")));
    }
    append_powershell_color_env(&mut out);
    out.push_str(&format!(
        "Set-Location -LiteralPath '{}'\n",
        escape_powershell_single_quoted(&plan.workspace.to_string_lossy())
    ));
    out.push_str(&powershell_command_block(command, &args));
    out.push('\n');
    out.push_str("if ($LASTEXITCODE -ne $null -and $LASTEXITCODE -ne 0) {\n");
    out.push_str("  Write-Host \"`nCommand exited with code $LASTEXITCODE\"\n");
    out.push_str("}\n");
    out.push_str("$scriptPath = $MyInvocation.MyCommand.Path\n");
    out.push_str("if ($scriptPath) { Remove-Item -LiteralPath $scriptPath -Force -ErrorAction SilentlyContinue }\n");
    out
}

fn normalize_windows_claude_profile_launch(
    plan: &LaunchPlan,
    command: &str,
    args: &[String],
) -> (Vec<(String, String)>, Vec<String>) {
    let mut env = plan.env.clone();
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

fn append_powershell_color_env(out: &mut String) {
    out.push_str("Remove-Item Env:NO_COLOR -ErrorAction SilentlyContinue\n");
    out.push_str("if (-not $env:TERM -or $env:TERM -eq 'dumb') { $env:TERM = 'xterm-256color' }\n");
    out.push_str("if (-not $env:COLORTERM) { $env:COLORTERM = 'truecolor' }\n");
    out.push_str("if (-not $env:CLICOLOR) { $env:CLICOLOR = '1' }\n");
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
    format!("'{}'", escape_powershell_single_quoted(value))
}

fn escape_powershell_single_quoted(value: &str) -> String {
    value.replace('\'', "''")
}

fn normalize_windows_launch_command(command: &str, args: &[String]) -> (String, Vec<String>) {
    let argv = command_words_with_args(command, args);
    let Some((program, program_args)) = argv.split_first() else {
        return (command.to_string(), args.to_vec());
    };

    if !command_stem_eq(program, "codex") {
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

    let Some(codex_js) = npm_shim_js_entry(&program_path) else {
        return (command.to_string(), args.to_vec());
    };

    let mut rewritten_args = Vec::with_capacity(program_args.len() + 1);
    rewritten_args.push(codex_js.to_string_lossy().into_owned());
    rewritten_args.extend(program_args.iter().cloned());
    ("node".to_string(), rewritten_args)
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

    let path_var = std::env::var_os("PATH")?;
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

#[cfg(test)]
mod tests {
    use super::*;

    fn plan(command: &str, args: Vec<String>) -> LaunchPlan {
        LaunchPlan {
            env: Vec::new(),
            command: command.to_string(),
            args,
            window_label: "Codex Test".to_string(),
            workspace: PathBuf::from(r"C:\Users\tester\project"),
        }
    }

    #[test]
    fn powershell_script_keeps_codex_hook_config_as_one_argument() {
        let hook_config = r#"hooks.SessionStart=[{ matcher = 'startup|resume|clear', hooks = [{ type = 'command', command = '"C:\Program Files\VibeAround\vibearound-hook.exe" --agent codex --event SessionStart', timeout = 5 }] }]"#;
        let plan = plan("codex", vec!["-c".to_string(), hook_config.to_string()]);
        let script = build_powershell_script(&plan, &plan.command, &plan.args);

        assert!(script.contains("$vaArgs = @(\n"));
        assert!(script.contains("  '-c'\n"));
        assert!(script.contains("C:\\Program Files\\VibeAround\\vibearound-hook.exe"));
        assert!(script.contains("& $vaCommand @vaArgs"));
        assert!(!script.contains("Files\\VibeAround\\vibearound-hook.exe'\n"));
    }

    #[test]
    fn rewrites_quoted_codex_npm_shim_under_space_path_to_node() {
        let root = std::env::temp_dir().join(format!("VibeAround Test {}", uuid::Uuid::new_v4()));
        let bin_dir = root.join("bin");
        let codex_js = bin_dir
            .join("node_modules")
            .join("@openai")
            .join("codex")
            .join("bin")
            .join("codex.js");
        std::fs::create_dir_all(codex_js.parent().expect("codex js parent"))
            .expect("create shim fixture");
        std::fs::write(&codex_js, "console.log('codex');\n").expect("write codex js fixture");
        let shim = bin_dir.join("codex.cmd");
        std::fs::write(
            &shim,
            r#"@ECHO off
node "%~dp0\node_modules\@openai\codex\bin\codex.js" %*
"#,
        )
        .expect("write codex cmd fixture");

        let command = format!("\"{}\"", shim.to_string_lossy());
        let (program, args) = normalize_windows_launch_command(
            &command,
            &["-c".to_string(), "features.hooks=true".to_string()],
        );

        assert_eq!(program, "node");
        assert_eq!(PathBuf::from(&args[0]), codex_js);
        assert_eq!(
            &args[1..],
            ["-c".to_string(), "features.hooks=true".to_string()]
        );

        std::fs::remove_dir_all(root).ok();
    }
}
