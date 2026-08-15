//! Profile CRUD and ordering.

use std::collections::BTreeMap;

use common::agent_state;
use common::profiles::schema::{ProfileApiConfig, ProviderSettings};
use common::profiles::{self, schema, AuthMode};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct ProfileDraft {
    pub label: String,
    pub provider: String,
    pub auth_mode: AuthMode,
    #[serde(default)]
    pub credentials: BTreeMap<String, String>,
    #[serde(default)]
    pub api_configs: BTreeMap<String, ProfileApiConfig>,
    #[serde(default)]
    pub use_settings_proxy: bool,
    #[serde(default)]
    pub provider_settings: ProviderSettings,
    #[serde(default)]
    pub connections: BTreeMap<String, agent_state::ProfileConnectionPreference>,
}

impl ProfileDraft {
    pub(super) fn into_profile(self, id: String) -> schema::ProfileDef {
        schema::ProfileDef {
            id,
            label: self.label,
            provider: self.provider,
            auth_mode: self.auth_mode,
            credentials: self.credentials,
            api_configs: self.api_configs,
            use_settings_proxy: self.use_settings_proxy,
            provider_settings: self.provider_settings,
            connections: self.connections,
        }
    }
}

pub(super) fn get_profile(id: &str) -> Result<schema::ProfileDef, String> {
    profiles::load_profile(id).ok_or_else(|| format!("profile '{id}' not found"))
}

pub(super) fn create_profile(draft: ProfileDraft) -> Result<schema::ProfileDef, String> {
    let id = schema::generate_unique_id(&draft.provider).map_err(|e| e.to_string())?;
    let profile = draft.into_profile(id);
    save_profile(&profile)?;
    Ok(profile)
}

pub(super) fn save_profile(profile: &schema::ProfileDef) -> Result<(), String> {
    profiles::save_profile(profile).map_err(|error| error.to_string())
}

pub(super) fn delete_profile(id: &str) -> Result<(), String> {
    profiles::delete_profile(id).map_err(|error| error.to_string())
}

pub(super) fn reorder_profiles(profile_ids: Vec<String>) -> Result<(), String> {
    profiles::reorder_profiles(&profile_ids).map_err(|error| error.to_string())
}

pub(crate) fn ordered_profiles() -> Vec<schema::ProfileDef> {
    common::profiles::ordered_profiles()
}
