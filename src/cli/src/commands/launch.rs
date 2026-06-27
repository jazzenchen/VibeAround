use crate::args::{LaunchRunArgs, Options};
use crate::error::CliError;
use std::{
    env,
    ffi::{OsStr, OsString},
    path::PathBuf,
    process::Command as ProcessCommand,
};

pub(super) fn run(options: &Options, args: &LaunchRunArgs) -> Result<(), CliError> {
    let launcher = resolve_va_launch_binary()?;
    let status = ProcessCommand::new(&launcher)
        .args(launcher_args(options, args))
        .status()
        .map_err(|source| CliError::Io {
            action: "starting va-launch",
            source,
        })?;

    if status.success() {
        Ok(())
    } else {
        Err(CliError::ProcessExit {
            program: launcher.display().to_string(),
            status,
        })
    }
}

fn launcher_args(options: &Options, args: &LaunchRunArgs) -> Vec<OsString> {
    let mut forwarded = Vec::new();
    if options.json {
        forwarded.push(OsString::from("--json"));
    }
    match (&args.profile, &args.profile_path) {
        (Some(name), None) => {
            forwarded.push(OsString::from("--profile"));
            forwarded.push(OsString::from(name));
        }
        (None, Some(path)) => {
            forwarded.push(OsString::from("--profile-path"));
            forwarded.push(path.as_os_str().to_owned());
        }
        _ => unreachable!("launch args are validated by parser"),
    }
    if args.dry_run {
        forwarded.push(OsString::from("--dry-run"));
    }
    forwarded
}

fn resolve_va_launch_binary() -> Result<PathBuf, CliError> {
    if let Some(path) = env::var_os("VIBEAROUND_VA_LAUNCH_BIN") {
        let path = PathBuf::from(path);
        if path.is_file() {
            return Ok(path);
        }
        return Err(CliError::Launch(format!(
            "VIBEAROUND_VA_LAUNCH_BIN is not a file: {}",
            path.display()
        )));
    }

    for path in packaged_launcher_candidates() {
        if path.is_file() {
            return Ok(path);
        }
    }

    Err(CliError::Launch(format!(
        "{} binary not found next to va; build/package va-launcher or set VIBEAROUND_VA_LAUNCH_BIN",
        launcher_binary_name()
    )))
}

fn packaged_launcher_candidates() -> Vec<PathBuf> {
    let binary = launcher_binary_name();
    let mut candidates = Vec::new();

    if let Ok(exe) = env::current_exe() {
        if let Some(parent) = exe.parent() {
            candidates.push(parent.join(binary));
            if parent
                .file_name()
                .is_some_and(|name| name == OsStr::new("deps"))
            {
                if let Some(target_profile_dir) = parent.parent() {
                    candidates.push(target_profile_dir.join(binary));
                }
            }
        }
    }

    let target_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("target");
    for profile in ["debug", "release"] {
        candidates.push(target_dir.join(profile).join(binary));
    }

    candidates
}

fn launcher_binary_name() -> &'static str {
    if cfg!(target_os = "windows") {
        "va-launch.exe"
    } else {
        "va-launch"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn forwards_profile_args_to_va_launch() {
        let options = Options {
            json: true,
            ..Default::default()
        };
        let args = LaunchRunArgs {
            profile: Some("codex-work".into()),
            profile_path: None,
            dry_run: true,
        };

        assert_eq!(
            launcher_args(&options, &args),
            vec![
                OsString::from("--json"),
                OsString::from("--profile"),
                OsString::from("codex-work"),
                OsString::from("--dry-run"),
            ]
        );
    }

    #[test]
    fn forwards_profile_path_args_to_va_launch() {
        let options = Options::default();
        let args = LaunchRunArgs {
            profile: None,
            profile_path: Some(PathBuf::from("/tmp/profile.json")),
            dry_run: false,
        };

        assert_eq!(
            launcher_args(&options, &args),
            vec![
                OsString::from("--profile-path"),
                OsString::from("/tmp/profile.json"),
            ]
        );
    }

    #[test]
    fn explicit_va_launch_binary_must_exist() {
        let _guard = env_test_lock().lock().expect("env test lock");
        let previous = env::var_os("VIBEAROUND_VA_LAUNCH_BIN");
        env::set_var("VIBEAROUND_VA_LAUNCH_BIN", "/definitely/not/va-launch");

        let error = resolve_va_launch_binary().unwrap_err().to_string();

        restore_env("VIBEAROUND_VA_LAUNCH_BIN", previous);
        assert!(error.contains("VIBEAROUND_VA_LAUNCH_BIN is not a file"));
    }

    #[cfg(unix)]
    #[test]
    fn run_execs_va_launch_binary() {
        use std::os::unix::fs::PermissionsExt;

        let _guard = env_test_lock().lock().expect("env test lock");
        let dir = env::temp_dir().join(format!(
            "va-cli-launch-exec-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("system time")
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).expect("create temp dir");
        let fake_bin = dir.join("va-launch");
        let capture = dir.join("args.txt");
        std::fs::write(
            &fake_bin,
            "#!/bin/sh\nprintf '%s\\n' \"$@\" > \"$VA_LAUNCH_CAPTURE\"\n",
        )
        .expect("write fake va-launch");
        std::fs::set_permissions(&fake_bin, std::fs::Permissions::from_mode(0o700))
            .expect("chmod fake va-launch");

        let previous_bin = env::var_os("VIBEAROUND_VA_LAUNCH_BIN");
        let previous_capture = env::var_os("VA_LAUNCH_CAPTURE");
        env::set_var("VIBEAROUND_VA_LAUNCH_BIN", &fake_bin);
        env::set_var("VA_LAUNCH_CAPTURE", &capture);

        let options = Options {
            json: true,
            ..Default::default()
        };
        let args = LaunchRunArgs {
            profile: Some("codex-work".into()),
            profile_path: None,
            dry_run: true,
        };
        run(&options, &args).expect("exec fake va-launch");

        restore_env("VIBEAROUND_VA_LAUNCH_BIN", previous_bin);
        restore_env("VA_LAUNCH_CAPTURE", previous_capture);

        let captured = std::fs::read_to_string(&capture).expect("read captured args");
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(captured, "--json\n--profile\ncodex-work\n--dry-run\n");
    }

    fn restore_env(key: &str, previous: Option<std::ffi::OsString>) {
        match previous {
            Some(value) => env::set_var(key, value),
            None => env::remove_var(key),
        }
    }

    fn env_test_lock() -> &'static std::sync::Mutex<()> {
        static LOCK: std::sync::OnceLock<std::sync::Mutex<()>> = std::sync::OnceLock::new();
        LOCK.get_or_init(|| std::sync::Mutex::new(()))
    }
}
