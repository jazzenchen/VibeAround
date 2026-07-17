use std::fs;
use std::path::Path;

use anyhow::{bail, Context};
use serde_json::{Map, Value};

use crate::TerminalChoice;

pub fn resolve_terminal_choice(explicit: Option<TerminalChoice>) -> anyhow::Result<TerminalChoice> {
    if let Some(choice) = explicit {
        ensure_terminal_supported(choice, "launch profile")?;
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
    let path = common::config::settings_path();
    let config = common::config::read_settings_json().map_err(anyhow::Error::msg)?;
    if let Some(value) = config
        .get("launcher")
        .and_then(|launcher| launcher.get("terminal"))
    {
        return terminal_from_config_value(value, &path);
    }

    let choice = detect_default_terminal();
    common::config::mutate_settings_json(|config| {
        if let Some(value) = config
            .get("launcher")
            .and_then(|launcher| launcher.get("terminal"))
        {
            return terminal_from_config_value(value, &path).map_err(|error| error.to_string());
        }

        let root = config
            .as_object_mut()
            .ok_or_else(|| "settings.json root must be a JSON object".to_string())?;
        launcher_config_mut(root).insert(
            "terminal".to_string(),
            Value::String(choice.id().to_string()),
        );
        Ok(choice)
    })
    .map_err(anyhow::Error::msg)
}

fn terminal_from_config_value(value: &Value, path: &Path) -> anyhow::Result<TerminalChoice> {
    let raw = value.as_str().with_context(|| {
        format!(
            "settings config {} launcher.terminal must be a string",
            path.display()
        )
    })?;
    let choice = TerminalChoice::from_id(raw).with_context(|| {
        format!(
            "settings config {} has unknown launcher.terminal '{}'",
            path.display(),
            raw
        )
    })?;
    ensure_terminal_supported(choice, &format!("settings config {}", path.display()))?;
    Ok(choice)
}

fn ensure_terminal_supported(choice: TerminalChoice, source: &str) -> anyhow::Result<()> {
    if !choice.is_supported_on_current_platform() {
        bail!(
            "{} terminal '{}' is not supported on this platform",
            source,
            choice.id()
        );
    }
    Ok(())
}

fn launcher_config_mut(config: &mut Map<String, Value>) -> &mut Map<String, Value> {
    let launcher = config
        .entry("launcher".to_string())
        .or_insert_with(|| Value::Object(Map::new()));
    if !launcher.is_object() {
        *launcher = Value::Object(Map::new());
    }
    launcher.as_object_mut().expect("launcher object")
}

fn platform_choices() -> &'static [TerminalChoice] {
    #[cfg(target_os = "macos")]
    {
        &[TerminalChoice::Terminal, TerminalChoice::Iterm2]
    }
    #[cfg(target_os = "windows")]
    {
        return &[
            TerminalChoice::PowerShell,
            TerminalChoice::PowerShell7,
            TerminalChoice::WindowsTerminalPowerShell,
            TerminalChoice::WindowsTerminalPowerShell7,
        ];
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
        TerminalChoice::PowerShell => {
            cfg!(target_os = "windows") && command_in_path("powershell.exe")
        }
        TerminalChoice::PowerShell7 => cfg!(target_os = "windows") && command_in_path("pwsh.exe"),
        TerminalChoice::WindowsTerminalPowerShell => {
            cfg!(target_os = "windows")
                && command_in_path("wt.exe")
                && command_in_path("powershell.exe")
        }
        TerminalChoice::WindowsTerminalPowerShell7 => {
            cfg!(target_os = "windows") && command_in_path("wt.exe") && command_in_path("pwsh.exe")
        }
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn explicit_terminal_skips_config() {
        let supported = TerminalChoice::default_for_current_platform();
        let choice = resolve_terminal_choice(Some(supported)).unwrap();
        assert_eq!(choice, supported);
    }

    #[test]
    fn explicit_terminal_must_support_current_platform() {
        let unsupported = if cfg!(target_os = "macos") {
            TerminalChoice::PowerShell
        } else {
            TerminalChoice::Terminal
        };

        let error = resolve_terminal_choice(Some(unsupported))
            .unwrap_err()
            .to_string();

        assert!(error.contains("is not supported on this platform"));
    }

    #[test]
    fn initializes_terminal_in_shared_settings_config() {
        let _guard = crate::env_test_lock().lock().expect("env test lock");
        let dir = temp_dir();
        let previous = std::env::var_os("VIBEAROUND_DATA_DIR");
        std::env::set_var("VIBEAROUND_DATA_DIR", &dir);

        let choice = resolve_terminal_choice(None).expect("resolve terminal");
        let body = fs::read_to_string(dir.join("settings.json")).expect("read settings config");
        let value: Value = serde_json::from_str(&body).expect("parse settings config");
        let launcher_json_exists = dir.join("launcher.json").exists();

        restore_env("VIBEAROUND_DATA_DIR", previous);
        let _ = fs::remove_dir_all(&dir);

        assert_eq!(value["launcher"]["terminal"], choice.id());
        assert!(!launcher_json_exists);
    }

    #[test]
    fn preserves_existing_settings_config_fields() {
        let _guard = crate::env_test_lock().lock().expect("env test lock");
        let dir = temp_dir();
        fs::create_dir_all(&dir).expect("create temp dir");
        fs::write(
            dir.join("settings.json"),
            r#"{
  "workspaces": ["/tmp/work"]
}"#,
        )
        .expect("write settings config");
        let previous = std::env::var_os("VIBEAROUND_DATA_DIR");
        std::env::set_var("VIBEAROUND_DATA_DIR", &dir);

        let choice = resolve_terminal_choice(None).expect("resolve terminal");
        let body = fs::read_to_string(dir.join("settings.json")).expect("read settings config");
        let value: Value = serde_json::from_str(&body).expect("parse settings config");

        restore_env("VIBEAROUND_DATA_DIR", previous);
        let _ = fs::remove_dir_all(&dir);

        assert_eq!(value["launcher"]["terminal"], choice.id());
        assert_eq!(value["workspaces"][0], "/tmp/work");
    }

    #[test]
    fn rejects_unknown_config_terminal() {
        let _guard = crate::env_test_lock().lock().expect("env test lock");
        let dir = temp_dir();
        fs::create_dir_all(&dir).expect("create temp dir");
        fs::write(
            dir.join("settings.json"),
            r#"{ "launcher": { "terminal": "warp" } }"#,
        )
        .expect("write settings config");
        let previous = std::env::var_os("VIBEAROUND_DATA_DIR");
        std::env::set_var("VIBEAROUND_DATA_DIR", &dir);

        let error = resolve_terminal_choice(None).unwrap_err().to_string();

        restore_env("VIBEAROUND_DATA_DIR", previous);
        let _ = fs::remove_dir_all(&dir);

        assert!(error.contains("unknown launcher.terminal"));
    }

    #[test]
    fn existing_terminal_is_not_rewritten() {
        let _guard = crate::env_test_lock().lock().expect("env test lock");
        let dir = temp_dir();
        fs::create_dir_all(&dir).expect("create temp dir");
        let choice = TerminalChoice::default_for_current_platform();
        let original = format!(
            "{{\n  \"launcher\": {{ \"terminal\": \"{}\" }},\n  \"workspaces\": []\n}}\n",
            choice.id()
        );
        fs::write(dir.join("settings.json"), &original).expect("write settings config");
        let previous = std::env::var_os("VIBEAROUND_DATA_DIR");
        std::env::set_var("VIBEAROUND_DATA_DIR", &dir);

        let resolved = resolve_terminal_choice(None).expect("resolve terminal");
        let current = fs::read_to_string(dir.join("settings.json")).expect("read settings config");

        restore_env("VIBEAROUND_DATA_DIR", previous);
        let _ = fs::remove_dir_all(&dir);

        assert_eq!(resolved, choice);
        assert_eq!(current, original);
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
