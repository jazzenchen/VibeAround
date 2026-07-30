use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::Context;
use serde::{Deserialize, Serialize};

const DESKTOP_DETECTION_SCHEMA_VERSION: u32 = 2;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DesktopAppDetectionFile {
    pub schema_version: u32,
    pub platform: String,
    pub scanned_at_unix_ms: u128,
    pub apps: BTreeMap<String, DesktopAppDetection>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DesktopAppDetection {
    pub installed: bool,
    pub launch_command: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub entry: Option<DesktopAppEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DesktopAppEntry {
    pub app_name: String,
    pub path: String,
    pub source: String,
    pub source_label: String,
}

pub fn detected_desktop_apps_path() -> PathBuf {
    common::config::data_dir().join("desktop-apps.detected.json")
}

pub fn read_detected_desktop_apps() -> Option<DesktopAppDetectionFile> {
    let path = detected_desktop_apps_path();
    let contents = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&contents)
        .ok()
        .filter(is_compatible_detection_cache)
}

fn is_compatible_detection_cache(detected: &DesktopAppDetectionFile) -> bool {
    detected.schema_version == DESKTOP_DETECTION_SCHEMA_VERSION
        && detected.platform == current_platform()
}

pub fn refresh_known_agent_and_persist(
    agent_id: &str,
) -> anyhow::Result<Option<DesktopAppDetection>> {
    let Some(agent) = common::resources::agent_by_id(agent_id) else {
        return Ok(None);
    };
    if !agent.direct_only || !agent.supports_current_platform() {
        return Ok(None);
    }

    let launch_command = agent.pty_command_for_current_platform().to_string();
    let Some(app_name) = desktop_app_name(&launch_command) else {
        return Ok(None);
    };
    let Some((path, source, source_label)) = known_desktop_app_entry(&app_name) else {
        return Ok(None);
    };

    let detection = DesktopAppDetection {
        installed: true,
        launch_command,
        entry: Some(DesktopAppEntry {
            app_name,
            path,
            source,
            source_label,
        }),
    };
    let mut detected = read_detected_desktop_apps().unwrap_or_else(|| DesktopAppDetectionFile {
        schema_version: DESKTOP_DETECTION_SCHEMA_VERSION,
        platform: current_platform().to_string(),
        scanned_at_unix_ms: now_unix_ms(),
        apps: BTreeMap::new(),
    });
    detected.schema_version = DESKTOP_DETECTION_SCHEMA_VERSION;
    detected.platform = current_platform().to_string();
    detected.scanned_at_unix_ms = now_unix_ms();
    detected.apps.insert(agent.id.clone(), detection.clone());
    write_detected_desktop_apps(&detected)?;
    Ok(Some(detection))
}

pub async fn scan_and_persist() -> anyhow::Result<DesktopAppDetectionFile> {
    let detected = scan_desktop_apps().await;
    write_detected_desktop_apps(&detected)?;
    Ok(detected)
}

pub async fn scan_desktop_apps() -> DesktopAppDetectionFile {
    let mut apps = BTreeMap::new();
    for agent in common::resources::AGENTS.iter() {
        if !agent.direct_only || !agent.supports_current_platform() {
            continue;
        }
        let launch_command = agent.pty_command_for_current_platform().to_string();
        let entry = match desktop_app_name(&launch_command) {
            Some(app_name) => desktop_app_entry(&app_name).await,
            None => None,
        };
        apps.insert(
            agent.id.clone(),
            DesktopAppDetection {
                installed: entry.is_some(),
                launch_command,
                entry,
            },
        );
    }
    DesktopAppDetectionFile {
        schema_version: DESKTOP_DETECTION_SCHEMA_VERSION,
        platform: current_platform().to_string(),
        scanned_at_unix_ms: now_unix_ms(),
        apps,
    }
}

async fn desktop_app_entry(app_name: &str) -> Option<DesktopAppEntry> {
    let (path, source, source_label) = if cfg!(target_os = "macos") {
        macos_application_entry(app_name).await?
    } else if cfg!(windows) {
        windows_application_entry(app_name).await?
    } else {
        return None;
    };
    Some(DesktopAppEntry {
        app_name: app_name.to_string(),
        path,
        source,
        source_label,
    })
}

fn desktop_app_name(command: &str) -> Option<String> {
    let command = command.trim();
    if cfg!(target_os = "macos") {
        if command
            .strip_prefix("open -b ")
            .map(str::trim)
            .map(|bundle_id| bundle_id.trim_matches('"'))
            == Some(common::resources::CHATGPT_DESKTOP_MACOS_BUNDLE_ID)
        {
            return Some(common::resources::CHATGPT_DESKTOP_MACOS_APP_NAME.to_string());
        }
        return command
            .strip_prefix("open -a ")
            .map(str::trim)
            .filter(|name| !name.is_empty())
            .map(|name| name.trim_matches('"').to_string());
    }
    if cfg!(windows) {
        return command
            .strip_prefix("Start-Process ")
            .map(str::trim)
            .filter(|name| !name.is_empty())
            .map(|name| name.trim_matches('"').to_string());
    }
    None
}

fn known_desktop_app_entry(app_name: &str) -> Option<(String, String, String)> {
    if cfg!(target_os = "macos") {
        let path = macos_application_candidate_paths(app_name)
            .into_iter()
            .find(|path| path.is_dir())?;
        return Some((
            normalize_app_path(&path),
            "macos_applications_dir".to_string(),
            "macOS Applications directory".to_string(),
        ));
    }
    if cfg!(windows) {
        let path = windows_application_candidate_paths(app_name)
            .into_iter()
            .find(|path| path.is_file())?;
        return Some((
            path.to_string_lossy().to_string(),
            "windows_known_location".to_string(),
            "Windows known location".to_string(),
        ));
    }
    None
}

async fn macos_application_entry(app_name: &str) -> Option<(String, String, String)> {
    for path in macos_application_candidate_paths(app_name) {
        if path.is_dir() {
            return Some((
                normalize_app_path(&path),
                "macos_applications_dir".to_string(),
                "macOS Applications directory".to_string(),
            ));
        }
    }

    let query = macos_spotlight_query(app_name);
    let path = command_stdout_line("mdfind", &[&query])
        .await
        .filter(|path| path.ends_with(".app"))?;
    Some((
        path.trim_end_matches('/').to_string(),
        "macos_spotlight".to_string(),
        "macOS Spotlight".to_string(),
    ))
}

async fn windows_application_entry(app_name: &str) -> Option<(String, String, String)> {
    if let Some(path) = windows_application_candidate_paths(app_name)
        .into_iter()
        .find(|path| path.is_file())
    {
        return Some((
            path.to_string_lossy().to_string(),
            "windows_known_location".to_string(),
            "Windows known location".to_string(),
        ));
    }

    if let Some(app_id) = windows_start_app_id(app_name).await {
        return Some((
            app_id,
            "windows_start_apps".to_string(),
            "Windows Start Apps".to_string(),
        ));
    }

    None
}

fn windows_application_candidate_paths(app_name: &str) -> Vec<PathBuf> {
    let Some(localappdata) = std::env::var_os("LOCALAPPDATA").map(PathBuf::from) else {
        return Vec::new();
    };
    if app_name.eq_ignore_ascii_case("Claude") {
        return claude_windows_candidate_paths(&localappdata);
    }
    if app_name.eq_ignore_ascii_case("Codex") {
        return codex_windows_candidate_paths(&localappdata);
    }
    Vec::new()
}

fn claude_windows_candidate_paths(localappdata: &Path) -> Vec<PathBuf> {
    let mut paths = vec![
        localappdata
            .join("Programs")
            .join("Claude")
            .join("Claude.exe"),
        localappdata
            .join("Anthropic")
            .join("Claude")
            .join("Claude.exe"),
        localappdata.join("Claude").join("Claude.exe"),
    ];
    paths.extend(versioned_child_exe_paths(
        &localappdata.join("AnthropicClaude"),
        "Claude.exe",
    ));
    paths
}

fn codex_windows_candidate_paths(localappdata: &Path) -> Vec<PathBuf> {
    vec![
        localappdata
            .join("Programs")
            .join("Codex")
            .join("Codex.exe"),
        localappdata.join("OpenAI").join("Codex").join("Codex.exe"),
    ]
}

fn versioned_child_exe_paths(parent: &Path, exe_name: &str) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(parent) else {
        return Vec::new();
    };
    let mut paths = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.is_dir())
        .map(|path| path.join(exe_name))
        .collect::<Vec<_>>();
    paths.sort();
    paths.reverse();
    paths
}

fn macos_application_candidate_paths(app_name: &str) -> Vec<PathBuf> {
    let bundle_names =
        if app_name.eq_ignore_ascii_case(common::resources::CHATGPT_DESKTOP_MACOS_APP_NAME) {
            vec!["ChatGPT.app".to_string(), "Codex.app".to_string()]
        } else {
            vec![macos_app_bundle_name(app_name)]
        };
    let mut roots = vec![
        PathBuf::from("/Applications"),
        PathBuf::from("/Applications/Utilities"),
        PathBuf::from("/System/Applications"),
        PathBuf::from("/System/Applications/Utilities"),
    ];
    if let Some(home) = std::env::var_os("HOME") {
        roots.insert(1, PathBuf::from(home).join("Applications"));
    }
    bundle_names
        .into_iter()
        .flat_map(|bundle_name| roots.iter().map(move |root| root.join(&bundle_name)))
        .collect()
}

fn macos_spotlight_query(app_name: &str) -> String {
    if app_name.eq_ignore_ascii_case(common::resources::CHATGPT_DESKTOP_MACOS_APP_NAME) {
        return format!(
            "kMDItemContentTypeTree == 'com.apple.application-bundle' && kMDItemCFBundleIdentifier == {}",
            mdfind_string(common::resources::CHATGPT_DESKTOP_MACOS_BUNDLE_ID)
        );
    }
    format!(
        "kMDItemContentTypeTree == 'com.apple.application-bundle' && kMDItemFSName == {}",
        mdfind_string(&macos_app_bundle_name(app_name))
    )
}

fn macos_app_bundle_name(app_name: &str) -> String {
    let app_name = app_name.trim().trim_matches('"').trim_end_matches('/');
    if app_name.ends_with(".app") {
        app_name.to_string()
    } else {
        format!("{app_name}.app")
    }
}

fn normalize_app_path(path: &Path) -> String {
    path.to_string_lossy().trim_end_matches('/').to_string()
}

async fn windows_start_app_id(app_name: &str) -> Option<String> {
    let script = if app_name.eq_ignore_ascii_case("Codex") {
        common::resources::chatgpt_desktop_windows_start_app_query()
    } else {
        format!(
            "$app = Get-StartApps -Name {} | Select-Object -First 1; if ($app) {{ $app.AppID }}",
            powershell_string(app_name)
        )
    };
    command_stdout_line("powershell.exe", &["-NoProfile", "-Command", &script]).await
}

async fn command_stdout_line(command: &str, args: &[&str]) -> Option<String> {
    let output = tokio::time::timeout(
        Duration::from_secs(6),
        common::process::env::command(command).args(args).output(),
    )
    .await
    .ok()?
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

fn write_detected_desktop_apps(detected: &DesktopAppDetectionFile) -> anyhow::Result<()> {
    let path = detected_desktop_apps_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("create detected desktop apps dir {:?}", parent))?;
    }
    let json = serde_json::to_string_pretty(detected).context("serialize desktop app detection")?;
    std::fs::write(&path, json).with_context(|| format!("write {:?}", path))?;
    Ok(())
}

fn powershell_string(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

fn mdfind_string(value: &str) -> String {
    format!("'{}'", value.replace('\'', "\\'"))
}

fn current_platform() -> &'static str {
    if cfg!(target_os = "macos") {
        "macos"
    } else if cfg!(target_os = "windows") {
        "windows"
    } else {
        "linux"
    }
}

fn now_unix_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn detection_cache(schema_version: u32, platform: &str) -> DesktopAppDetectionFile {
        DesktopAppDetectionFile {
            schema_version,
            platform: platform.to_string(),
            scanned_at_unix_ms: 0,
            apps: BTreeMap::new(),
        }
    }

    #[test]
    fn desktop_detection_cache_requires_current_schema_and_platform() {
        assert!(is_compatible_detection_cache(&detection_cache(
            DESKTOP_DETECTION_SCHEMA_VERSION,
            current_platform()
        )));
        assert!(!is_compatible_detection_cache(&detection_cache(
            DESKTOP_DETECTION_SCHEMA_VERSION - 1,
            current_platform()
        )));
        assert!(!is_compatible_detection_cache(&detection_cache(
            DESKTOP_DETECTION_SCHEMA_VERSION,
            "another-platform"
        )));
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn macos_desktop_app_name_parses_open_command() {
        assert_eq!(
            desktop_app_name("open -a Claude").as_deref(),
            Some("Claude")
        );
        assert_eq!(
            desktop_app_name("open -b com.openai.codex").as_deref(),
            Some("ChatGPT")
        );
    }

    #[test]
    fn macos_chatgpt_candidates_include_current_and_legacy_app_names() {
        let paths = macos_application_candidate_paths("ChatGPT");
        assert!(paths.iter().any(|path| path.ends_with("ChatGPT.app")));
        assert!(paths.iter().any(|path| path.ends_with("Codex.app")));
    }

    #[test]
    fn macos_chatgpt_spotlight_query_uses_bundle_id() {
        let query = macos_spotlight_query("ChatGPT");
        assert!(query.contains("kMDItemCFBundleIdentifier == 'com.openai.codex'"));
        assert!(!query.contains("kMDItemFSName"));
    }

    #[test]
    fn macos_app_bundle_name_adds_app_suffix_once() {
        assert_eq!(macos_app_bundle_name("Claude"), "Claude.app");
        assert_eq!(macos_app_bundle_name("Codex.app"), "Codex.app");
    }

    #[test]
    fn mdfind_string_quotes_single_quotes() {
        assert_eq!(mdfind_string("Dev's App.app"), "'Dev\\'s App.app'");
    }

    #[test]
    fn windows_codex_candidates_do_not_include_cli_bin_children() {
        let root = PathBuf::from(r"C:\Users\tester\AppData\Local");
        assert!(!codex_windows_candidate_paths(&root)
            .iter()
            .any(|path| windows_path_string(path).contains(r"OpenAI\Codex\bin")));
    }

    #[test]
    fn windows_claude_candidates_include_squirrel_root() {
        let root = PathBuf::from(r"C:\Users\tester\AppData\Local");
        assert!(claude_windows_candidate_paths(&root).iter().any(|path| {
            windows_path_string(path) == r"C:\Users\tester\AppData\Local\Programs\Claude\Claude.exe"
        }));
    }

    fn windows_path_string(path: &Path) -> String {
        path.to_string_lossy().replace('/', r"\")
    }
}
