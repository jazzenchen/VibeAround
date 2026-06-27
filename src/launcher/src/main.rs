use std::collections::BTreeMap;
use std::fs;
use std::io::Read;
use std::path::PathBuf;

use anyhow::Context;
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
    if args.read_stdin {
        let mut body = String::new();
        std::io::stdin()
            .read_to_string(&mut body)
            .context("read launch input from stdin")?;
        return serde_json::from_str(&body).context("parse launch input JSON from stdin");
    }
    if let Some(path) = &args.input_file {
        let body = fs::read_to_string(path)
            .with_context(|| format!("read launch input file {}", path.display()))?;
        return serde_json::from_str(&body)
            .with_context(|| format!("parse launch input JSON from {}", path.display()));
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
