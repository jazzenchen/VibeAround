use std::collections::{HashMap, HashSet};

use anyhow::{anyhow, bail};

use crate::agent_detection;

use super::{Manifest, StartkitChoices, StartkitItem, StartkitItemSummary, StartkitPlan};

pub(in crate::startkit) fn item_summary(item: &StartkitItem) -> StartkitItemSummary {
    StartkitItemSummary {
        id: item.id.clone(),
        label: item.label.clone(),
        group: item.group.clone(),
        category: item.category.clone(),
        description: item.description.clone(),
        severity: item.severity.clone(),
        kind: item.kind.clone(),
        managed: item.managed,
        has_repair: item.repair.is_some(),
        secret: item.secret,
        settings_key: item.settings_key.clone(),
    }
}

pub(in crate::startkit) fn plan_from_manifest(
    manifest: &Manifest,
    choices: &StartkitChoices,
    platform: &str,
) -> anyhow::Result<StartkitPlan> {
    let by_id: HashMap<&str, &StartkitItem> = manifest
        .items
        .iter()
        .map(|item| (item.id.as_str(), item))
        .collect();
    let mut selected = HashSet::<String>::new();

    for item in &manifest.items {
        if !supports_platform(item, platform) {
            continue;
        }
        if should_include(item, choices, platform) {
            add_with_deps(item, &by_id, platform, choices, &mut selected)?;
        }
    }

    let mut ordered = Vec::new();
    let mut temporary = HashSet::new();
    let mut permanent = HashSet::new();
    for id in selected.iter() {
        visit(
            id,
            &by_id,
            platform,
            choices,
            &selected,
            &mut temporary,
            &mut permanent,
            &mut ordered,
        )?;
    }

    let items = ordered
        .iter()
        .map(|id| item_summary(find_item(manifest, id).expect("planned item exists")))
        .collect();

    Ok(StartkitPlan {
        platform: platform.to_string(),
        source: choices.source.clone(),
        item_ids: ordered,
        items,
    })
}

fn add_with_deps(
    item: &StartkitItem,
    by_id: &HashMap<&str, &StartkitItem>,
    platform: &str,
    choices: &StartkitChoices,
    selected: &mut HashSet<String>,
) -> anyhow::Result<()> {
    selected.insert(item.id.clone());
    for dep in effective_item_dependencies(item, choices, platform) {
        let dep_item = by_id
            .get(dep)
            .ok_or_else(|| anyhow!("startkit item '{}' depends on missing '{}'", item.id, dep))?;
        if !supports_platform(dep_item, platform) {
            continue;
        }
        add_with_deps(dep_item, by_id, platform, choices, selected)?;
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn visit(
    id: &str,
    by_id: &HashMap<&str, &StartkitItem>,
    platform: &str,
    choices: &StartkitChoices,
    selected: &HashSet<String>,
    temporary: &mut HashSet<String>,
    permanent: &mut HashSet<String>,
    ordered: &mut Vec<String>,
) -> anyhow::Result<()> {
    if permanent.contains(id) {
        return Ok(());
    }
    if !temporary.insert(id.to_string()) {
        bail!("cycle in startkit item dependencies at '{id}'");
    }
    let item = by_id
        .get(id)
        .ok_or_else(|| anyhow!("planned startkit item missing: {id}"))?;
    for dep in effective_item_dependencies(item, choices, platform) {
        if selected.contains(dep) {
            let dep_item = by_id.get(dep).ok_or_else(|| {
                anyhow!("startkit item '{}' depends on missing '{}'", item.id, dep)
            })?;
            if supports_platform(dep_item, platform) {
                visit(
                    dep, by_id, platform, choices, selected, temporary, permanent, ordered,
                )?;
            }
        }
    }
    temporary.remove(id);
    permanent.insert(id.to_string());
    ordered.push(id.to_string());
    Ok(())
}

pub(in crate::startkit) fn effective_item_dependencies<'a>(
    item: &'a StartkitItem,
    choices: &StartkitChoices,
    platform: &str,
) -> Vec<&'a str> {
    let mut deps = item
        .depends_on
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>();
    if item.id == "channels.plugins" && portable_toolchain_enabled(choices) && platform != "windows"
    {
        deps.retain(|dep| *dep != "essentials.git");
    }
    if let Some(agent_id) = super::agent_id_from_cli_item(&item.id) {
        if agent_detection::agent_uses_npm_install(agent_id) && !deps.contains(&"essentials.node") {
            deps.push("essentials.node");
        }
    }
    deps
}

fn should_include(item: &StartkitItem, choices: &StartkitChoices, platform: &str) -> bool {
    if item.id == "essentials.git" && portable_toolchain_enabled(choices) && platform != "windows" {
        return false;
    }
    item.include_if.iter().any(|rule| match rule.as_str() {
        "always" => true,
        "agent:any" => !choices.agents.is_empty(),
        "channels:any" => !choices.channels.is_empty(),
        "tunnel:any" => choices.tunnel != "none",
        "shell_path:true" => choices.shell_path,
        "toolchain:system" => !is_managed_mode(choices),
        "toolchain:managed" => is_managed_mode(choices),
        rule if rule.starts_with("managed-tunnel:") => {
            let tunnel = &rule["managed-tunnel:".len()..];
            is_managed_mode(choices) && choices.tunnel == tunnel
        }
        rule if rule.starts_with("agent:") => {
            let agent = &rule["agent:".len()..];
            choices.agents.iter().any(|id| id == agent)
        }
        rule if rule.starts_with("tunnel:") => {
            let tunnel = &rule["tunnel:".len()..];
            choices.tunnel == tunnel
        }
        _ => false,
    })
}

pub(in crate::startkit) fn is_managed_mode(choices: &StartkitChoices) -> bool {
    choices
        .toolchain_mode
        .trim()
        .eq_ignore_ascii_case("managed")
}

pub(in crate::startkit) fn portable_toolchain_enabled(choices: &StartkitChoices) -> bool {
    choices
        .portable_toolchain
        .unwrap_or_else(|| is_managed_mode(choices))
}

pub(in crate::startkit) fn supports_platform(item: &StartkitItem, platform: &str) -> bool {
    item.platforms.is_empty() || item.platforms.iter().any(|p| p == platform)
}

pub(in crate::startkit) fn find_item<'a>(
    manifest: &'a Manifest,
    id: &str,
) -> anyhow::Result<&'a StartkitItem> {
    manifest
        .items
        .iter()
        .find(|item| item.id == id)
        .ok_or_else(|| anyhow!("unknown startkit item: {id}"))
}
