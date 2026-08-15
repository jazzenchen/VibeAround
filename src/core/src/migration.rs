//! One-time migrations for files under the VibeAround data directory.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use chrono::Local;

const APPLICATION_VERSION: &str = env!("CARGO_PKG_VERSION");
const LEGACY_STATE_FILES: [&str; 2] = ["workspaces.jsonl", "workspace-threads.jsonl"];
const DASHSCOPE_PROVIDER_ID: &str = "dashscope";
const DASHSCOPE_LABEL: &str = "Alibaba DashScope";
const LEGACY_QWEN_PROVIDER_ID: &str = "qwen";
const LEGACY_QWEN_LABEL: &str = "Qwen / DashScope";
const MOONSHOT_PROVIDER_ID: &str = "moonshot";
const LEGACY_KIMI_PROVIDER_ID: &str = "kimi";
const KIMI_CODING_ENDPOINT_ID: &str = "kimi-coding";
const KIMI_CODING_LEGACY_BASE_URL: &str = "https://api.kimi.com/coding";
const GEMINI_PROVIDER_ID: &str = "gemini";
const GEMINI_API_ENDPOINT_ID: &str = "gemini-api";
const LEGACY_GEMINI_OPENAI_ENDPOINT_ID: &str = "openai-compatible";

pub fn run() -> Result<()> {
    run_at(&crate::config::data_dir())
}

fn run_at(data_dir: &Path) -> Result<()> {
    std::fs::create_dir_all(data_dir)
        .with_context(|| format!("create data directory {}", data_dir.display()))?;
    let _lock = crate::file_lock::ExclusiveFileLock::acquire(&data_dir.join("migration.lock"))
        .with_context(|| format!("lock migrations in {}", data_dir.display()))?;

    let mut changes = legacy_state_changes(data_dir);
    changes.extend(legacy_profile_changes(data_dir)?);
    if changes.is_empty() {
        return Ok(());
    }

    let backup_dir = create_backup(data_dir, changes.iter().map(Change::source))?;
    for change in changes {
        apply_change(change)?;
    }
    tracing::info!(backup = ?backup_dir, "completed configuration migration");
    Ok(())
}

enum Change {
    Rewrite { path: PathBuf, contents: String },
    MoveState { source: PathBuf, target: PathBuf },
}

impl Change {
    fn source(&self) -> &Path {
        match self {
            Self::Rewrite { path, .. } => path,
            Self::MoveState { source, .. } => source,
        }
    }
}

fn legacy_state_changes(data_dir: &Path) -> Vec<Change> {
    LEGACY_STATE_FILES
        .iter()
        .filter_map(|name| {
            let source = data_dir.join(name);
            source.exists().then(|| Change::MoveState {
                source,
                target: data_dir.join("state").join(name),
            })
        })
        .collect()
}

fn legacy_profile_changes(data_dir: &Path) -> Result<Vec<Change>> {
    let profiles_dir = data_dir.join("profiles");
    let entries = match std::fs::read_dir(&profiles_dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => {
            return Err(error).with_context(|| format!("read {}", profiles_dir.display()))
        }
    };
    let mut paths = entries
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| path.extension().and_then(|value| value.to_str()) == Some("json"))
        .collect::<Vec<_>>();
    paths.sort();

    let mut changes = Vec::new();
    for path in paths {
        let body = std::fs::read_to_string(&path)
            .with_context(|| format!("read profile {}", path.display()))?;
        let mut profile: crate::profiles::ProfileDef = match serde_json::from_str(&body) {
            Ok(profile) => profile,
            Err(error) => {
                tracing::warn!(path = ?path, %error, "skipping invalid profile during migration");
                continue;
            }
        };
        let provider_changed = migrate_legacy_profile_provider(&mut profile);
        let api_config_count = profile.api_configs.len();
        crate::profiles::schema::hydrate_legacy_api_configs(&mut profile);
        if !provider_changed && profile.api_configs.len() == api_config_count {
            continue;
        }
        changes.push(Change::Rewrite {
            path,
            contents: serde_json::to_string_pretty(&profile)
                .context("serialize migrated profile")?,
        });
    }
    Ok(changes)
}

pub(crate) fn migrate_legacy_profile_provider(profile: &mut crate::profiles::ProfileDef) -> bool {
    if !needs_legacy_profile_provider_migration(profile) {
        return false;
    }

    normalize_legacy_dashscope_profile(profile);
    normalize_legacy_kimi_profile(profile);
    normalize_legacy_gemini_profile(profile);
    if profile.provider == "azure" && profile.api_types.iter().any(|item| item == "openai-chat") {
        let chat_overrides = profile.overrides.remove("openai-chat");
        profile.api_types.retain(|item| item != "openai-chat");
        if !profile
            .api_types
            .iter()
            .any(|item| item == "openai-responses")
        {
            profile.api_types.push("openai-responses".to_string());
            if let Some(overrides) = chat_overrides {
                profile
                    .overrides
                    .entry("openai-responses".to_string())
                    .or_insert(overrides);
            }
        }
    }
    true
}

fn needs_legacy_profile_provider_migration(profile: &crate::profiles::ProfileDef) -> bool {
    profile.provider == LEGACY_QWEN_PROVIDER_ID
        || (profile.provider == DASHSCOPE_PROVIDER_ID
            && (profile.label == LEGACY_QWEN_LABEL
                || profile.overrides.values().any(|overrides| {
                    matches!(
                        overrides.endpoint_id.as_deref(),
                        Some("coding-global" | "coding-cn" | "standard-global" | "standard-cn")
                    )
                })))
        || profile.provider == LEGACY_KIMI_PROVIDER_ID
        || (profile.provider == GEMINI_PROVIDER_ID
            && (profile.auth_mode == crate::profiles::AuthMode::OauthViaCli
                || profile.overrides.values().any(|overrides| {
                    overrides.endpoint_id.as_deref() == Some(LEGACY_GEMINI_OPENAI_ENDPOINT_ID)
                })))
        || (profile.provider == "azure"
            && profile.api_types.iter().any(|item| item == "openai-chat"))
}

fn normalize_legacy_dashscope_profile(profile: &mut crate::profiles::ProfileDef) {
    if profile.provider == LEGACY_QWEN_PROVIDER_ID {
        profile.provider = DASHSCOPE_PROVIDER_ID.to_string();
    }
    if profile.provider != DASHSCOPE_PROVIDER_ID {
        return;
    }
    if profile.label == LEGACY_QWEN_LABEL {
        profile.label = DASHSCOPE_LABEL.to_string();
    }
    for overrides in profile.overrides.values_mut() {
        overrides.endpoint_id = match overrides.endpoint_id.as_deref() {
            Some("coding-global") => Some("coding-plan".to_string()),
            Some("coding-cn") => Some("coding-plan-cn".to_string()),
            Some("standard-global") => Some("token-plan".to_string()),
            Some("standard-cn") => Some("token-plan-cn".to_string()),
            _ => overrides.endpoint_id.clone(),
        };
    }
}

fn normalize_legacy_kimi_profile(profile: &mut crate::profiles::ProfileDef) {
    if profile.provider != LEGACY_KIMI_PROVIDER_ID {
        return;
    }
    profile.provider = MOONSHOT_PROVIDER_ID.to_string();
    if !profile.api_types.iter().any(|item| item == "anthropic") {
        return;
    }
    let overrides = profile
        .overrides
        .entry("anthropic".to_string())
        .or_default();
    if matches!(overrides.endpoint_id.as_deref(), None | Some("anthropic")) {
        overrides.endpoint_id = Some(KIMI_CODING_ENDPOINT_ID.to_string());
    }
    if overrides
        .base_url
        .as_deref()
        .map(|value| value.trim_end_matches('/'))
        == Some(KIMI_CODING_LEGACY_BASE_URL)
    {
        overrides.base_url = None;
    }
}

fn normalize_legacy_gemini_profile(profile: &mut crate::profiles::ProfileDef) {
    if profile.provider != GEMINI_PROVIDER_ID {
        return;
    }
    if profile.auth_mode == crate::profiles::AuthMode::OauthViaCli {
        profile.auth_mode = crate::profiles::AuthMode::GoogleOauth;
    }
    for overrides in profile.overrides.values_mut() {
        if overrides.endpoint_id.as_deref() == Some(LEGACY_GEMINI_OPENAI_ENDPOINT_ID) {
            overrides.endpoint_id = Some(GEMINI_API_ENDPOINT_ID.to_string());
        }
    }
}

fn create_backup<'a>(
    data_dir: &Path,
    sources: impl IntoIterator<Item = &'a Path>,
) -> Result<PathBuf> {
    let backup_root = data_dir.join("migration-backups");
    create_private_dir(&backup_root)?;
    let version_dir = backup_root.join(format!("v{APPLICATION_VERSION}"));
    create_private_dir(&version_dir)?;
    let date_dir = version_dir.join(Local::now().format("%Y-%m-%d").to_string());
    create_private_dir(&date_dir)?;
    let backup_dir = next_backup_dir(&date_dir)?;
    create_private_dir(&backup_dir)?;

    for source in sources {
        let relative = source
            .strip_prefix(data_dir)
            .with_context(|| format!("{} is outside {}", source.display(), data_dir.display()))?;
        let target = backup_dir.join(relative);
        if let Some(parent) = target.parent() {
            create_private_dir(parent)?;
        }
        std::fs::copy(source, &target)
            .with_context(|| format!("back up {} to {}", source.display(), target.display()))?;
        make_private_file(&target)?;
    }

    Ok(backup_dir)
}

fn next_backup_dir(date_dir: &Path) -> Result<PathBuf> {
    let count = std::fs::read_dir(date_dir)
        .with_context(|| format!("read {}", date_dir.display()))?
        .filter_map(|entry| entry.ok())
        .filter_map(|entry| entry.file_name().to_str()?.parse::<u32>().ok())
        .max()
        .unwrap_or(0)
        + 1;
    Ok(date_dir.join(format!("{count:03}")))
}

fn apply_change(change: Change) -> Result<()> {
    match change {
        Change::Rewrite { path, contents } => crate::file_replace::write_private(&path, contents)
            .with_context(|| format!("write migrated {}", path.display())),
        Change::MoveState { source, target } => {
            if target.exists() {
                return std::fs::remove_file(&source)
                    .with_context(|| format!("remove migrated {}", source.display()));
            }
            if let Some(parent) = target.parent() {
                create_private_dir(parent)?;
            }
            std::fs::rename(&source, &target)
                .with_context(|| format!("move {} to {}", source.display(), target.display()))
        }
    }
}

fn create_private_dir(path: &Path) -> Result<()> {
    std::fs::create_dir_all(path).with_context(|| format!("create {}", path.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))
            .with_context(|| format!("set permissions on {}", path.display()))?;
    }
    Ok(())
}

fn make_private_file(path: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
            .with_context(|| format!("set permissions on {}", path.display()))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_dir() -> PathBuf {
        std::env::temp_dir().join(format!(
            "vibearound-migration-{}-{}",
            std::process::id(),
            nanoid::nanoid!(8)
        ))
    }

    #[test]
    fn backs_up_then_moves_legacy_state_files_once() {
        let dir = test_dir();
        std::fs::create_dir_all(dir.join("state")).unwrap();
        std::fs::write(dir.join("workspaces.jsonl"), "legacy-workspaces\n").unwrap();
        std::fs::write(dir.join("workspace-threads.jsonl"), "legacy-threads\n").unwrap();
        std::fs::write(
            dir.join("state/workspace-threads.jsonl"),
            "current-threads\n",
        )
        .unwrap();

        run_at(&dir).unwrap();

        assert!(!dir.join("workspaces.jsonl").exists());
        assert!(!dir.join("workspace-threads.jsonl").exists());
        assert_eq!(
            std::fs::read_to_string(dir.join("state/workspaces.jsonl")).unwrap(),
            "legacy-workspaces\n"
        );
        assert_eq!(
            std::fs::read_to_string(dir.join("state/workspace-threads.jsonl")).unwrap(),
            "current-threads\n"
        );

        let backups = backup_dirs(&dir);
        assert_eq!(backups.len(), 1);
        assert!(backups[0].starts_with(
            dir.join("migration-backups")
                .join(format!("v{APPLICATION_VERSION}"))
        ));
        assert_eq!(
            std::fs::read_to_string(backups[0].join("workspaces.jsonl")).unwrap(),
            "legacy-workspaces\n"
        );
        assert_eq!(
            std::fs::read_to_string(backups[0].join("workspace-threads.jsonl")).unwrap(),
            "legacy-threads\n"
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(&backups[0]).unwrap().permissions().mode() & 0o777,
                0o700
            );
            assert_eq!(
                std::fs::metadata(backups[0].join("workspaces.jsonl"))
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777,
                0o600
            );
        }

        run_at(&dir).unwrap();
        assert_eq!(backup_dirs(&dir).len(), 1);

        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn backs_up_then_rewrites_legacy_provider_profiles_once() {
        let dir = test_dir();
        std::fs::create_dir_all(dir.join("profiles")).unwrap();
        let legacy_path = dir.join("profiles/qwen-old.json");
        std::fs::write(
            &legacy_path,
            r#"{
  "id": "qwen-old",
  "label": "Qwen / DashScope",
  "provider": "qwen",
  "auth_mode": "api_key",
  "api_types": ["openai-chat"],
  "credentials": { "api_key": "secret" },
  "overrides": {
    "openai-chat": { "endpoint_id": "standard-cn" }
  }
}"#,
        )
        .unwrap();
        std::fs::write(
            dir.join("profiles/current.json"),
            r#"{
  "id": "current",
  "label": "Current",
  "provider": "deepseek",
  "auth_mode": "api_key",
  "api_types": ["openai-chat"],
  "api_configs": {
    "openai-chat": { "enabled": true }
  }
}"#,
        )
        .unwrap();

        run_at(&dir).unwrap();

        let migrated: crate::profiles::ProfileDef =
            serde_json::from_str(&std::fs::read_to_string(&legacy_path).unwrap()).unwrap();
        assert_eq!(migrated.provider, "dashscope");
        assert_eq!(migrated.label, "Alibaba DashScope");
        assert_eq!(
            migrated.overrides["openai-chat"].endpoint_id.as_deref(),
            Some("token-plan-cn")
        );
        assert_eq!(
            migrated.api_configs["openai-chat"].endpoint_id.as_deref(),
            Some("token-plan-cn")
        );
        assert_eq!(migrated.credentials["api_key"], "secret");

        let backups = backup_dirs(&dir);
        assert_eq!(backups.len(), 1);
        let original = std::fs::read_to_string(backups[0].join("profiles/qwen-old.json")).unwrap();
        assert!(original.contains("\"provider\": \"qwen\""));
        assert!(!backups[0].join("profiles/current.json").exists());

        run_at(&dir).unwrap();
        assert_eq!(backup_dirs(&dir).len(), 1);

        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn numbers_each_backup_for_the_application_version_and_local_date() {
        let dir = test_dir();
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("workspaces.jsonl"), "legacy-workspaces\n").unwrap();
        run_at(&dir).unwrap();

        std::fs::write(
            dir.join("workspace-threads.jsonl"),
            "legacy-workspace-threads\n",
        )
        .unwrap();
        run_at(&dir).unwrap();

        let backups = backup_dirs(&dir);
        assert_eq!(backups.len(), 2);
        assert_eq!(backups[0].file_name().unwrap(), "001");
        assert_eq!(backups[1].file_name().unwrap(), "002");

        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn migrates_legacy_azure_api_type() {
        let mut profile: crate::profiles::ProfileDef = serde_json::from_value(serde_json::json!({
            "id": "azure-old",
            "label": "Azure",
            "provider": "azure",
            "auth_mode": "api_key",
            "api_types": ["openai-chat"],
            "overrides": {
                "openai-chat": { "model": "gpt-5" }
            }
        }))
        .unwrap();

        assert!(migrate_legacy_profile_provider(&mut profile));
        assert_eq!(profile.api_types, ["openai-responses"]);
        assert_eq!(
            profile.overrides["openai-responses"].model.as_deref(),
            Some("gpt-5")
        );
    }

    fn backup_dirs(data_dir: &Path) -> Vec<PathBuf> {
        let version_dir = data_dir
            .join("migration-backups")
            .join(format!("v{APPLICATION_VERSION}"));
        let mut dirs = std::fs::read_dir(version_dir)
            .unwrap()
            .flat_map(|entry| std::fs::read_dir(entry.unwrap().path()).unwrap())
            .map(|entry| entry.unwrap().path())
            .filter(|path| path.is_dir())
            .collect::<Vec<_>>();
        dirs.sort();
        dirs
    }
}
