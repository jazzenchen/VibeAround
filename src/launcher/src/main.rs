use std::path::PathBuf;

use anyhow::bail;
use clap::Parser;
use va_launcher::{
    dry_run, launch, load_launch_profile, load_launch_profile_path, NativeLaunchInput,
};

#[derive(Debug, Parser)]
#[command(
    name = "va-launch",
    about = "Launch local coding agents through the @va/launcher native boundary"
)]
struct CliArgs {
    #[arg(long = "profile")]
    profile: Option<String>,
    #[arg(long = "profile-path")]
    profile_path: Option<PathBuf>,
    #[arg(long = "dry-run")]
    dry_run: bool,
    #[arg(long = "json")]
    json: bool,
}

fn main() {
    if let Err(error) = run() {
        eprintln!("va-launch: {error:?}");
        std::process::exit(2);
    }
}

fn run() -> anyhow::Result<()> {
    common::migration::run()?;
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
    match (&args.profile, &args.profile_path) {
        (Some(_), Some(_)) => bail!("use either --profile or --profile-path, not both"),
        (Some(name), None) => load_launch_profile(name),
        (None, Some(path)) => load_launch_profile_path(path),
        (None, None) => {
            bail!("launch profile is required; pass --profile <name> or --profile-path <path>")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cli_args() -> CliArgs {
        CliArgs {
            profile: None,
            profile_path: None,
            dry_run: true,
            json: true,
        }
    }

    #[test]
    fn requires_launch_profile_source() {
        let args = cli_args();

        let error = read_input(&args).unwrap_err().to_string();

        assert!(error.contains("launch profile is required"));
    }

    #[test]
    fn reads_named_launch_profile() {
        let _guard = env_test_lock().lock().expect("env test lock");
        let dir = temp_dir();
        let profile_dir = dir.join("profiles");
        std::fs::create_dir_all(&profile_dir).expect("create profile dir");
        std::fs::write(
            dir.join("settings.json"),
            r#"{ "default_agent": "codex", "enabled_agents": ["codex"] }"#,
        )
        .expect("write settings");
        std::fs::write(
            profile_dir.join("openai.json"),
            r#"{
  "id": "openai",
  "label": "OpenAI",
  "provider": "xai",
  "auth_mode": "api_key",
  "api_configs": {
    "openai-responses": { "enabled": true }
  },
  "credentials": {
    "api_key": "secret"
  }
}"#,
        )
        .expect("write profile");
        let previous = std::env::var_os("VIBEAROUND_DATA_DIR");
        std::env::set_var("VIBEAROUND_DATA_DIR", &dir);
        let mut args = cli_args();
        args.profile = Some("openai".to_string());

        let input = read_input(&args).expect("read profile input");

        restore_env("VIBEAROUND_DATA_DIR", previous);
        let _ = std::fs::remove_dir_all(&dir);

        assert_eq!(input.agent, "codex");
        assert_eq!(input.profile_id.as_deref(), Some("openai"));
        assert_eq!(input.workspace, Some(dir.join("workspaces")));
    }

    #[test]
    fn rejects_multiple_profile_sources() {
        let mut args = cli_args();
        args.profile = Some("openai".to_string());
        args.profile_path = Some(PathBuf::from("/tmp/profile.json"));

        let error = read_input(&args).unwrap_err().to_string();

        assert!(error.contains("use either --profile or --profile-path"));
    }

    #[test]
    fn rejects_legacy_direct_launch_flags() {
        let error = CliArgs::try_parse_from(["va-launch", "--agent", "codex"]).unwrap_err();

        assert_eq!(error.kind(), clap::error::ErrorKind::UnknownArgument);
    }

    fn temp_dir() -> PathBuf {
        std::env::temp_dir().join(format!("va-launch-cli-test-{}", uuid::Uuid::new_v4()))
    }

    fn restore_env(key: &str, previous: Option<std::ffi::OsString>) {
        match previous {
            Some(value) => std::env::set_var(key, value),
            None => std::env::remove_var(key),
        }
    }

    fn env_test_lock() -> &'static std::sync::Mutex<()> {
        static LOCK: std::sync::OnceLock<std::sync::Mutex<()>> = std::sync::OnceLock::new();
        LOCK.get_or_init(|| std::sync::Mutex::new(()))
    }
}
