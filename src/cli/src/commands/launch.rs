use crate::args::{LaunchRunArgs, Options};
use crate::error::CliError;

pub(super) fn run(options: &Options, args: &LaunchRunArgs) -> Result<(), CliError> {
    let input = match (&args.profile, &args.profile_path) {
        (Some(name), None) => va_launcher::load_launch_profile(name),
        (None, Some(path)) => va_launcher::load_launch_profile_path(path),
        _ => unreachable!("launch args are validated by parser"),
    }
    .map_err(|error| CliError::Launch(format!("{error:#}")))?;

    let output = if args.dry_run {
        va_launcher::dry_run(input)
    } else {
        va_launcher::launch(input)
    }
    .map_err(|error| CliError::Launch(format!("{error:#}")))?;

    if options.json {
        crate::print_json(serde_json::to_value(output)?)?;
        return Ok(());
    }

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
    Ok(())
}
