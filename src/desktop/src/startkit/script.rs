use std::collections::BTreeMap;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{anyhow, bail};
use common::script::ScriptOutcome;

use super::{
    is_managed_mode, item_uses_managed_dependency_dir, portable_toolchain_enabled, Manifest,
    PlatformScript, StartkitChoices, StartkitItem, StartkitItemStatus, StartkitPaths,
    StartkitProgress,
};

pub(super) type ScriptOutput = ScriptOutcome;

/// Runs one manifest script for an item, forwarding its progress messages to the
/// item's progress callback.
#[allow(clippy::too_many_arguments)]
pub(super) async fn run_script(
    manifest: &Manifest,
    paths: &StartkitPaths,
    item: &StartkitItem,
    choices: &StartkitChoices,
    platform: &str,
    script_path: &str,
    script: &PlatformScript,
    cancelled: Option<&Arc<AtomicBool>>,
    progress: StartkitProgress<'_>,
) -> anyhow::Result<ScriptOutput> {
    let full_path = paths.root.join(script_path);
    if !full_path.exists() {
        bail!("script not found: {}", full_path.display());
    }

    let mut env = BTreeMap::new();
    apply_startkit_env(&mut env, manifest, paths, item, choices)?;

    common::script::run(
        common::script::command_for(&full_path, &script.args, platform),
        &env,
        Duration::from_secs(manifest.runner.default_timeout_secs),
        cancelled,
        &manifest.runner.log_redact_keys,
        |message| {
            if let Some(progress) = progress {
                progress(item, StartkitItemStatus::Running, Some(message));
            }
        },
    )
    .await
}

/// Environment values arrive as paths, owned strings, and literals; normalize
/// them all into the string map the runner sends to the child.
fn set(env: &mut BTreeMap<String, String>, key: &str, value: impl AsRef<std::ffi::OsStr>) {
    env.insert(
        key.to_string(),
        value.as_ref().to_string_lossy().into_owned(),
    );
}

fn apply_startkit_env(
    env: &mut BTreeMap<String, String>,
    manifest: &Manifest,
    paths: &StartkitPaths,
    item: &StartkitItem,
    choices: &StartkitChoices,
) -> anyhow::Result<()> {
    std::fs::create_dir_all(&paths.cache_dir).ok();

    let source = manifest
        .sources
        .get(&choices.source)
        .or_else(|| manifest.sources.get("global"))
        .ok_or_else(|| anyhow!("startkit source '{}' not found", choices.source))?;

    set(env, "STARTKIT_HOME", &paths.home);
    set(env, "STARTKIT_ROOT", &paths.root);
    set(env, "STARTKIT_CACHE_DIR", &paths.cache_dir);
    set(env, "STARTKIT_SOURCE", &choices.source);
    // Scripts that can install either a system-wide or a VibeAround-managed copy
    // of a tool need the user's toolchain choice to pick a path.
    set(env, "STARTKIT_TOOLCHAIN_MODE", &choices.toolchain_mode);
    set(env, "STARTKIT_PORTABLE_TOOLCHAIN", if portable_toolchain_enabled(choices) {
            "true"
        } else {
            "false"
        },);
    let managed_item_active = item_uses_managed_dependency_dir(item) && is_managed_mode(choices);
    set(env, "STARTKIT_ITEM_MANAGED", if managed_item_active { "true" } else { "false" },);
    set(env, "STARTKIT_NPM_REGISTRY", &source.npm_registry);
    set(env, "STARTKIT_NODE_INDEX_URL", &source.node_index);
    set(env, "STARTKIT_NODE_DIST_BASE", &source.node_dist);
    set(env, "STARTKIT_CAN_INSTALL", if item.install.is_some() && (!item.managed || managed_item_active) {
            "true"
        } else {
            "false"
        },);
    set(env, "STARTKIT_ITEM_ID", &item.id);
    if let Some(value) = &item.min_version {
        set(env, "STARTKIT_MIN_VERSION", value);
    }
    if let Some(value) = &item.program {
        set(env, "STARTKIT_PROGRAM", value);
    }
    if let Some(value) = &item.version_arg {
        set(env, "STARTKIT_VERSION_ARG", value);
    }
    if let Some(value) = &item.npm_package {
        set(env, "STARTKIT_NPM_PACKAGE", value);
    }
    if let Some(value) = &item.plugin_dependency {
        let plugin_dir = common::plugins::user_plugin_dependency_dir(value);
        let plugin_bin_dir = plugin_dir.join("bin");
        std::fs::create_dir_all(&plugin_bin_dir).ok();
        set(env, "STARTKIT_PLUGIN_DIR", plugin_dir);
        set(env, "STARTKIT_PLUGIN_BIN_DIR", plugin_bin_dir);
    }

    Ok(())
}
