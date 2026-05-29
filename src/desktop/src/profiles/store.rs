//! Profile CRUD and ordering.

use std::collections::{BTreeMap, HashSet};

use common::agent_state;
use common::profiles::schema::{ApiTypeOverrides, ProviderSettings};
use common::profiles::{catalog, normalize_legacy_profile_and_persist, schema};
use common::{config, profiles::AuthMode};
use serde::Deserialize;

use super::terminal;

#[derive(Debug, Deserialize)]
pub struct ProfileDraft {
    pub label: String,
    pub provider: String,
    pub auth_mode: AuthMode,
    pub api_types: Vec<String>,
    #[serde(default)]
    pub credentials: BTreeMap<String, String>,
    #[serde(default)]
    pub overrides: BTreeMap<String, ApiTypeOverrides>,
    #[serde(default)]
    pub use_settings_proxy: bool,
    #[serde(default)]
    pub provider_settings: ProviderSettings,
}

impl ProfileDraft {
    fn into_profile(self, id: String) -> schema::ProfileDef {
        schema::ProfileDef {
            id,
            label: self.label,
            provider: self.provider,
            auth_mode: self.auth_mode,
            api_types: self.api_types,
            credentials: self.credentials,
            overrides: self.overrides,
            use_settings_proxy: self.use_settings_proxy,
            provider_settings: self.provider_settings,
        }
    }
}

pub(super) fn get_profile(id: &str) -> Result<schema::ProfileDef, String> {
    schema::load(id)
        .map(normalize_legacy_profile_and_persist)
        .ok_or_else(|| format!("profile '{id}' not found"))
}

pub(super) fn create_profile(draft: ProfileDraft) -> Result<schema::ProfileDef, String> {
    let id = schema::generate_unique_id(&draft.provider).map_err(|e| e.to_string())?;
    let profile = draft.into_profile(id);
    save_profile(&profile)?;
    Ok(profile)
}

pub(super) fn save_profile(profile: &schema::ProfileDef) -> Result<(), String> {
    schema::validate(profile).map_err(|e| e.to_string())?;
    let provider = catalog::get(&profile.provider)
        .ok_or_else(|| format!("unknown provider '{}'", profile.provider))?;
    for api_type in &profile.api_types {
        let endpoint_id = profile
            .overrides
            .get(api_type)
            .and_then(|overrides| overrides.endpoint_id.as_deref());
        if catalog::find_endpoint(provider, api_type, endpoint_id).is_none() {
            let suffix = endpoint_id
                .map(|id| format!(" endpoint_id '{id}'"))
                .unwrap_or_default();
            return Err(format!(
                "provider '{}' does not support api kind '{}'{}",
                profile.provider, api_type, suffix
            ));
        }
    }
    schema::save(profile).map_err(|e| e.to_string())?;
    ensure_profile_order_contains(&profile.id)
}

pub(super) fn delete_profile(id: &str) -> Result<(), String> {
    schema::delete(id).map_err(|e| e.to_string())?;
    clear_default_profile_references(id)?;
    agent_state::remove_profile_references(id).map_err(|e| e.to_string())?;
    terminal::remove_profile_connections(id).map_err(|e| e.to_string())
}

pub(super) fn reorder_profiles(profile_ids: Vec<String>) -> Result<(), String> {
    let profiles: Vec<_> = schema::list()
        .into_iter()
        .map(normalize_legacy_profile_and_persist)
        .collect();
    let existing_ids: HashSet<_> = profiles.iter().map(|profile| profile.id.as_str()).collect();
    let mut seen = HashSet::new();
    let mut ordered_ids = Vec::new();

    for id in profile_ids {
        let id = id.trim();
        if existing_ids.contains(id) && seen.insert(id.to_string()) {
            ordered_ids.push(id.to_string());
        }
    }

    for profile in profiles {
        if seen.insert(profile.id.clone()) {
            ordered_ids.push(profile.id);
        }
    }

    write_profile_order(&ordered_ids)
}

pub(crate) fn ordered_profiles() -> Vec<schema::ProfileDef> {
    common::profiles::ordered_profiles()
}

fn clear_default_profile_references(profile_id: &str) -> Result<(), String> {
    config::update_settings_json(|root| {
        if let Some(obj) = root.as_object_mut() {
            let mut remove_default_profiles = false;
            if let Some(map) = obj
                .get_mut("default_profiles")
                .and_then(|value| value.as_object_mut())
            {
                map.retain(|_, value| value.as_str() != Some(profile_id));
                remove_default_profiles = map.is_empty();
            }
            if remove_default_profiles {
                obj.remove("default_profiles");
            }
            if let Some(order) = obj
                .get_mut("profile_order")
                .and_then(|value| value.as_array_mut())
            {
                order.retain(|value| value.as_str() != Some(profile_id));
                if order.is_empty() {
                    obj.remove("profile_order");
                }
            }
        }
    })
    .map_err(|e| e.to_string())
}

fn read_profile_order() -> Vec<String> {
    let path = config::data_dir().join("settings.json");
    std::fs::read_to_string(path)
        .ok()
        .and_then(|data| serde_json::from_str::<serde_json::Value>(&data).ok())
        .and_then(|root| {
            root.get("profile_order")
                .and_then(|value| value.as_array())
                .map(|items| {
                    items
                        .iter()
                        .filter_map(|item| item.as_str())
                        .map(str::trim)
                        .filter(|id| !id.is_empty())
                        .map(ToOwned::to_owned)
                        .collect()
                })
        })
        .unwrap_or_default()
}

fn write_profile_order(profile_ids: &[String]) -> Result<(), String> {
    config::update_settings_json(|root| {
        if let Some(obj) = root.as_object_mut() {
            obj.insert(
                "profile_order".to_string(),
                serde_json::Value::Array(
                    profile_ids
                        .iter()
                        .map(|id| serde_json::Value::String(id.clone()))
                        .collect(),
                ),
            );
        }
    })
    .map_err(|e| e.to_string())
}

fn ensure_profile_order_contains(profile_id: &str) -> Result<(), String> {
    let mut order = read_profile_order();
    if !order.iter().any(|id| id == profile_id) {
        order.push(profile_id.to_string());
        write_profile_order(&order)?;
    }
    Ok(())
}
