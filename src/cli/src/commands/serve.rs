use std::env;
use std::path::PathBuf;
use std::process::Command as ProcessCommand;

use crate::args::{Options, ServeArgs};
use crate::error::CliError;

pub(super) fn run(options: &Options, args: &ServeArgs) -> Result<(), CliError> {
    if options.json {
        return Err(CliError::Usage("serve does not support --json".into()));
    }

    let server_bin = resolve_server_binary(args)?;
    let server_args = server_args(args);
    eprintln!(
        "starting {} {}",
        server_bin.display(),
        shell_words(&server_args)
    );
    let status = ProcessCommand::new(&server_bin)
        .args(&server_args)
        .status()
        .map_err(|source| CliError::Io {
            action: "starting vibearound-server",
            source,
        })?;
    if status.success() {
        Ok(())
    } else {
        Err(CliError::ProcessExit {
            program: server_bin.display().to_string(),
            status,
        })
    }
}

fn resolve_server_binary(args: &ServeArgs) -> Result<PathBuf, CliError> {
    if let Some(path) = &args.server_bin {
        return Ok(path.clone());
    }

    let name = server_binary_name();
    if let Ok(current) = env::current_exe() {
        if let Some(dir) = current.parent() {
            let candidate = dir.join(name);
            if candidate.exists() {
                return Ok(candidate);
            }
        }
    }

    if let Some(path) = find_on_path(name) {
        return Ok(path);
    }

    Err(CliError::Usage(format!(
        "could not find {name}; build it with `cargo build --manifest-path src/Cargo.toml -p server` or pass --server-bin PATH"
    )))
}

fn server_args(args: &ServeArgs) -> Vec<String> {
    let mut out = Vec::new();
    if let Some(port) = args.port {
        out.push("--port".into());
        out.push(port.to_string());
    }
    if let Some(path) = &args.data_dir {
        out.push("--data-dir".into());
        out.push(path.display().to_string());
    }
    if let Some(path) = &args.web_dist {
        out.push("--web-dist".into());
        out.push(path.display().to_string());
    }
    if let Some(auth_mode) = &args.auth_mode {
        out.push("--auth-mode".into());
        out.push(auth_mode.clone());
    }
    out
}

fn find_on_path(name: &str) -> Option<PathBuf> {
    env::var_os("PATH").and_then(|paths| {
        env::split_paths(&paths)
            .map(|path| path.join(name))
            .find(|path| path.exists())
    })
}

fn server_binary_name() -> &'static str {
    if cfg!(windows) {
        "vibearound-server.exe"
    } else {
        "vibearound-server"
    }
}

fn shell_words(args: &[String]) -> String {
    args.iter()
        .map(|arg| {
            if arg.contains(char::is_whitespace) {
                format!("{arg:?}")
            } else {
                arg.clone()
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_server_args() {
        let args = ServeArgs {
            port: Some(12358),
            data_dir: Some(PathBuf::from("/tmp/va-data")),
            web_dist: Some(PathBuf::from("/tmp/web")),
            auth_mode: Some("token".into()),
            server_bin: None,
        };

        assert_eq!(
            server_args(&args),
            vec![
                "--port".to_string(),
                "12358".to_string(),
                "--data-dir".to_string(),
                "/tmp/va-data".to_string(),
                "--web-dist".to_string(),
                "/tmp/web".to_string(),
                "--auth-mode".to_string(),
                "token".to_string(),
            ]
        );
    }

    #[test]
    fn explicit_server_bin_wins() {
        let args = ServeArgs {
            port: None,
            data_dir: None,
            web_dist: None,
            auth_mode: None,
            server_bin: Some(PathBuf::from("/tmp/vibearound-server")),
        };

        assert_eq!(
            resolve_server_binary(&args).expect("path"),
            PathBuf::from("/tmp/vibearound-server")
        );
    }
}
