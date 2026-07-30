use std::path::{Path, PathBuf};

use anyhow::Context;

use super::{command_stem_eq, command_words_with_args, powershell_single_quoted};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Target {
    Executable(PathBuf),
    StartApp(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Invocation {
    pub(super) target: Target,
    pub(super) args: Vec<String>,
}

impl Invocation {
    pub(super) fn open(&self) -> anyhow::Result<()> {
        match &self.target {
            Target::Executable(path) => open_program(path, &self.args),
            Target::StartApp(app_id) => {
                let mut args = Vec::with_capacity(self.args.len() + 1);
                args.push(format!(r"shell:AppsFolder\{app_id}"));
                args.extend(self.args.iter().cloned());
                open_program(Path::new("explorer.exe"), &args)
            }
        }
    }

    pub(super) fn into_powershell_command(self) -> (String, Vec<String>) {
        match self.target {
            Target::Executable(path) => {
                let mut args = Vec::with_capacity(self.args.len() + 2);
                args.push("-FilePath".to_string());
                args.push(path.to_string_lossy().into_owned());
                args.extend(self.args);
                ("Start-Process".to_string(), args)
            }
            Target::StartApp(app_id) => {
                let mut args = Vec::with_capacity(self.args.len() + 1);
                args.push(format!(r"shell:AppsFolder\{app_id}"));
                args.extend(self.args);
                ("explorer.exe".to_string(), args)
            }
        }
    }
}

fn open_program(program: &Path, args: &[String]) -> anyhow::Result<()> {
    if args.is_empty() {
        open::that(program).with_context(|| format!("open {}", program.display()))?;
    } else {
        let params = args
            .iter()
            .map(|arg| super::quote_windows_process_arg(arg))
            .collect::<Vec<_>>()
            .join(" ");
        open::with(params, program.to_string_lossy())
            .with_context(|| format!("open {}", program.display()))?;
    }
    Ok(())
}

pub(super) fn resolve(
    command: &str,
    args: &[String],
    configured_path: Option<&Path>,
) -> Option<Invocation> {
    let argv = command_words_with_args(command, args);
    let (program, program_args) = argv.split_first()?;
    if !program.eq_ignore_ascii_case("Start-Process") {
        return None;
    }

    let (target, rest) = program_args.split_first()?;
    let app = app_kind(target)?;

    if let Some(path) = configured_path
        .filter(|path| path.exists())
        .map(Path::to_path_buf)
    {
        return Some(Invocation {
            target: Target::Executable(path),
            args: rest.to_vec(),
        });
    }

    if let Some(app_id) = configured_path
        .and_then(start_app_id_from_path)
        .or_else(|| find_start_app_id(app))
    {
        return Some(Invocation {
            target: Target::StartApp(app_id),
            args: rest.to_vec(),
        });
    }

    Some(Invocation {
        target: Target::Executable(find_executable(app)?),
        args: rest.to_vec(),
    })
}

fn start_app_id_from_path(path: &Path) -> Option<String> {
    let value = path.to_string_lossy();
    let value = value.trim();
    if value.is_empty() || value.contains('\\') || value.contains('/') || !value.contains('!') {
        return None;
    }
    Some(value.to_string())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum App {
    Claude,
    Codex,
}

fn app_kind(target: &str) -> Option<App> {
    if command_stem_eq(target, "claude") {
        Some(App::Claude)
    } else if command_stem_eq(target, "codex") {
        Some(App::Codex)
    } else {
        None
    }
}

fn find_start_app_id(app: App) -> Option<String> {
    let script = match app {
        App::Claude => format!(
            "$app = Get-StartApps -Name {} | Select-Object -First 1; if ($app) {{ $app.AppID }}",
            powershell_single_quoted("Claude")
        ),
        App::Codex => common::resources::chatgpt_desktop_windows_start_app_query(),
    };
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

fn find_executable(app: App) -> Option<PathBuf> {
    let mut candidates = match app {
        App::Claude => claude_executable_candidates(),
        App::Codex => codex_executable_candidates(),
    };
    candidates.sort_by_key(|path| {
        std::fs::metadata(path)
            .and_then(|metadata| metadata.modified())
            .ok()
    });
    candidates.into_iter().rev().find(|path| path.exists())
}

fn claude_executable_candidates() -> Vec<PathBuf> {
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
        paths.extend(versioned_child_executables(
            &localappdata.join("AnthropicClaude"),
            "Claude.exe",
        ));
    }
    paths
}

fn codex_executable_candidates() -> Vec<PathBuf> {
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

fn versioned_child_executables(parent: &Path, executable_name: &str) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    let Ok(entries) = std::fs::read_dir(parent) else {
        return paths;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            paths.push(path.join(executable_name));
        }
    }
    paths
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_configured_executable() {
        let root = std::env::temp_dir().join(format!("VibeAround Test {}", uuid::Uuid::new_v4()));
        let executable = root.join("Codex.exe");
        std::fs::create_dir_all(&root).expect("create fixture");
        std::fs::write(&executable, "").expect("write executable fixture");

        let invocation =
            resolve("Start-Process Codex", &[], Some(&executable)).expect("desktop invocation");

        assert_eq!(invocation.target, Target::Executable(executable));
        assert!(invocation.args.is_empty());

        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn resolves_configured_start_app_id() {
        let app_id = PathBuf::from("OpenAI.Codex_2p2nqsd0c76g0!App");

        let invocation =
            resolve("Start-Process Codex", &[], Some(&app_id)).expect("desktop invocation");

        assert_eq!(
            invocation.target,
            Target::StartApp("OpenAI.Codex_2p2nqsd0c76g0!App".to_string())
        );
        assert!(invocation.args.is_empty());
    }

    #[test]
    fn leaves_cli_commands_for_terminal_launch() {
        assert_eq!(resolve("codex", &[], None), None);
        assert_eq!(resolve("claude", &[], None), None);
    }
}
