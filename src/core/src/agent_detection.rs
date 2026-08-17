use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::Context;
use serde::{Deserialize, Serialize};
use tokio::process::Command;
use tokio::task::JoinSet;

const AGENT_SOURCES_TOML: &str = include_str!("../../resources/agent-sources.toml");
const DETECTION_SCHEMA_VERSION: u32 = 2;
const VERSION_CHECK_TIMEOUT: Duration = Duration::from_secs(30);
const LATEST_VERSION_CHECK_TIMEOUT: Duration = Duration::from_secs(8);
const USER_SHELL_RANK_BASE: u32 = 0;
const HOMEBREW_RANK_BASE: u32 = 1_000;
const NPM_GLOBAL_RANK_BASE: u32 = 2_000;
const NATIVE_RANK_BASE: u32 = 3_000;
const GENERIC_PATH_RANK_BASE: u32 = 4_000;
const MANAGED_RANK_BASE: u32 = 10_000;
const APP_BUNDLED_RANK_BASE: u32 = 20_000;

#[derive(Debug, Clone, Default)]
struct PackageManagerCandidateDirs {
    dirs: Vec<PathBuf>,
}

impl PackageManagerCandidateDirs {
    fn candidate_paths(&self, program: &str) -> Vec<PathBuf> {
        let paths = self
            .dirs
            .iter()
            .flat_map(|dir| program_candidates_in_dir(dir.clone(), program))
            .collect();
        dedupe_paths(paths)
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct AgentSourceCatalog {
    #[serde(default)]
    pub agents: BTreeMap<String, AgentCommandSpec>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AgentCommandSpec {
    pub program: String,
    #[serde(default = "default_version_arg")]
    pub version_arg: String,
    #[serde(default)]
    pub sources: BTreeMap<String, AgentSourceSpec>,
}

fn default_version_arg() -> String {
    "--version".to_string()
}

#[derive(Debug, Clone, Deserialize)]
pub struct AgentSourceSpec {
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default)]
    pub package: Option<String>,
    #[serde(default)]
    pub install: PlatformCommand,
    #[serde(default)]
    pub upgrade: PlatformCommand,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct PlatformCommand {
    #[serde(default)]
    pub macos: Option<String>,
    #[serde(default)]
    pub linux: Option<String>,
    #[serde(default)]
    pub windows: Option<String>,
}

impl PlatformCommand {
    pub fn for_current_platform(&self) -> Option<&str> {
        match current_platform() {
            "macos" => self.macos.as_deref(),
            "linux" => self.linux.as_deref(),
            "windows" => self.windows.as_deref(),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentDetectionFile {
    pub schema_version: u32,
    pub platform: String,
    pub scanned_at_unix_ms: u128,
    pub agents: BTreeMap<String, AgentDetection>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentDetection {
    #[serde(rename = "default", default)]
    pub default_candidate: Option<AgentCandidate>,
    #[serde(default)]
    pub system_selected: Option<AgentCandidate>,
    #[serde(default)]
    pub candidates: Vec<AgentCandidate>,
}

impl AgentDetection {
    pub fn system_selected_candidate(&self) -> Option<AgentCandidate> {
        self.system_selected
            .clone()
            .filter(is_system_toolchain_candidate)
    }

    pub fn managed_selected_candidate(&self) -> Option<AgentCandidate> {
        self.candidates
            .iter()
            .find(|candidate| candidate.source == "npm_managed")
            .cloned()
    }
}

fn is_system_toolchain_candidate(candidate: &AgentCandidate) -> bool {
    !matches!(candidate.source.as_str(), "npm_managed" | "app_bundled")
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentCandidate {
    pub path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub realpath: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    pub source: String,
    pub source_label: String,
    pub rank: u32,
    pub is_user_default: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub package: Option<String>,
}

pub fn source_catalog() -> anyhow::Result<AgentSourceCatalog> {
    toml::from_str(AGENT_SOURCES_TOML).context("parse agent-sources.toml")
}

pub fn detected_agents_path() -> PathBuf {
    crate::config::data_dir().join("agents.detected.json")
}

pub fn read_detected_agents() -> Option<AgentDetectionFile> {
    let path = detected_agents_path();
    let contents = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&contents)
        .ok()
        .filter(is_compatible_detection_cache)
}

fn is_compatible_detection_cache(detected: &AgentDetectionFile) -> bool {
    detected.schema_version == DETECTION_SCHEMA_VERSION && detected.platform == current_platform()
}

pub fn configured_candidate(agent_id: &str) -> Option<AgentCandidate> {
    let prefs = crate::agent_state::read_prefs();
    let configured = crate::agent_state::resolve_agent_executable(&prefs, agent_id)?;
    let path = configured.path.clone();
    if !path.is_file() {
        return None;
    }
    configured_candidate_from_detection(agent_id, &path)
        .or_else(|| Some(candidate_from_executable_preference(configured)))
}

pub async fn configured_candidate_with_version(agent_id: &str) -> Option<AgentCandidate> {
    let prefs = crate::agent_state::read_prefs();
    let configured = crate::agent_state::resolve_agent_executable(&prefs, agent_id)?;
    let path = configured.path.clone();
    if !path.is_file() {
        return None;
    }
    if let Some(candidate) = configured_candidate_from_detection(agent_id, &path) {
        return Some(candidate);
    }
    let mut candidate = candidate_from_executable_preference(configured);
    if candidate.version.is_none() {
        let catalog = source_catalog().ok()?;
        let spec = agent_command_spec(&catalog, agent_id).ok()?;
        candidate.version = command_version(Path::new(&candidate.path), &spec.version_arg).await;
    }
    Some(candidate)
}

pub async fn manual_candidate_with_version(
    agent_id: &str,
    path: PathBuf,
) -> anyhow::Result<AgentCandidate> {
    if !path.is_file() {
        anyhow::bail!("executable path is not a file: {}", path.display());
    }
    let catalog = source_catalog()?;
    let spec = agent_command_spec(&catalog, agent_id)?;
    let mut candidate = manual_candidate_from_path(path, "Manual path".to_string());
    candidate.version = command_version(Path::new(&candidate.path), &spec.version_arg).await;
    Ok(candidate)
}

pub fn selected_candidate_for_mode(agent_id: &str, toolchain_mode: &str) -> Option<AgentCandidate> {
    crate::agent_availability::resolve_cached_agent_availability(
        agent_id,
        toolchain_mode,
        crate::agent_availability::AgentCandidatePreference::ToolchainMode,
    )
    .selected
}

pub fn selected_candidate(agent_id: &str) -> Option<AgentCandidate> {
    let mode = crate::config::ensure_loaded().toolchain_mode.as_str();
    selected_candidate_for_mode(agent_id, mode)
}

pub fn resolve_agent_command_for_mode(
    agent_id: &str,
    fallback_command: &str,
    toolchain_mode: &str,
) -> String {
    resolve_agent_command_for_mode_strict(agent_id, fallback_command, toolchain_mode)
        .unwrap_or_else(|_| fallback_command.to_string())
}

pub fn resolve_agent_command_for_mode_strict(
    agent_id: &str,
    fallback_command: &str,
    toolchain_mode: &str,
) -> anyhow::Result<String> {
    if crate::resources::agent_by_id(agent_id)
        .map(|agent| agent.direct_only)
        .unwrap_or(false)
    {
        return Ok(fallback_command.to_string());
    }

    if let Some(candidate) = selected_candidate_for_mode(agent_id, toolchain_mode) {
        return Ok(replace_command_program(fallback_command, &candidate.path));
    }

    if is_managed_toolchain_mode(toolchain_mode) {
        anyhow::bail!("{}", managed_agent_missing_message(agent_id));
    }

    Ok(fallback_command.to_string())
}

pub fn resolve_agent_command_strict(
    agent_id: &str,
    fallback_command: &str,
) -> anyhow::Result<String> {
    let mode = crate::config::ensure_loaded().toolchain_mode.as_str();
    resolve_agent_command_for_mode_strict(agent_id, fallback_command, mode)
}

pub fn preferred_startkit_candidate(
    agent_id: &str,
    detection: &AgentDetection,
    toolchain_mode: &str,
) -> Option<AgentCandidate> {
    if is_managed_toolchain_mode(toolchain_mode) {
        return agent_uses_npm_install(agent_id)
            .then(|| detection.managed_selected_candidate())
            .flatten();
    }
    detection.system_selected_candidate()
}

pub fn managed_agent_missing_message(agent_id: &str) -> String {
    let label = crate::resources::agent_by_id(agent_id)
        .map(|agent| agent.display_name.as_str())
        .unwrap_or(agent_id);
    format!(
        "{label} is not installed in the VibeAround managed toolchain. Install it with Startkit, or switch Settings > General > Agent Toolchain to System to use a local installation."
    )
}

fn is_managed_toolchain_mode(toolchain_mode: &str) -> bool {
    toolchain_mode.trim().eq_ignore_ascii_case("managed")
}

fn manual_candidate_from_path(path: PathBuf, source_label: String) -> AgentCandidate {
    AgentCandidate {
        path: path.to_string_lossy().to_string(),
        realpath: canonicalize_to_string(&path),
        version: None,
        source: "manual_path".to_string(),
        source_label,
        rank: 0,
        is_user_default: true,
        package: None,
    }
}

pub fn executable_preference_from_candidate_path(
    candidate: &AgentCandidate,
    path: &Path,
) -> crate::agent_state::AgentExecutablePreference {
    crate::agent_state::AgentExecutablePreference {
        path: path.to_path_buf(),
        realpath: candidate.realpath.as_ref().map(PathBuf::from),
        version: candidate.version.clone(),
        source: candidate.source.clone(),
        source_label: candidate.source_label.clone(),
        rank: candidate.rank,
        package: candidate.package.clone(),
    }
}

fn candidate_from_executable_preference(
    executable: crate::agent_state::AgentExecutablePreference,
) -> AgentCandidate {
    AgentCandidate {
        path: executable.path.to_string_lossy().to_string(),
        realpath: executable
            .realpath
            .map(|path| path.to_string_lossy().to_string())
            .or_else(|| canonicalize_to_string(&executable.path)),
        version: executable.version,
        source: executable.source,
        source_label: executable.source_label,
        rank: executable.rank,
        is_user_default: true,
        package: executable.package,
    }
}

pub fn candidate_for_path(detection: &AgentDetection, path: &Path) -> Option<AgentCandidate> {
    let configured_key = normalize_path_key(path);
    detection
        .candidates
        .iter()
        .find(|candidate| candidate_key(candidate) == configured_key)
        .cloned()
}

fn configured_candidate_from_detection(agent_id: &str, path: &Path) -> Option<AgentCandidate> {
    let detected = read_detected_agents()?;
    let detection = detected.agents.get(agent_id)?;
    candidate_for_path(detection, path)
}

fn candidate_key(candidate: &AgentCandidate) -> String {
    candidate
        .realpath
        .clone()
        .unwrap_or_else(|| candidate.path.clone())
}

pub fn agent_uses_npm_install(agent_id: &str) -> bool {
    source_package(agent_id, "npm_global").is_some()
}

pub fn source_command_template(agent_id: &str, source: &str, action: &str) -> Option<String> {
    let catalog = source_catalog().ok()?;
    let spec = catalog.agents.get(agent_id)?;
    let source = spec.sources.get(source)?;
    let command = match action {
        "install" => &source.install,
        "upgrade" => &source.upgrade,
        _ => return None,
    };
    command.for_current_platform().map(str::to_string)
}

pub fn source_package(agent_id: &str, source: &str) -> Option<String> {
    let catalog = source_catalog().ok()?;
    let spec = catalog.agents.get(agent_id)?;
    spec.sources
        .get(source)
        .and_then(|source_spec| source_spec.package.clone())
        .or_else(|| {
            spec.sources
                .values()
                .find_map(|source_spec| source_spec.package.clone())
        })
}

pub async fn latest_version_for_candidate(
    agent_id: &str,
    candidate: &AgentCandidate,
) -> Option<String> {
    if matches!(candidate.source.as_str(), "manual_path" | "app_bundled") {
        return None;
    }

    if matches!(
        candidate.source.as_str(),
        "homebrew_formula" | "homebrew_cask"
    ) {
        let is_cask = candidate.source == "homebrew_cask";
        let package = homebrew_package_name(agent_id, &candidate.source)?;
        return homebrew_latest_version(&package, is_cask).await;
    }

    let package = candidate
        .package
        .clone()
        .or_else(|| source_package(agent_id, &candidate.source))?;
    npm_latest_version(&package).await
}

pub fn candidate_update_available(
    candidate: &AgentCandidate,
    latest_version: Option<&str>,
) -> Option<bool> {
    let current = comparable_version(candidate.version.as_deref()?)?;
    let latest = comparable_version(latest_version?)?;
    Some(current != latest)
}

pub async fn scan_and_persist() -> anyhow::Result<AgentDetectionFile> {
    let catalog = source_catalog()?;
    let detected = scan_agents(&catalog).await?;
    write_detected_agents(&detected)?;
    Ok(detected)
}

pub async fn scan_agent_and_persist(agent_id: &str) -> anyhow::Result<AgentDetection> {
    let catalog = source_catalog()?;
    let spec = agent_command_spec(&catalog, agent_id)?;
    let package_manager_dirs = package_manager_candidate_dirs().await;
    let detection = scan_agent(agent_id, &spec, &package_manager_dirs).await;
    let mut detected = read_detected_agents().unwrap_or_else(|| AgentDetectionFile {
        schema_version: DETECTION_SCHEMA_VERSION,
        platform: current_platform().to_string(),
        scanned_at_unix_ms: now_unix_ms(),
        agents: BTreeMap::new(),
    });
    detected.schema_version = DETECTION_SCHEMA_VERSION;
    detected.platform = current_platform().to_string();
    detected.scanned_at_unix_ms = now_unix_ms();
    detected
        .agents
        .insert(agent_id.to_string(), detection.clone());
    write_detected_agents(&detected)?;
    Ok(detection)
}

pub async fn scan_agents(catalog: &AgentSourceCatalog) -> anyhow::Result<AgentDetectionFile> {
    let mut scan_specs = Vec::new();
    for agent in crate::resources::AGENTS.iter() {
        if agent.direct_only || !agent.supports_current_platform() {
            continue;
        }
        let spec = agent_command_spec(catalog, &agent.id)?;
        scan_specs.push((agent.id.clone(), spec));
    }

    let package_manager_dirs = package_manager_candidate_dirs().await;
    let mut tasks = JoinSet::new();
    for (agent_id, spec) in scan_specs {
        let package_manager_dirs = package_manager_dirs.clone();
        tasks.spawn(async move {
            let detection = scan_agent(&agent_id, &spec, &package_manager_dirs).await;
            (agent_id, detection)
        });
    }

    let mut agents = BTreeMap::new();
    while let Some(result) = tasks.join_next().await {
        let (agent_id, detection) = result?;
        agents.insert(agent_id, detection);
    }

    Ok(AgentDetectionFile {
        schema_version: DETECTION_SCHEMA_VERSION,
        platform: current_platform().to_string(),
        scanned_at_unix_ms: now_unix_ms(),
        agents,
    })
}

fn agent_command_spec(
    catalog: &AgentSourceCatalog,
    agent_id: &str,
) -> anyhow::Result<AgentCommandSpec> {
    if let Some(spec) = catalog.agents.get(agent_id) {
        return Ok(spec.clone());
    }
    let agent = crate::resources::agent_by_id(agent_id)
        .ok_or_else(|| anyhow::anyhow!("agent '{}' not found", agent_id))?;
    Ok(AgentCommandSpec {
        program: program_from_command(agent.pty_command_for_current_platform())
            .unwrap_or_else(|| agent.id.clone()),
        version_arg: "--version".to_string(),
        sources: BTreeMap::new(),
    })
}

async fn scan_agent(
    agent_id: &str,
    spec: &AgentCommandSpec,
    package_manager_dirs: &PackageManagerCandidateDirs,
) -> AgentDetection {
    let mut paths = Vec::new();
    let mut seen = BTreeSet::new();

    for path in user_shell_paths(&spec.program).await {
        if seen.insert(normalize_path_key(&path)) {
            paths.push((path, true));
        }
    }

    for path in package_manager_dirs.candidate_paths(&spec.program) {
        if seen.insert(normalize_path_key(&path)) {
            paths.push((path, false));
        }
    }

    for path in system_candidate_paths(&spec.program) {
        if seen.insert(normalize_path_key(&path)) {
            paths.push((path, false));
        }
    }

    for path in managed_candidate_paths(&spec.program) {
        if seen.insert(normalize_path_key(&path)) {
            paths.push((path, false));
        }
    }

    if agent_id == "codex" {
        for path in codex_app_paths() {
            if seen.insert(normalize_path_key(&path)) {
                paths.push((path, false));
            }
        }
    }

    let paths = dedupe_agent_command_paths(paths, &spec.program);
    let mut candidates = Vec::new();
    for (index, (path, from_user_shell)) in paths.into_iter().enumerate() {
        let realpath = canonicalize_to_string(&path);
        let source = classify_source(&path, realpath.as_deref());
        let package = package_for_source(spec, &source);
        let version = command_version(&path, &spec.version_arg).await;
        let rank = candidate_rank(index, from_user_shell, &source, &path, realpath.as_deref());
        let source_label = source_label_for_candidate(spec, &source, &path, realpath.as_deref());
        candidates.push(AgentCandidate {
            path: path.to_string_lossy().to_string(),
            realpath,
            version,
            source_label,
            source,
            rank,
            is_user_default: from_user_shell && index == 0,
            package,
        });
    }

    candidates.sort_by_key(|candidate| candidate.rank);
    let system_selected = candidates
        .iter()
        .find(|candidate| is_system_toolchain_candidate(candidate))
        .cloned();
    let default_candidate = system_selected.clone();
    AgentDetection {
        default_candidate,
        system_selected,
        candidates,
    }
}

fn write_detected_agents(detected: &AgentDetectionFile) -> anyhow::Result<()> {
    let path = detected_agents_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("create detected agents dir {:?}", parent))?;
    }
    let json = serde_json::to_string_pretty(detected).context("serialize detected agents")?;
    std::fs::write(&path, json).with_context(|| format!("write {:?}", path))?;
    Ok(())
}

async fn user_shell_paths(program: &str) -> Vec<PathBuf> {
    if cfg!(windows) {
        return windows_path_env_paths(program);
    }
    unix_shell_paths(program).await
}

async fn unix_shell_paths(program: &str) -> Vec<PathBuf> {
    let shell = std::env::var("SHELL").unwrap_or_else(|_| {
        if cfg!(target_os = "macos") && Path::new("/bin/zsh").exists() {
            "/bin/zsh".to_string()
        } else {
            "/bin/sh".to_string()
        }
    });
    let escaped_program = shell_escape::unix::escape(std::borrow::Cow::Borrowed(program));
    let script = format!(
        r#"name={};
path_dirs=$PATH:
while [ -n "$path_dirs" ]; do
  dir=${{path_dirs%%:*}}
  path_dirs=${{path_dirs#*:}}
  [ -n "$dir" ] || dir=.
  candidate="$dir/$name"
  if [ -x "$candidate" ] && [ ! -d "$candidate" ]; then
    printf '%s\n' "$candidate"
  fi
done"#,
        escaped_program
    );
    let mut command = crate::process::env::command(shell);
    command.args(["-lic", &script]);
    output_lines(command, Duration::from_secs(6)).await
}

fn windows_path_env_paths(program: &str) -> Vec<PathBuf> {
    let Some(path_value) = crate::process::env::detection_path_value() else {
        return Vec::new();
    };
    let paths = std::env::split_paths(&path_value)
        .flat_map(|dir| program_candidates_in_dir(dir, program))
        .collect();
    dedupe_paths(paths)
}

async fn package_manager_candidate_dirs() -> PackageManagerCandidateDirs {
    let mut dirs = Vec::new();
    if cfg!(windows) {
        dirs.extend(windows_package_manager_candidate_dirs());
    } else {
        dirs.extend(npm_candidate_dirs().await);
        dirs.extend(global_bin_command_candidate_dirs("bun", &["pm", "bin", "-g"]).await);
        dirs.extend(global_bin_command_candidate_dirs("pnpm", &["bin", "-g"]).await);
        dirs.extend(global_bin_command_candidate_dirs("yarn", &["global", "bin"]).await);
        dirs.extend(homebrew_candidate_dirs().await);
    }
    PackageManagerCandidateDirs {
        dirs: dedupe_paths(dirs),
    }
}

async fn npm_candidate_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    for prefix in command_stdout_lines("npm", &["prefix", "-g"], Duration::from_secs(4)).await {
        let prefix = PathBuf::from(prefix);
        let bin_dir = if cfg!(windows) {
            prefix
        } else {
            prefix.join("bin")
        };
        dirs.push(bin_dir);
    }
    for root in command_stdout_lines("npm", &["root", "-g"], Duration::from_secs(4)).await {
        let root = PathBuf::from(root);
        if let Some(bin_dir) = root.parent().and_then(Path::parent).map(|prefix| {
            if cfg!(windows) {
                prefix.to_path_buf()
            } else {
                prefix.join("bin")
            }
        }) {
            dirs.push(bin_dir);
        }
    }
    dirs
}

async fn homebrew_candidate_dirs() -> Vec<PathBuf> {
    if !cfg!(target_os = "macos") {
        return Vec::new();
    }
    let mut dirs = Vec::new();
    for prefix in command_stdout_lines("brew", &["--prefix"], Duration::from_secs(4)).await {
        let prefix = PathBuf::from(prefix);
        dirs.push(prefix.join("bin"));
        dirs.push(prefix.join("sbin"));
    }
    dirs
}

async fn global_bin_command_candidate_dirs(command: &str, args: &[&str]) -> Vec<PathBuf> {
    command_stdout_lines(command, args, Duration::from_secs(4))
        .await
        .into_iter()
        .map(PathBuf::from)
        .collect()
}

async fn command_stdout_lines(command: &str, args: &[&str], max_duration: Duration) -> Vec<String> {
    let mut command = crate::process::env::command(command);
    command.args(args);
    output_lines(command, max_duration)
        .await
        .into_iter()
        .map(|path| path.to_string_lossy().to_string())
        .collect()
}

fn system_candidate_paths(program: &str) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    if cfg!(windows) {
        paths.extend(windows_known_candidate_paths(program));
        return paths;
    }

    let home = crate::config::home_dir();
    for dir in [
        home.join(".bun").join("bin"),
        home.join(".local").join("bin"),
        home.join(".npm-global").join("bin"),
        home.join(".yarn").join("bin"),
        home.join("Library").join("pnpm"),
        PathBuf::from("/opt/homebrew/bin"),
        PathBuf::from("/usr/local/bin"),
        PathBuf::from("/opt/homebrew/sbin"),
        PathBuf::from("/usr/bin"),
    ] {
        paths.extend(program_candidates_in_dir(dir, program));
    }
    paths
}

fn managed_candidate_paths(program: &str) -> Vec<PathBuf> {
    program_candidates_in_dir(crate::process::env::managed_npm_bin_dir(), program)
}

fn windows_package_manager_candidate_dirs() -> Vec<PathBuf> {
    if !cfg!(windows) {
        return Vec::new();
    }
    let mut dirs = Vec::new();
    if let Ok(chocolatey) = std::env::var("ChocolateyInstall") {
        dirs.push(Path::new(&chocolatey).join("bin"));
    }
    if let Ok(scoop) = std::env::var("SCOOP") {
        dirs.push(Path::new(&scoop).join("shims"));
    }
    if let Ok(scoop_global) = std::env::var("SCOOP_GLOBAL") {
        dirs.push(Path::new(&scoop_global).join("shims"));
    }
    dirs
}

fn windows_known_candidate_paths(program: &str) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    let home = crate::config::home_dir();
    if let Ok(appdata) = std::env::var("APPDATA") {
        paths.extend(program_candidates_in_dir(
            Path::new(&appdata).join("npm"),
            program,
        ));
    }
    if let Ok(localappdata) = std::env::var("LOCALAPPDATA") {
        let localappdata = Path::new(&localappdata);
        paths.extend(program_candidates_in_dir(
            localappdata
                .join("Microsoft")
                .join("WinGet")
                .join("Packages"),
            program,
        ));
        paths.extend(program_candidates_in_dir(
            localappdata.join("Programs").join("Git").join("cmd"),
            program,
        ));
    }
    paths.extend(program_candidates_in_dir(
        home.join("scoop").join("shims"),
        program,
    ));
    paths.extend(program_candidates_in_dir(
        home.join(".bun").join("bin"),
        program,
    ));
    paths.extend(program_candidates_in_dir(
        home.join(".local").join("bin"),
        program,
    ));
    paths
}

fn program_candidates_in_dir(dir: PathBuf, program: &str) -> Vec<PathBuf> {
    if cfg!(windows) {
        let candidates = if Path::new(program).extension().is_some() {
            vec![dir.join(program)]
        } else {
            vec![
                dir.join(program),
                dir.join(format!("{program}.exe")),
                dir.join(format!("{program}.cmd")),
                dir.join(format!("{program}.bat")),
                dir.join(format!("{program}.ps1")),
            ]
        };
        return candidates
            .into_iter()
            .filter(|path| path.exists())
            .collect();
    }
    let path = dir.join(program);
    if path.exists() {
        vec![path]
    } else {
        Vec::new()
    }
}

async fn output_lines(mut command: Command, max_duration: Duration) -> Vec<PathBuf> {
    let Ok(output) = tokio::time::timeout(max_duration, command.output()).await else {
        return Vec::new();
    };
    let Ok(output) = output else {
        return Vec::new();
    };
    if !output.status.success() && output.stdout.is_empty() {
        return Vec::new();
    }
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(PathBuf::from)
        .collect()
}

async fn command_stdout_text(
    command: &str,
    args: &[&str],
    max_duration: Duration,
) -> Option<String> {
    let mut command = crate::process::env::command(command);
    command.args(args);
    let output = tokio::time::timeout(max_duration, command.output())
        .await
        .ok()?
        .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).trim().to_string())
}

async fn npm_latest_version(package: &str) -> Option<String> {
    command_stdout_lines(
        "npm",
        &["view", package, "version"],
        LATEST_VERSION_CHECK_TIMEOUT,
    )
    .await
    .into_iter()
    .next()
}

async fn homebrew_latest_version(package: &str, is_cask: bool) -> Option<String> {
    if !cfg!(target_os = "macos") {
        return None;
    }
    let source_arg = if is_cask { "--cask" } else { "--formula" };
    let output = command_stdout_text(
        "brew",
        &["info", "--json=v2", source_arg, package],
        LATEST_VERSION_CHECK_TIMEOUT,
    )
    .await?;
    let json: serde_json::Value = serde_json::from_str(&output).ok()?;
    if is_cask {
        json.get("casks")?
            .as_array()?
            .first()?
            .get("version")?
            .as_str()
            .map(str::to_string)
    } else {
        json.get("formulae")?
            .as_array()?
            .first()?
            .get("versions")?
            .get("stable")?
            .as_str()
            .map(str::to_string)
    }
}

fn homebrew_package_name(agent_id: &str, source: &str) -> Option<String> {
    let catalog = source_catalog().ok()?;
    let source_spec = catalog.agents.get(agent_id)?.sources.get(source)?;
    source_spec
        .package
        .clone()
        .or_else(|| homebrew_package_name_from_command(agent_id, source))
}

fn homebrew_package_name_from_command(agent_id: &str, source: &str) -> Option<String> {
    let command = source_command_template(agent_id, source, "upgrade")?;
    command
        .split_whitespace()
        .rfind(|word| !word.starts_with('-') && !matches!(*word, "brew" | "upgrade" | "install"))
        .map(|word| word.trim_matches(['"', '\'']).to_string())
        .filter(|word| !word.is_empty())
}

async fn command_version(path: &Path, version_arg: &str) -> Option<String> {
    let mut command = command_for_version_check(path, version_arg);
    let output = tokio::time::timeout(VERSION_CHECK_TIMEOUT, command.output())
        .await
        .ok()?
        .ok()?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let text = stdout
        .lines()
        .chain(stderr.lines())
        .map(str::trim)
        .find(|line| !line.is_empty())?
        .to_string();
    Some(text)
}

fn command_for_version_check(path: &Path, version_arg: &str) -> Command {
    if cfg!(windows) {
        let ext = path
            .extension()
            .and_then(|value| value.to_str())
            .map(|value| value.to_ascii_lowercase());
        if matches!(ext.as_deref(), Some("cmd" | "bat")) {
            let mut command = crate::process::env::command("cmd.exe");
            command.arg("/C").arg(path).arg(version_arg);
            return command;
        }
        if ext.as_deref() == Some("ps1") {
            let mut command = crate::process::env::command("powershell.exe");
            command
                .args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-File"])
                .arg(path)
                .arg(version_arg);
            return command;
        }
    }
    let mut command = crate::process::env::command(path);
    command.arg(version_arg);
    command
}

fn codex_app_paths() -> Vec<PathBuf> {
    codex_app_binary_paths()
        .into_iter()
        .filter(|path| path.exists())
        .collect()
}

fn codex_app_binary_paths() -> Vec<PathBuf> {
    if cfg!(target_os = "macos") {
        [
            PathBuf::from("/Applications/ChatGPT.app"),
            crate::config::home_dir()
                .join("Applications")
                .join("ChatGPT.app"),
            PathBuf::from("/Applications/Codex.app"),
            crate::config::home_dir()
                .join("Applications")
                .join("Codex.app"),
        ]
        .into_iter()
        .map(|app| app.join("Contents").join("Resources").join("codex"))
        .collect()
    } else {
        Vec::new()
    }
}

fn classify_source(path: &Path, realpath: Option<&str>) -> String {
    let path_str = path.to_string_lossy();
    let real = realpath.unwrap_or(path_str.as_ref());
    let data_dir = crate::config::data_dir();
    let managed_prefix = data_dir.to_string_lossy();

    if path_str.starts_with(managed_prefix.as_ref()) || real.starts_with(managed_prefix.as_ref()) {
        return "npm_managed".to_string();
    }
    if path_str.contains(".bun/bin") || real.contains(".bun/install/global") {
        return "bun_global".to_string();
    }
    if real.contains("/Cellar/") || real.contains("\\Cellar\\") {
        return "homebrew_formula".to_string();
    }
    if real.contains("/Caskroom/") || real.contains("\\Caskroom\\") {
        return "homebrew_cask".to_string();
    }
    if is_app_bundle_path(&path_str) || is_app_bundle_path(real) {
        return "app_bundled".to_string();
    }
    if real.contains("/lib/node_modules/") || real.contains("\\node_modules\\") {
        return "npm_global".to_string();
    }
    if path_str.contains(".local/bin") || real.contains(".local/share") {
        return "native".to_string();
    }
    "path".to_string()
}

fn is_app_bundle_path(path: &str) -> bool {
    if cfg!(target_os = "macos") {
        return path.contains(".app/Contents/");
    }
    path.contains(".app\\Contents\\")
}

fn package_for_source(spec: &AgentCommandSpec, source: &str) -> Option<String> {
    spec.sources
        .get(source)
        .and_then(|source| source.package.clone())
        .or_else(|| {
            if source == "npm_managed" || source == "npm_global" || source == "bun_global" {
                spec.sources
                    .values()
                    .find_map(|source| source.package.clone())
            } else {
                None
            }
        })
}

fn source_label(spec: &AgentCommandSpec, source: &str) -> String {
    spec.sources
        .get(source)
        .and_then(|source| source.label.clone())
        .unwrap_or_else(|| {
            match source {
                "npm_managed" => "VibeAround npm",
                "npm_global" => "npm global",
                "bun_global" => "Bun global",
                "homebrew_formula" => "Homebrew formula",
                "homebrew_cask" => "Homebrew cask",
                "native" => "Native installer",
                "app_bundled" => "Bundled app",
                _ => "PATH",
            }
            .to_string()
        })
}

fn source_label_for_candidate(
    spec: &AgentCommandSpec,
    source: &str,
    path: &Path,
    realpath: Option<&str>,
) -> String {
    if source == "npm_global" && is_homebrew_prefix_path(path, realpath) {
        return "npm global (Homebrew prefix)".to_string();
    }
    source_label(spec, source)
}

fn is_homebrew_prefix_path(path: &Path, realpath: Option<&str>) -> bool {
    let path = path.to_string_lossy();
    let real = realpath.unwrap_or(path.as_ref());
    path.starts_with("/opt/homebrew/")
        || real.starts_with("/opt/homebrew/")
        || path.starts_with("/usr/local/")
        || real.starts_with("/usr/local/")
}

fn candidate_rank(
    index: usize,
    from_user_shell: bool,
    source: &str,
    path: &Path,
    realpath: Option<&str>,
) -> u32 {
    let base = match source {
        "app_bundled" => APP_BUNDLED_RANK_BASE,
        "npm_managed" => MANAGED_RANK_BASE,
        _ if from_user_shell => USER_SHELL_RANK_BASE,
        "homebrew_formula" | "homebrew_cask" => HOMEBREW_RANK_BASE,
        _ if is_homebrew_prefix_path(path, realpath) => HOMEBREW_RANK_BASE,
        "npm_global" | "bun_global" => NPM_GLOBAL_RANK_BASE,
        "native" => NATIVE_RANK_BASE,
        _ => GENERIC_PATH_RANK_BASE,
    };
    base + index as u32
}

fn canonicalize_to_string(path: &Path) -> Option<String> {
    std::fs::canonicalize(path)
        .ok()
        .map(|path| path.to_string_lossy().to_string())
}

fn normalize_path_key(path: &Path) -> String {
    canonicalize_to_string(path).unwrap_or_else(|| path.to_string_lossy().to_string())
}

fn dedupe_paths(paths: Vec<PathBuf>) -> Vec<PathBuf> {
    let mut seen = BTreeSet::new();
    let mut deduped = Vec::new();
    for path in paths {
        if seen.insert(normalize_path_key(&path)) {
            deduped.push(path);
        }
    }
    deduped
}

fn dedupe_agent_command_paths(paths: Vec<(PathBuf, bool)>, program: &str) -> Vec<(PathBuf, bool)> {
    let mut seen: BTreeMap<String, usize> = BTreeMap::new();
    let mut deduped: Vec<(PathBuf, bool)> = Vec::new();
    for (path, from_user_shell) in paths {
        let key = agent_command_path_key(&path, program);
        if let Some(existing_index) = seen.get(&key).copied() {
            let merged_from_user_shell = deduped[existing_index].1 || from_user_shell;
            if windows_command_variant_preferred(&path, &deduped[existing_index].0, program) {
                deduped[existing_index] = (path, merged_from_user_shell);
            } else {
                deduped[existing_index].1 = merged_from_user_shell;
            }
            continue;
        }
        seen.insert(key, deduped.len());
        deduped.push((path, from_user_shell));
    }
    deduped
}

fn agent_command_path_key(path: &Path, program: &str) -> String {
    windows_command_variant_key(path, program).unwrap_or_else(|| normalize_path_key(path))
}

fn windows_command_variant_key(path: &Path, program: &str) -> Option<String> {
    if !cfg!(windows) {
        return None;
    }
    let expected_stem = Path::new(program)
        .file_stem()
        .or_else(|| Path::new(program).file_name())?
        .to_string_lossy();
    let stem = path.file_stem()?.to_string_lossy();
    if !stem.eq_ignore_ascii_case(&expected_stem) {
        return None;
    }
    if !is_windows_command_shim_extension(path) {
        return None;
    }
    let parent = path.parent()?;
    let parent_key =
        canonicalize_to_string(parent).unwrap_or_else(|| parent.to_string_lossy().to_string());
    Some(format!(
        "windows-command-shim:{}\\{}",
        parent_key.to_ascii_lowercase(),
        expected_stem.to_ascii_lowercase()
    ))
}

fn is_windows_command_shim_extension(path: &Path) -> bool {
    matches!(
        path.extension()
            .and_then(|ext| ext.to_str())
            .map(|ext| ext.to_ascii_lowercase())
            .as_deref(),
        None | Some("cmd" | "bat" | "ps1")
    )
}

fn windows_command_variant_preferred(candidate: &Path, existing: &Path, program: &str) -> bool {
    let Some(candidate_key) = windows_command_variant_key(candidate, program) else {
        return false;
    };
    if Some(candidate_key) != windows_command_variant_key(existing, program) {
        return false;
    }
    windows_command_variant_rank(candidate) < windows_command_variant_rank(existing)
}

fn windows_command_variant_rank(path: &Path) -> u8 {
    match path
        .extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.to_ascii_lowercase())
        .as_deref()
    {
        Some("cmd") => 0,
        Some("bat") => 1,
        Some("ps1") => 2,
        None => 3,
        _ => 4,
    }
}

fn program_from_command(command: &str) -> Option<String> {
    command.split_whitespace().next().map(str::to_string)
}

fn replace_command_program(command: &str, program: &str) -> String {
    let trimmed = command.trim_start();
    let Some(first) = program_from_command(trimmed) else {
        return shell_command_word(program);
    };
    let rest = trimmed
        .strip_prefix(&first)
        .map(str::trim_start)
        .unwrap_or_default();
    let program = shell_command_word(program);
    if rest.is_empty() {
        program
    } else {
        format!("{program} {rest}")
    }
}

fn shell_command_word(value: &str) -> String {
    shell_escape::unix::escape(Cow::Borrowed(value)).to_string()
}

fn comparable_version(value: &str) -> Option<String> {
    let mut out = String::new();
    let mut started = false;
    for ch in value.chars() {
        if !started {
            if ch.is_ascii_digit() {
                started = true;
                out.push(ch);
            }
            continue;
        }
        if ch.is_ascii_alphanumeric() || matches!(ch, '.' | '-' | '_' | '+') {
            out.push(ch);
        } else {
            break;
        }
    }
    let out = out.trim_matches(['.', '-', '_', '+']).to_ascii_lowercase();
    (!out.is_empty()).then_some(out)
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

    #[test]
    fn detection_cache_requires_current_schema_and_platform() {
        let mut detected = AgentDetectionFile {
            schema_version: DETECTION_SCHEMA_VERSION,
            platform: current_platform().to_string(),
            scanned_at_unix_ms: 0,
            agents: BTreeMap::new(),
        };
        assert!(is_compatible_detection_cache(&detected));

        detected.schema_version -= 1;
        assert!(!is_compatible_detection_cache(&detected));
        detected.schema_version = DETECTION_SCHEMA_VERSION;
        detected.platform = "other".to_string();
        assert!(!is_compatible_detection_cache(&detected));
    }

    #[test]
    fn source_catalog_parses_agent_commands() {
        let catalog = source_catalog().expect("catalog parses");
        let codex = catalog.agents.get("codex").expect("codex source");
        assert_eq!(codex.program, "codex");
        assert!(!codex.sources.contains_key("npm_managed"));
        assert_eq!(
            codex.sources["npm_global"].package.as_deref(),
            Some("@openai/codex")
        );
    }

    #[test]
    fn classifies_homebrew_npm_global_separately_from_formula() {
        assert_eq!(
            classify_source(
                Path::new("/opt/homebrew/bin/codex"),
                Some("/opt/homebrew/lib/node_modules/@openai/codex/bin/codex.js"),
            ),
            "npm_global"
        );
        assert_eq!(
            classify_source(
                Path::new("/opt/homebrew/bin/opencode"),
                Some("/opt/homebrew/Cellar/opencode/1.17.0/bin/opencode"),
            ),
            "homebrew_formula"
        );
    }

    #[test]
    fn labels_homebrew_prefix_npm_globals_without_treating_them_as_brew_packages() {
        let catalog = source_catalog().expect("catalog parses");
        let spec = catalog.agents.get("codex").expect("codex source");
        assert_eq!(
            source_label_for_candidate(
                spec,
                "npm_global",
                Path::new("/opt/homebrew/bin/codex"),
                Some("/opt/homebrew/lib/node_modules/@openai/codex/bin/codex.js"),
            ),
            "npm global (Homebrew prefix)"
        );
    }

    #[test]
    fn native_sources_can_reuse_agent_package_for_latest_checks() {
        assert_eq!(
            source_package("claude", "native").as_deref(),
            Some("@anthropic-ai/claude-code")
        );
        assert_eq!(
            source_package("codex", "app_bundled").as_deref(),
            Some("@openai/codex")
        );
    }

    #[test]
    fn replace_command_program_quotes_selected_paths_with_spaces() {
        assert_eq!(
            replace_command_program(
                "claude code --permission-mode acceptEdits",
                "/Applications/Claude CLI/bin/claude"
            ),
            "'/Applications/Claude CLI/bin/claude' code --permission-mode acceptEdits"
        );
    }

    #[test]
    fn replace_command_program_quotes_empty_fallback() {
        assert_eq!(
            replace_command_program("", "/tmp/Codex CLI"),
            "'/tmp/Codex CLI'"
        );
    }

    #[test]
    fn npm_installability_comes_from_source_catalog() {
        assert!(agent_uses_npm_install("codex"));
        assert!(agent_uses_npm_install("claude"));
        assert!(agent_uses_npm_install("gemini"));
        assert!(agent_uses_npm_install("qwen-code"));
        assert!(!agent_uses_npm_install("cursor"));
    }

    #[test]
    fn codex_app_binary_paths_include_user_applications() {
        let paths = codex_app_binary_paths();
        if cfg!(target_os = "macos") {
            assert!(paths.contains(&PathBuf::from(
                "/Applications/ChatGPT.app/Contents/Resources/codex"
            )));
            assert!(paths.contains(
                &crate::config::home_dir()
                    .join("Applications")
                    .join("ChatGPT.app")
                    .join("Contents")
                    .join("Resources")
                    .join("codex")
            ));
            assert!(paths.contains(&PathBuf::from(
                "/Applications/Codex.app/Contents/Resources/codex"
            )));
            assert!(paths.contains(
                &crate::config::home_dir()
                    .join("Applications")
                    .join("Codex.app")
                    .join("Contents")
                    .join("Resources")
                    .join("codex")
            ));
        } else {
            assert!(paths.is_empty());
        }
    }

    #[test]
    fn managed_candidates_rank_after_user_shell_hits() {
        assert!(
            candidate_rank(
                0,
                true,
                "npm_global",
                Path::new("/usr/local/bin/codex"),
                None
            ) < candidate_rank(
                0,
                false,
                "npm_managed",
                Path::new("/tmp/.vibearound/npm/bin/codex"),
                None
            )
        );
    }

    #[test]
    fn homebrew_candidates_rank_before_other_fallbacks() {
        assert!(
            candidate_rank(
                0,
                false,
                "npm_global",
                Path::new("/opt/homebrew/bin/codex"),
                Some("/opt/homebrew/lib/node_modules/@openai/codex/bin/codex.js")
            ) < candidate_rank(
                0,
                false,
                "npm_global",
                Path::new("/tmp/npm/bin/codex"),
                Some("/tmp/npm/lib/node_modules/@openai/codex/bin/codex.js")
            )
        );
    }

    #[test]
    fn app_bundled_candidates_rank_after_cli_candidates() {
        assert!(
            candidate_rank(
                0,
                true,
                "app_bundled",
                Path::new("/Applications/Codex.app/Contents/Resources/codex"),
                None
            ) > candidate_rank(
                0,
                false,
                "npm_global",
                Path::new("/opt/homebrew/bin/codex"),
                None
            )
        );
    }

    #[test]
    fn dedupe_paths_uses_realpath_when_available() {
        let first = crate::config::home_dir()
            .join(".bun")
            .join("bin")
            .join("codex");
        let second = first.clone();
        let paths = dedupe_paths(vec![first, second]);
        assert_eq!(paths.len(), 1);
    }

    #[test]
    #[cfg(target_os = "windows")]
    fn dedupe_agent_command_paths_prefers_cmd_over_other_windows_shims() {
        let bin_dir = std::env::temp_dir().join("vibearound-agent-detection-windows-shims");
        let paths = vec![
            (bin_dir.join("codex"), true),
            (bin_dir.join("codex.cmd"), true),
            (bin_dir.join("codex.ps1"), true),
        ];

        let deduped = dedupe_agent_command_paths(paths, "codex");

        assert_eq!(deduped.len(), 1);
        assert_eq!(deduped[0].0, bin_dir.join("codex.cmd"));
        assert!(deduped[0].1);
    }

    #[test]
    fn system_selection_prefers_system_candidate() {
        let system = test_candidate("/usr/local/bin/codex", "npm_global", 0);
        let managed = test_candidate("/tmp/.vibearound/npm/bin/codex", "npm_managed", 10_000);
        let detection = AgentDetection {
            default_candidate: Some(managed.clone()),
            system_selected: Some(system.clone()),
            candidates: vec![system.clone(), managed.clone()],
        };

        assert_eq!(
            detection
                .system_selected_candidate()
                .as_ref()
                .map(|candidate| candidate.path.as_str()),
            Some(system.path.as_str())
        );

        let json = serde_json::to_string(&detection).expect("serialize detection");
        assert!(json.contains("\"default\""));
        assert!(json.contains("\"systemSelected\""));
        assert!(!json.contains("\"selected\""));
    }

    #[test]
    fn managed_startkit_selection_accepts_managed_candidate_for_npm_agents() {
        let managed = test_candidate("/tmp/.vibearound/npm/bin/codex", "npm_managed", 10_000);
        let detection = AgentDetection {
            default_candidate: Some(managed.clone()),
            system_selected: Some(managed.clone()),
            candidates: vec![managed.clone()],
        };

        assert_eq!(
            preferred_startkit_candidate("codex", &detection, "managed")
                .as_ref()
                .map(|candidate| candidate.path.as_str()),
            Some(managed.path.as_str())
        );
        assert!(preferred_startkit_candidate("codex", &detection, "system").is_none());
    }

    #[test]
    fn system_selection_uses_selected_path_and_ignores_unselected_candidates() {
        let system = test_candidate("/usr/local/bin/codex", "npm_global", 0);
        let managed = test_candidate("/tmp/.vibearound/npm/bin/codex", "npm_managed", 10_000);
        let detection = AgentDetection {
            default_candidate: Some(managed.clone()),
            system_selected: Some(system.clone()),
            candidates: vec![managed.clone(), system.clone()],
        };

        assert_eq!(
            detection
                .system_selected_candidate()
                .as_ref()
                .map(|candidate| candidate.path.as_str()),
            Some(system.path.as_str())
        );

        let managed_only = AgentDetection {
            default_candidate: Some(managed.clone()),
            system_selected: Some(managed.clone()),
            candidates: vec![managed.clone(), system.clone()],
        };
        assert!(managed_only.system_selected_candidate().is_none());
    }

    #[test]
    fn system_selection_accepts_system_toolchain_candidates_off_probe_path() {
        let npm_global_not_on_path = test_candidate("/opt/homebrew/bin/codex", "npm_global", 5_000);
        let app_bundled = test_candidate(
            "/Applications/Codex.app/Contents/Resources/codex",
            "app_bundled",
            20_000,
        );
        let detection = AgentDetection {
            default_candidate: Some(npm_global_not_on_path.clone()),
            system_selected: Some(npm_global_not_on_path.clone()),
            candidates: vec![app_bundled, npm_global_not_on_path.clone()],
        };

        assert_eq!(
            detection
                .system_selected_candidate()
                .as_ref()
                .map(|candidate| candidate.path.as_str()),
            Some(npm_global_not_on_path.path.as_str())
        );
        assert!(preferred_startkit_candidate("codex", &detection, "system").is_some());
    }

    #[test]
    fn app_bundle_paths_are_not_system_toolchain_candidates() {
        let candidate = test_candidate(
            "/Applications/Claude.app/Contents/Resources/claude",
            "app_bundled",
            0,
        );
        assert!(!is_system_toolchain_candidate(&candidate));
        assert_eq!(
            classify_source(
                Path::new("/usr/local/bin/claude"),
                Some("/Applications/Claude.app/Contents/Resources/claude"),
            ),
            "app_bundled"
        );
    }

    #[test]
    fn update_available_compares_version_numbers_inside_cli_output() {
        let mut candidate = test_candidate("/usr/local/bin/codex", "npm_global", 0);
        candidate.version = Some("codex-cli 0.139.0".to_string());

        assert_eq!(
            candidate_update_available(&candidate, Some("0.139.0")),
            Some(false)
        );
        assert_eq!(
            candidate_update_available(&candidate, Some("0.140.0")),
            Some(true)
        );
    }

    #[test]
    fn homebrew_package_name_comes_from_upgrade_command() {
        assert_eq!(
            homebrew_package_name_from_command("gemini", "homebrew_formula").as_deref(),
            Some("gemini-cli")
        );
        assert_eq!(
            homebrew_package_name_from_command("kiro", "homebrew_cask").as_deref(),
            Some("kiro-cli")
        );
    }

    #[test]
    fn candidate_for_path_matches_detected_realpath() {
        let temp_dir = std::env::temp_dir().join(format!(
            "vibearound-agent-detection-test-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&temp_dir).expect("create temp dir");
        let path = temp_dir.join("codex");
        std::fs::write(&path, "#!/bin/sh\n").expect("write temp executable");
        let realpath = std::fs::canonicalize(&path)
            .expect("canonicalize temp executable")
            .to_string_lossy()
            .to_string();

        let mut candidate = test_candidate("/displayed/codex", "npm_global", 1);
        candidate.realpath = Some(realpath);
        candidate.version = Some("codex-cli 0.139.0".to_string());
        let detection = AgentDetection {
            default_candidate: Some(candidate.clone()),
            system_selected: Some(candidate.clone()),
            candidates: vec![candidate],
        };

        let matched = candidate_for_path(&detection, &path).expect("candidate matched");
        assert_eq!(matched.source, "npm_global");
        assert_eq!(matched.version.as_deref(), Some("codex-cli 0.139.0"));

        let _ = std::fs::remove_file(path);
        let _ = std::fs::remove_dir(temp_dir);
    }

    fn test_candidate(path: &str, source: &str, rank: u32) -> AgentCandidate {
        AgentCandidate {
            path: path.to_string(),
            realpath: None,
            version: None,
            source: source.to_string(),
            source_label: source.to_string(),
            rank,
            is_user_default: rank == 0,
            package: None,
        }
    }
}
