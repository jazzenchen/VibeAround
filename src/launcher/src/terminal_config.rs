use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context};
use serde_json::{Map, Value};

use crate::{paths, TerminalChoice};

pub fn launcher_config_path() -> anyhow::Result<PathBuf> {
    paths::launcher_config_path()
}

pub fn resolve_terminal_choice(explicit: Option<TerminalChoice>) -> anyhow::Result<TerminalChoice> {
    if let Some(choice) = explicit {
        return Ok(choice);
    }
    read_or_initialize_terminal()
}

pub fn detect_default_terminal() -> TerminalChoice {
    platform_choices()
        .iter()
        .copied()
        .find(|choice| is_installed(*choice))
        .unwrap_or_else(TerminalChoice::default_for_current_platform)
}

fn read_or_initialize_terminal() -> anyhow::Result<TerminalChoice> {
    let path = paths::launcher_config_path()?;
    let mut config = read_launcher_config(&path)?;
    if let Some(value) = config.get("terminal") {
        return terminal_from_config_value(value, &path);
    }

    let choice = detect_default_terminal();
    config.insert(
        "terminal".to_string(),
        Value::String(choice.id().to_string()),
    );
    write_launcher_config(&path, &config)?;
    Ok(choice)
}

fn terminal_from_config_value(value: &Value, path: &Path) -> anyhow::Result<TerminalChoice> {
    let raw = value.as_str().with_context(|| {
        format!(
            "launcher config {} terminal must be a string",
            path.display()
        )
    })?;
    let choice = TerminalChoice::from_id(raw).with_context(|| {
        format!(
            "launcher config {} has unknown terminal '{}'",
            path.display(),
            raw
        )
    })?;
    if !choice.is_supported_on_current_platform() {
        bail!(
            "launcher config {} terminal '{}' is not supported on this platform",
            path.display(),
            raw
        );
    }
    Ok(choice)
}

fn read_launcher_config(path: &Path) -> anyhow::Result<Map<String, Value>> {
    let body = match fs::read_to_string(path) {
        Ok(body) => body,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Map::new()),
        Err(error) => return Err(error).with_context(|| format!("read {}", path.display())),
    };
    let value: Value =
        serde_json::from_str(&body).with_context(|| format!("parse {}", path.display()))?;
    match value {
        Value::Object(object) => Ok(object),
        _ => bail!("launcher config {} must be a JSON object", path.display()),
    }
}

fn write_launcher_config(path: &Path, config: &Map<String, Value>) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    }
    let body = serde_json::to_string_pretty(config).context("serialize launcher config")?;
    let tmp = path.with_extension("tmp");
    fs::write(&tmp, body).with_context(|| format!("write {}", tmp.display()))?;
    set_owner_only(&tmp).ok();
    fs::rename(&tmp, path)
        .with_context(|| format!("rename {} to {}", tmp.display(), path.display()))?;
    Ok(())
}

fn platform_choices() -> &'static [TerminalChoice] {
    #[cfg(target_os = "macos")]
    {
        return &[TerminalChoice::Terminal, TerminalChoice::Iterm2];
    }
    #[cfg(target_os = "windows")]
    {
        return &[TerminalChoice::PowerShell];
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        return &[
            TerminalChoice::SystemTerminal,
            TerminalChoice::GnomeTerminal,
            TerminalChoice::Konsole,
            TerminalChoice::XfceTerminal,
            TerminalChoice::Xterm,
            TerminalChoice::Kitty,
            TerminalChoice::Alacritty,
            TerminalChoice::WezTerm,
        ];
    }
}

fn is_installed(choice: TerminalChoice) -> bool {
    match choice {
        TerminalChoice::Terminal => cfg!(target_os = "macos"),
        TerminalChoice::Iterm2 => Path::new("/Applications/iTerm.app").exists(),
        TerminalChoice::PowerShell => cfg!(target_os = "windows"),
        TerminalChoice::SystemTerminal => [
            "xdg-terminal-exec",
            "x-terminal-emulator",
            "gnome-terminal",
            "konsole",
            "xfce4-terminal",
            "kitty",
            "alacritty",
            "wezterm",
            "xterm",
        ]
        .iter()
        .any(|program| command_in_path(program)),
        TerminalChoice::GnomeTerminal => command_in_path("gnome-terminal"),
        TerminalChoice::Konsole => command_in_path("konsole"),
        TerminalChoice::XfceTerminal => command_in_path("xfce4-terminal"),
        TerminalChoice::Xterm => command_in_path("xterm"),
        TerminalChoice::Kitty => command_in_path("kitty"),
        TerminalChoice::Alacritty => command_in_path("alacritty"),
        TerminalChoice::WezTerm => command_in_path("wezterm"),
    }
}

fn command_in_path(program: &str) -> bool {
    let Some(path_var) = std::env::var_os("PATH") else {
        return false;
    };
    std::env::split_paths(&path_var).any(|dir| is_executable_file(&dir.join(program)))
}

fn is_executable_file(path: &Path) -> bool {
    let Ok(metadata) = fs::metadata(path) else {
        return false;
    };
    if !metadata.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        metadata.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        true
    }
}

fn set_owner_only(path: &Path) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    }
    #[cfg(not(unix))]
    {
        let _ = path;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explicit_terminal_skips_config() {
        let choice = resolve_terminal_choice(Some(TerminalChoice::Terminal)).unwrap();
        assert_eq!(choice, TerminalChoice::Terminal);
    }

    #[test]
    fn initializes_terminal_in_shared_launcher_config() {
        let _guard = crate::env_test_lock().lock().expect("env test lock");
        let dir = temp_dir();
        let previous = std::env::var_os("VIBEAROUND_DATA_DIR");
        std::env::set_var("VIBEAROUND_DATA_DIR", &dir);

        let choice = resolve_terminal_choice(None).expect("resolve terminal");
        let body = fs::read_to_string(dir.join("launcher.json")).expect("read launcher config");
        let value: Value = serde_json::from_str(&body).expect("parse launcher config");

        restore_env("VIBEAROUND_DATA_DIR", previous);
        let _ = fs::remove_dir_all(&dir);

        assert_eq!(value["terminal"], choice.id());
    }

    #[test]
    fn preserves_existing_launcher_config_fields() {
        let _guard = crate::env_test_lock().lock().expect("env test lock");
        let dir = temp_dir();
        fs::create_dir_all(&dir).expect("create temp dir");
        fs::write(
            dir.join("launcher.json"),
            r#"{
  "workspace": "/tmp/work"
}"#,
        )
        .expect("write launcher config");
        let previous = std::env::var_os("VIBEAROUND_DATA_DIR");
        std::env::set_var("VIBEAROUND_DATA_DIR", &dir);

        let choice = resolve_terminal_choice(None).expect("resolve terminal");
        let body = fs::read_to_string(dir.join("launcher.json")).expect("read launcher config");
        let value: Value = serde_json::from_str(&body).expect("parse launcher config");

        restore_env("VIBEAROUND_DATA_DIR", previous);
        let _ = fs::remove_dir_all(&dir);

        assert_eq!(value["terminal"], choice.id());
        assert_eq!(value["workspace"], "/tmp/work");
    }

    #[test]
    fn rejects_unknown_config_terminal() {
        let _guard = crate::env_test_lock().lock().expect("env test lock");
        let dir = temp_dir();
        fs::create_dir_all(&dir).expect("create temp dir");
        fs::write(dir.join("launcher.json"), r#"{ "terminal": "warp" }"#)
            .expect("write launcher config");
        let previous = std::env::var_os("VIBEAROUND_DATA_DIR");
        std::env::set_var("VIBEAROUND_DATA_DIR", &dir);

        let error = resolve_terminal_choice(None).unwrap_err().to_string();

        restore_env("VIBEAROUND_DATA_DIR", previous);
        let _ = fs::remove_dir_all(&dir);

        assert!(error.contains("unknown terminal"));
    }

    fn temp_dir() -> PathBuf {
        std::env::temp_dir().join(format!("va-launch-config-test-{}", uuid::Uuid::new_v4()))
    }

    fn restore_env(key: &str, previous: Option<std::ffi::OsString>) {
        match previous {
            Some(value) => std::env::set_var(key, value),
            None => std::env::remove_var(key),
        }
    }
}
