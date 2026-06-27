use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context};
use clap::Parser;
use va_launcher::{
    dry_run, launch, parse_env_pair, NativeLaunchArgs, NativeLaunchInput, TerminalChoice,
};

#[derive(Debug, Parser)]
#[command(
    name = "va-launch",
    about = "Launch local coding agents through the @va/launcher native boundary"
)]
struct CliArgs {
    #[arg(long = "stdin", conflicts_with = "input_file")]
    read_stdin: bool,
    #[arg(long = "input-file")]
    input_file: Option<PathBuf>,
    #[arg(long = "preset")]
    preset: Option<String>,
    #[arg(long = "dry-run")]
    dry_run: bool,
    #[arg(long = "json")]
    json: bool,
    #[arg(long)]
    agent: Option<String>,
    #[arg(long = "profile")]
    profile_id: Option<String>,
    #[arg(long = "target")]
    launch_target: Option<String>,
    #[arg(long)]
    workspace: Option<PathBuf>,
    #[arg(long = "session-id")]
    session_id: Option<String>,
    #[arg(long)]
    terminal: Option<TerminalChoice>,
    #[arg(long)]
    command: Option<String>,
    #[arg(long = "executable-path")]
    executable_path: Option<PathBuf>,
    #[arg(long = "window-label")]
    window_label: Option<String>,
    #[arg(long = "env", value_parser = parse_env_pair)]
    env: Vec<(String, String)>,
    #[arg(long = "arg")]
    native_args: Vec<String>,
    #[arg(value_name = "PRESET")]
    positional_preset: Option<String>,
}

fn main() {
    if let Err(error) = run() {
        eprintln!("va-launch: {error:?}");
        std::process::exit(2);
    }
}

fn run() -> anyhow::Result<()> {
    let args = CliArgs::parse();
    let input = read_input(&args)?;
    let output = if args.dry_run {
        dry_run(input)?
    } else {
        launch(input)?
    };
    if args.json {
        println!("{}", serde_json::to_string_pretty(&output)?);
    } else {
        println!("status: {:?}", output.status);
        println!(
            "command: {} {}",
            output.plan.command,
            output.plan.args.join(" ")
        );
        println!("workspace: {}", output.plan.workspace.display());
        println!("terminal: {}", output.plan.terminal);
        if let Some(script_path) = output.script_path {
            println!("script: {}", script_path.display());
        }
    }
    Ok(())
}

fn read_input(args: &CliArgs) -> anyhow::Result<NativeLaunchInput> {
    let preset_name = resolve_preset_name(args)?;
    if args.read_stdin {
        ensure_no_preset(&preset_name, "--stdin")?;
        let mut body = String::new();
        std::io::stdin()
            .read_to_string(&mut body)
            .context("read launch input from stdin")?;
        return serde_json::from_str(&body).context("parse launch input JSON from stdin");
    }
    if let Some(path) = &args.input_file {
        ensure_no_preset(&preset_name, "--input-file")?;
        let body = fs::read_to_string(path)
            .with_context(|| format!("read launch input file {}", path.display()))?;
        return serde_json::from_str(&body)
            .with_context(|| format!("parse launch input JSON from {}", path.display()));
    }
    if let Some(preset_name) = preset_name {
        let mut input = read_preset(&preset_name)?;
        apply_invocation_overrides(&mut input, args);
        return Ok(input);
    }

    let agent = args
        .agent
        .clone()
        .context("--agent is required without --stdin or --input-file")?;
    Ok(NativeLaunchInput {
        schema_version: 1,
        agent,
        profile_id: args.profile_id.clone(),
        launch_target: args.launch_target.clone(),
        workspace: args.workspace.clone(),
        session_id: args.session_id.clone(),
        terminal: args.terminal,
        command: args.command.clone(),
        executable_path: args.executable_path.clone(),
        window_label: args.window_label.clone(),
        env: args.env.iter().cloned().collect::<BTreeMap<_, _>>(),
        args: NativeLaunchArgs {
            native: args.native_args.clone(),
        },
        cleanup_paths: Vec::new(),
        macos_app_probe: None,
        windows_process_probe: None,
    })
}

fn resolve_preset_name(args: &CliArgs) -> anyhow::Result<Option<String>> {
    match (&args.preset, &args.positional_preset) {
        (Some(_), Some(_)) => bail!("use either --preset or positional PRESET, not both"),
        (Some(name), None) | (None, Some(name)) => Ok(Some(name.clone())),
        (None, None) => Ok(None),
    }
}

fn ensure_no_preset(preset_name: &Option<String>, source: &str) -> anyhow::Result<()> {
    if preset_name.is_some() {
        bail!("{source} cannot be combined with a preset");
    }
    Ok(())
}

fn read_preset(name: &str) -> anyhow::Result<NativeLaunchInput> {
    let path = preset_path(name)?;
    let body = fs::read_to_string(&path)
        .with_context(|| format!("read launch preset {}", path.display()))?;
    serde_json::from_str(&body).with_context(|| format!("parse launch preset {}", path.display()))
}

fn preset_path(name: &str) -> anyhow::Result<PathBuf> {
    if name.trim().is_empty()
        || name.contains('/')
        || name.contains('\\')
        || name == "."
        || name == ".."
        || name.contains("..")
    {
        bail!("invalid launch preset name '{}'", name);
    }
    Ok(vibearound_home()?
        .join("launches")
        .join(format!("{name}.json")))
}

fn vibearound_home() -> anyhow::Result<PathBuf> {
    if let Some(path) = non_empty_env("VIBEAROUND_HOME") {
        return Ok(PathBuf::from(path));
    }
    let home = non_empty_env("HOME")
        .or_else(|| non_empty_env("USERPROFILE"))
        .context("HOME is not set; pass --input-file instead")?;
    Ok(Path::new(&home).join(".vibearound"))
}

fn non_empty_env(key: &str) -> Option<String> {
    env::var(key).ok().filter(|value| !value.trim().is_empty())
}

fn apply_invocation_overrides(input: &mut NativeLaunchInput, args: &CliArgs) {
    if let Some(profile_id) = &args.profile_id {
        input.profile_id = Some(profile_id.clone());
    }
    if let Some(launch_target) = &args.launch_target {
        input.launch_target = Some(launch_target.clone());
    }
    if let Some(workspace) = &args.workspace {
        input.workspace = Some(workspace.clone());
    }
    if let Some(session_id) = &args.session_id {
        input.session_id = Some(session_id.clone());
    }
    if let Some(terminal) = args.terminal {
        input.terminal = Some(terminal);
    }
    if let Some(command) = &args.command {
        input.command = Some(command.clone());
    }
    if let Some(executable_path) = &args.executable_path {
        input.executable_path = Some(executable_path.clone());
    }
    if let Some(window_label) = &args.window_label {
        input.window_label = Some(window_label.clone());
    }
    input.env.extend(args.env.iter().cloned());
    if !args.native_args.is_empty() {
        input.args.native.extend(args.native_args.clone());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cli_args() -> CliArgs {
        CliArgs {
            read_stdin: false,
            input_file: None,
            preset: None,
            dry_run: true,
            json: true,
            agent: None,
            profile_id: None,
            launch_target: None,
            workspace: None,
            session_id: None,
            terminal: None,
            command: None,
            executable_path: None,
            window_label: None,
            env: Vec::new(),
            native_args: Vec::new(),
            positional_preset: None,
        }
    }

    #[test]
    fn rejects_path_like_preset_names() {
        assert!(preset_path("../secret").is_err());
        assert!(preset_path("nested/name").is_err());
    }

    #[test]
    fn invocation_overrides_extend_preset() {
        let mut input = NativeLaunchInput {
            schema_version: 1,
            agent: "codex".to_string(),
            profile_id: None,
            launch_target: None,
            workspace: Some(PathBuf::from("/old")),
            session_id: None,
            terminal: None,
            command: None,
            executable_path: None,
            window_label: None,
            env: BTreeMap::new(),
            args: NativeLaunchArgs::default(),
            cleanup_paths: Vec::new(),
            macos_app_probe: None,
            windows_process_probe: None,
        };
        let mut args = cli_args();
        args.workspace = Some(PathBuf::from("/new"));
        args.session_id = Some("abc".to_string());
        args.env = vec![("NO_PROXY".to_string(), "localhost".to_string())];
        args.native_args = vec!["--flag".to_string()];

        apply_invocation_overrides(&mut input, &args);

        assert_eq!(input.workspace, Some(PathBuf::from("/new")));
        assert_eq!(input.session_id.as_deref(), Some("abc"));
        assert_eq!(input.env["NO_PROXY"], "localhost");
        assert_eq!(input.args.native, vec!["--flag"]);
    }
}
