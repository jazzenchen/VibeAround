//! Onboarding: first-run setup wizard.
//! Checks whether settings.json has `"onboarded": true`; exposes Tauri IPC
//! commands so the desktop-ui frontend can read/write settings and signal completion.

use std::path::PathBuf;
use std::sync::Arc;

use serde_json::Value;
use tauri::{AppHandle, Emitter, Manager, Runtime};
use tokio::sync::Notify;

use common::config;

/// Shared state injected into Tauri so the `finish_onboarding` command can
/// unblock the daemon-start future that is waiting in `setup`.
pub struct OnboardingGate {
    pub notify: Arc<Notify>,
}

// ---------------------------------------------------------------------------
// helpers
// ---------------------------------------------------------------------------

fn settings_path() -> PathBuf {
    config::data_dir().join("settings.json")
}

fn read_settings_value() -> Value {
    let path = settings_path();
    std::fs::read_to_string(&path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_else(|| serde_json::json!({}))
}

fn write_settings_value(val: &Value) -> Result<(), String> {
    let path = settings_path();
    // Ensure parent dir exists
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let pretty = serde_json::to_string_pretty(val).map_err(|e| e.to_string())?;
    std::fs::write(&path, pretty).map_err(|e| e.to_string())
}

// ---------------------------------------------------------------------------
// public API
// ---------------------------------------------------------------------------

/// Returns `true` when the user has NOT completed onboarding yet.
pub fn needs_onboarding() -> bool {
    let val = read_settings_value();
    !val.get("onboarded")
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
}

// ---------------------------------------------------------------------------
// Tauri IPC commands
// ---------------------------------------------------------------------------

/// Return the current settings.json as a JSON value.
#[tauri::command]
pub fn get_settings() -> Result<Value, String> {
    Ok(read_settings_value())
}

/// Merge the provided partial JSON into settings.json and write it back.
/// The frontend sends the full desired config; we overwrite.
#[tauri::command]
pub fn save_settings(settings: Value) -> Result<(), String> {
    write_settings_value(&settings)
}

/// Mark onboarding complete: set `"onboarded": true`, write settings,
/// then notify the daemon-start future so the server boots up.
/// Also emits `onboarding-complete` event so tray can re-enable menu items.
#[tauri::command]
pub fn finish_onboarding<R: Runtime>(app: AppHandle<R>, settings: Value) -> Result<(), String> {
    // Merge onboarded flag
    let mut val = settings;
    if let Some(obj) = val.as_object_mut() {
        obj.insert("onboarded".into(), serde_json::json!(true));
    }
    write_settings_value(&val)?;

    // Emit event for tray menu to re-enable items
    let _ = app.emit("onboarding-complete", ());

    // Unblock the daemon-start future
    if let Some(gate) = app.try_state::<OnboardingGate>() {
        gate.notify.notify_one();
    }

    Ok(())
}
