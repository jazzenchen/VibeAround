//! Terminal-app preferences.
//!
//! v1 supports native terminal choices per platform. The preference lives
//! under `settings.json.launcher` with the rest of the Launch tab state.
//!
//! Adding more terminals (Ghostty, WezTerm, Warp, …) is a matter of:
//!   1. adding a variant to `TerminalChoice`,
//!   2. teaching `detect_installed` how to find it, and
//!   3. adding an OS/terminal executor under `launcher/`.
//!
//! No catalog changes; no schema migration.

use std::path::{Path, PathBuf};

use anyhow::Context;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use common::config;

// ---------------------------------------------------------------------------
// Choice enum
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TerminalChoice {
    Terminal,
    Iterm2,
    PowerShell,
    PowerShell7,
    WindowsTerminalPowerShell,
    WindowsTerminalPowerShell7,
    SystemTerminal,
    GnomeTerminal,
    Konsole,
    XfceTerminal,
    Xterm,
    Kitty,
    Alacritty,
    WezTerm,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CompatibilityBridgeMode {
    Auto,
    On,
    Off,
}

impl CompatibilityBridgeMode {
    #[cfg(test)]
    pub const ALL: &'static [CompatibilityBridgeMode] = &[Self::Auto, Self::On, Self::Off];

    #[cfg(test)]
    pub fn id(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::On => "on",
            Self::Off => "off",
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        match id {
            "auto" => Some(Self::Auto),
            "on" => Some(Self::On),
            "off" => Some(Self::Off),
            _ => None,
        }
    }
}

impl TerminalChoice {
    #[cfg(target_os = "macos")]
    pub const ALL: &'static [TerminalChoice] = &[Self::Terminal, Self::Iterm2];
    #[cfg(target_os = "windows")]
    pub const ALL: &'static [TerminalChoice] = &[
        Self::PowerShell,
        Self::PowerShell7,
        Self::WindowsTerminalPowerShell,
        Self::WindowsTerminalPowerShell7,
    ];
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    pub const ALL: &'static [TerminalChoice] = &[
        Self::SystemTerminal,
        Self::GnomeTerminal,
        Self::Konsole,
        Self::XfceTerminal,
        Self::Xterm,
        Self::Kitty,
        Self::Alacritty,
        Self::WezTerm,
    ];

    pub fn id(self) -> &'static str {
        match self {
            Self::Terminal => "terminal",
            Self::Iterm2 => "iterm2",
            Self::PowerShell => "powershell",
            Self::PowerShell7 => "powershell-7",
            Self::WindowsTerminalPowerShell => "windows-terminal-powershell",
            Self::WindowsTerminalPowerShell7 => "windows-terminal-powershell-7",
            Self::SystemTerminal => "system-terminal",
            Self::GnomeTerminal => "gnome-terminal",
            Self::Konsole => "konsole",
            Self::XfceTerminal => "xfce4-terminal",
            Self::Xterm => "xterm",
            Self::Kitty => "kitty",
            Self::Alacritty => "alacritty",
            Self::WezTerm => "wezterm",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Terminal => "Terminal.app",
            Self::Iterm2 => "iTerm2",
            Self::PowerShell => "PowerShell",
            Self::PowerShell7 => "PowerShell 7",
            Self::WindowsTerminalPowerShell => "Windows Terminal / PowerShell",
            Self::WindowsTerminalPowerShell7 => "Windows Terminal / PowerShell 7",
            Self::SystemTerminal => "System terminal",
            Self::GnomeTerminal => "GNOME Terminal",
            Self::Konsole => "Konsole",
            Self::XfceTerminal => "XFCE Terminal",
            Self::Xterm => "xterm",
            Self::Kitty => "Kitty",
            Self::Alacritty => "Alacritty",
            Self::WezTerm => "WezTerm",
        }
    }

    pub fn from_id(s: &str) -> Option<Self> {
        match s {
            "terminal" => Some(Self::Terminal),
            "iterm2" => Some(Self::Iterm2),
            "powershell" => Some(Self::PowerShell),
            "powershell-7" => Some(Self::PowerShell7),
            "windows-terminal-powershell" => Some(Self::WindowsTerminalPowerShell),
            "windows-terminal-powershell-7" => Some(Self::WindowsTerminalPowerShell7),
            "system-terminal" => Some(Self::SystemTerminal),
            "gnome-terminal" => Some(Self::GnomeTerminal),
            "konsole" => Some(Self::Konsole),
            "xfce4-terminal" => Some(Self::XfceTerminal),
            "xterm" => Some(Self::Xterm),
            "kitty" => Some(Self::Kitty),
            "alacritty" => Some(Self::Alacritty),
            "wezterm" => Some(Self::WezTerm),
            _ => None,
        }
    }

    pub fn default_for_platform() -> Self {
        #[cfg(target_os = "windows")]
        {
            Self::PowerShell
        }
        #[cfg(target_os = "macos")]
        {
            Self::Terminal
        }
        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        {
            Self::SystemTerminal
        }
    }
}

// ---------------------------------------------------------------------------
// Detection
// ---------------------------------------------------------------------------

/// Probe the filesystem for which of the supported terminal apps the user
/// actually has installed. Order matches `TerminalChoice::ALL` so the UI
/// can render a stable list.
pub fn detect_installed() -> Vec<TerminalChoice> {
    let mut out = Vec::new();
    for choice in TerminalChoice::ALL {
        if is_available(*choice) {
            out.push(*choice);
        }
    }
    out
}

pub fn is_available(choice: TerminalChoice) -> bool {
    TerminalChoice::ALL.contains(&choice) && is_installed(choice)
}

pub fn default_available_for_platform() -> TerminalChoice {
    TerminalChoice::ALL
        .iter()
        .copied()
        .find(|choice| is_installed(*choice))
        .unwrap_or_else(TerminalChoice::default_for_platform)
}

fn is_installed(choice: TerminalChoice) -> bool {
    match choice {
        // Terminal.app ships with macOS; assume present.
        TerminalChoice::Terminal => cfg!(target_os = "macos"),
        TerminalChoice::Iterm2 => va_launcher::iterm2_installed(),
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
    let Ok(metadata) = std::fs::metadata(path) else {
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

// ---------------------------------------------------------------------------
// Preference file I/O
// ---------------------------------------------------------------------------

#[derive(Debug, Default, Clone, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
struct LauncherPrefsFile {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    terminal: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    compatibility_bridge: Option<CompatibilityBridgeMode>,
}

/// Read the preferred terminal, falling back to the platform default.
pub fn read_preference() -> TerminalChoice {
    read_prefs_file()
        .terminal
        .as_deref()
        .and_then(TerminalChoice::from_id)
        .filter(|choice| is_available(*choice))
        .unwrap_or_else(default_available_for_platform)
}

pub fn write_preference(choice: TerminalChoice) -> anyhow::Result<()> {
    update_prefs_file(|prefs| {
        prefs.terminal = Some(choice.id().to_string());
    })
}

pub fn read_compatibility_bridge_preference() -> CompatibilityBridgeMode {
    read_prefs_file()
        .compatibility_bridge
        .unwrap_or(CompatibilityBridgeMode::Auto)
}

pub fn write_compatibility_bridge_preference(mode: CompatibilityBridgeMode) -> anyhow::Result<()> {
    update_prefs_file(|prefs| {
        prefs.compatibility_bridge = Some(mode);
    })
}

pub fn canonical_workspace_path(path: &std::path::Path) -> anyhow::Result<PathBuf> {
    let canonical = std::fs::canonicalize(path)
        .with_context(|| format!("workspace does not exist: {}", path.display()))?;
    if !canonical.is_dir() {
        anyhow::bail!("workspace is not a directory: {}", canonical.display());
    }
    Ok(strip_windows_unc_prefix(canonical))
}

/// Strip the `\\?\` extended-length path prefix that `std::fs::canonicalize`
/// adds on Windows.  CMD and many tools choke on it.
fn strip_windows_unc_prefix(p: PathBuf) -> PathBuf {
    #[cfg(target_os = "windows")]
    {
        let s = p.to_string_lossy();
        if let Some(rest) = s.strip_prefix(r"\\?\") {
            // Only strip if it's a regular drive path (e.g. \\?\D:\...).
            // True UNC shares like \\?\UNC\server\share must keep the prefix.
            if rest.len() >= 2 && rest.as_bytes()[1] == b':' {
                return PathBuf::from(rest.to_string());
            }
        }
        p
    }
    #[cfg(not(target_os = "windows"))]
    {
        p
    }
}

fn read_prefs_file() -> LauncherPrefsFile {
    config::read_settings_json()
        .ok()
        .map(|root| prefs_from_settings(&root))
        .unwrap_or_default()
}

fn prefs_from_settings(root: &Value) -> LauncherPrefsFile {
    root.get("launcher")
        .cloned()
        .and_then(
            |launcher| match serde_json::from_value::<LauncherPrefsFile>(launcher) {
                Ok(prefs) => Some(prefs),
                Err(error) => {
                    tracing::warn!(
                        "[launcher] settings.json launcher prefs parse error: {} - using default",
                        error
                    );
                    None
                }
            },
        )
        .unwrap_or_default()
}

fn update_prefs_file(f: impl FnOnce(&mut LauncherPrefsFile)) -> anyhow::Result<()> {
    config::mutate_settings_json(|root| update_prefs_in_settings(root, f))
        .map_err(anyhow::Error::msg)
}

fn update_prefs_in_settings(
    root: &mut Value,
    f: impl FnOnce(&mut LauncherPrefsFile),
) -> Result<(), String> {
    let mut prefs = prefs_from_settings(root);
    f(&mut prefs);
    let value = serde_json::to_value(prefs).map_err(|error| error.to_string())?;
    let prefs_obj = value.as_object().cloned().unwrap_or_default();
    let root_obj = root
        .as_object_mut()
        .ok_or_else(|| "settings.json root must be a JSON object".to_string())?;
    let launcher = root_obj
        .entry("launcher".to_string())
        .or_insert_with(|| Value::Object(Map::new()));
    if !launcher.is_object() {
        *launcher = Value::Object(Map::new());
    }
    let launcher_obj = launcher
        .as_object_mut()
        .ok_or_else(|| "settings.json launcher must be a JSON object".to_string())?;
    merge_pref_field(launcher_obj, &prefs_obj, "terminal");
    merge_pref_field(launcher_obj, &prefs_obj, "compatibility_bridge");
    if launcher_obj.is_empty() {
        root_obj.remove("launcher");
    }
    Ok(())
}

fn merge_pref_field(launcher: &mut Map<String, Value>, prefs: &Map<String, Value>, key: &str) {
    if let Some(value) = prefs.get(key) {
        launcher.insert(key.to_string(), value.clone());
    } else {
        launcher.remove(key);
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn id_roundtrip() {
        for c in TerminalChoice::ALL {
            assert_eq!(TerminalChoice::from_id(c.id()), Some(*c));
        }
    }

    #[test]
    fn unknown_id_is_none() {
        assert!(TerminalChoice::from_id("warp").is_none());
        assert!(TerminalChoice::from_id("").is_none());
    }

    #[test]
    fn compatibility_bridge_mode_ids_roundtrip() {
        for mode in CompatibilityBridgeMode::ALL {
            assert_eq!(CompatibilityBridgeMode::from_id(mode.id()), Some(*mode));
        }
    }

    #[test]
    fn preference_update_uses_latest_settings_value() {
        let mut settings = serde_json::json!({
            "launcher": {
                "terminal": "terminal",
                "selected_agent": "codex"
            }
        });

        update_prefs_in_settings(&mut settings, |prefs| {
            prefs.compatibility_bridge = Some(CompatibilityBridgeMode::On);
        })
        .unwrap();

        assert_eq!(settings["launcher"]["terminal"], "terminal");
        assert_eq!(settings["launcher"]["selected_agent"], "codex");
        assert_eq!(settings["launcher"]["compatibility_bridge"], "on");
    }
}
