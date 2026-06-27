use std::collections::{BTreeMap, HashSet};

use axum::{extract::Path, http::StatusCode, Json};
use common::agent_state;
use common::profiles::{
    catalog, normalize_legacy_profile_and_persist, runtime, schema, AuthMode, ProfileDef,
};
use serde::Deserialize;

/// GET /api/profiles -- list saved profiles and the CLI targets each can launch.
pub async fn list_profiles_handler() -> Json<Vec<crate::api_types::ProfileLaunchOption>> {
    let profile_connections = common::profiles::connections::merged_profile_connections();
    let profiles = common::profiles::ordered_profiles()
        .into_iter()
        .map(|profile| {
            let launch_targets =
                common::profiles::connections::launch_targets_for_profile_with_connections(
                    &profile,
                    &profile_connections,
                )
                .into_iter()
                .map(|target| crate::api_types::ProfileLaunchTarget {
                    id: target.id.to_string(),
                    label: target.label.to_string(),
                    api_type: target.api_type,
                    bridge_target_api_type: target.bridge_target_api_type,
                })
                .collect();
            crate::api_types::ProfileLaunchOption {
                id: profile.id,
                label: profile.label,
                provider: profile.provider,
                launch_targets,
            }
        })
        .collect();
    Json(profiles)
}

#[derive(Debug, Deserialize)]
pub struct ModelProfileDraft {
    pub label: String,
    pub provider: String,
    pub auth_mode: AuthMode,
    pub api_types: Vec<String>,
    #[serde(default)]
    pub credentials: BTreeMap<String, String>,
    #[serde(default)]
    pub overrides: BTreeMap<String, schema::ApiTypeOverrides>,
    #[serde(default)]
    pub use_settings_proxy: bool,
    #[serde(default)]
    pub provider_settings: schema::ProviderSettings,
    #[serde(default)]
    pub connections: BTreeMap<String, agent_state::ProfileConnectionPreference>,
}

#[derive(Debug, Deserialize)]
pub struct ProfileOrderBody {
    pub profile_ids: Vec<String>,
}

/// GET /api/model-profiles -- list full profile summaries without credentials.
pub async fn list_model_profiles_handler() -> Json<Vec<crate::api_types::ModelProfileSummary>> {
    Json(
        common::profiles::ordered_profiles()
            .into_iter()
            .map(model_profile_summary)
            .collect(),
    )
}

/// GET /api/model-profiles/:id -- return one full profile, including credentials.
pub async fn get_model_profile_handler(
    Path(id): Path<String>,
) -> Result<Json<ProfileDef>, (StatusCode, String)> {
    load_profile(&id).map(Json)
}

/// POST /api/model-profiles -- create a profile from a draft.
pub async fn create_model_profile_handler(
    Json(draft): Json<ModelProfileDraft>,
) -> Result<Json<ProfileDef>, (StatusCode, String)> {
    let id = schema::generate_unique_id(&draft.provider)
        .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;
    let profile = draft.into_profile(id);
    save_model_profile(&profile)?;
    ensure_profile_order_contains(&profile.id)?;
    Ok(Json(profile))
}

/// PUT /api/model-profiles/:id -- replace a profile definition.
pub async fn update_model_profile_handler(
    Path(id): Path<String>,
    Json(mut profile): Json<ProfileDef>,
) -> Result<Json<ProfileDef>, (StatusCode, String)> {
    if profile.id != id {
        return Err((
            StatusCode::BAD_REQUEST,
            format!("profile id mismatch: path '{id}' body '{}'", profile.id),
        ));
    }
    profile = normalize_legacy_profile_and_persist(profile);
    save_model_profile(&profile)?;
    ensure_profile_order_contains(&profile.id)?;
    Ok(Json(profile))
}

/// DELETE /api/model-profiles/:id -- delete a profile and clear references.
pub async fn delete_model_profile_handler(
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    schema::delete(&id).map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;
    clear_profile_references(&id)?;
    agent_state::remove_profile_references(&id)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(Json(serde_json::json!({ "deleted": id })))
}

/// PUT /api/model-profiles/order -- persist profile display order.
pub async fn reorder_model_profiles_handler(
    Json(body): Json<ProfileOrderBody>,
) -> Result<Json<Vec<crate::api_types::ModelProfileSummary>>, (StatusCode, String)> {
    reorder_profiles(body.profile_ids)?;
    Ok(Json(
        common::profiles::ordered_profiles()
            .into_iter()
            .map(model_profile_summary)
            .collect(),
    ))
}

impl ModelProfileDraft {
    fn into_profile(self, id: String) -> ProfileDef {
        ProfileDef {
            id,
            label: self.label,
            provider: self.provider,
            auth_mode: self.auth_mode,
            api_types: self.api_types,
            credentials: self.credentials,
            overrides: self.overrides,
            use_settings_proxy: self.use_settings_proxy,
            provider_settings: self.provider_settings,
            connections: self.connections,
        }
    }
}

fn load_profile(id: &str) -> Result<ProfileDef, (StatusCode, String)> {
    schema::load(id)
        .map(normalize_legacy_profile_and_persist)
        .ok_or_else(|| (StatusCode::NOT_FOUND, format!("profile '{id}' not found")))
}

fn save_model_profile(profile: &ProfileDef) -> Result<(), (StatusCode, String)> {
    validate_profile(profile)?;
    schema::save(profile).map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))
}

fn validate_profile(profile: &ProfileDef) -> Result<(), (StatusCode, String)> {
    schema::validate(profile).map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;
    let provider = catalog::get(&profile.provider).ok_or_else(|| {
        (
            StatusCode::BAD_REQUEST,
            format!("unknown provider '{}'", profile.provider),
        )
    })?;
    for api_type in &profile.api_types {
        let endpoint_id = profile
            .overrides
            .get(api_type)
            .and_then(|overrides| overrides.endpoint_id.as_deref());
        if catalog::find_endpoint(provider, api_type, endpoint_id).is_none() {
            let suffix = endpoint_id
                .map(|id| format!(" endpoint_id '{id}'"))
                .unwrap_or_default();
            return Err((
                StatusCode::BAD_REQUEST,
                format!(
                    "provider '{}' does not support api kind '{}'{}",
                    profile.provider, api_type, suffix
                ),
            ));
        }
    }
    Ok(())
}

fn reorder_profiles(profile_ids: Vec<String>) -> Result<(), (StatusCode, String)> {
    let profiles = common::profiles::ordered_profiles();
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

fn ensure_profile_order_contains(profile_id: &str) -> Result<(), (StatusCode, String)> {
    let mut order = read_profile_order();
    if !order.iter().any(|id| id == profile_id) {
        order.push(profile_id.to_string());
        write_profile_order(&order)?;
    }
    Ok(())
}

fn read_profile_order() -> Vec<String> {
    common::config::read_settings_json()
        .ok()
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

fn write_profile_order(profile_ids: &[String]) -> Result<(), (StatusCode, String)> {
    common::config::update_settings_json(|root| {
        if !root.is_object() {
            *root = serde_json::json!({});
        }
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
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))
}

fn clear_profile_references(profile_id: &str) -> Result<(), (StatusCode, String)> {
    common::config::update_settings_json(|root| {
        let Some(obj) = root.as_object_mut() else {
            return;
        };
        if let Some(order) = obj
            .get_mut("profile_order")
            .and_then(|value| value.as_array_mut())
        {
            order.retain(|value| value.as_str() != Some(profile_id));
            if order.is_empty() {
                obj.remove("profile_order");
            }
        }
    })
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))
}

fn model_profile_summary(profile: ProfileDef) -> crate::api_types::ModelProfileSummary {
    let provider = catalog::get(&profile.provider);
    let (provider_label, provider_icon) = match provider {
        Some(catalog) => (catalog.label.clone(), catalog.icon.clone()),
        None => (profile.provider.clone(), None),
    };
    let api_type_warnings = api_type_warnings(&profile, provider);
    let api_type_models = api_type_models(&profile, provider);
    let api_type_model_options = api_type_model_options(&profile, provider, &api_type_models);
    let api_type_headers = api_type_headers(&profile, provider);
    let warnings_for_targets = api_type_warnings.clone();

    crate::api_types::ModelProfileSummary {
        id: profile.id,
        label: profile.label,
        provider: profile.provider,
        provider_label,
        provider_icon,
        auth_mode: profile.auth_mode,
        launch_targets: runtime::launch_targets_for_api_types(&profile.api_types)
            .into_iter()
            .map(
                |(id, label, api_type)| crate::api_types::ModelProfileLaunchTarget {
                    id: id.to_string(),
                    label: label.to_string(),
                    api_type: api_type.to_string(),
                    warning: warnings_for_targets.get(api_type).cloned(),
                },
            )
            .collect(),
        api_types: profile.api_types,
        api_type_warnings,
        api_type_models,
        api_type_model_options,
        api_type_headers,
    }
}

fn api_type_warnings(
    profile: &ProfileDef,
    provider: Option<&'static catalog::ProviderCatalog>,
) -> BTreeMap<String, String> {
    let mut warnings = BTreeMap::new();
    let Some(provider) = provider else {
        return warnings;
    };
    for api_type in &profile.api_types {
        let endpoint_id = profile
            .overrides
            .get(api_type)
            .and_then(|overrides| overrides.endpoint_id.as_deref());
        if let Some(endpoint) = catalog::find_endpoint(provider, api_type, endpoint_id) {
            if let Some(warning) = &endpoint.compatibility_warning {
                warnings.insert(api_type.clone(), warning.clone());
            }
        }
    }
    warnings
}

fn api_type_models(
    profile: &ProfileDef,
    provider: Option<&'static catalog::ProviderCatalog>,
) -> BTreeMap<String, String> {
    profile
        .api_types
        .iter()
        .filter_map(|api_type| {
            let endpoint = endpoint_for(profile, provider, api_type);
            let model = profile
                .overrides
                .get(api_type)
                .and_then(|overrides| overrides.model.as_ref())
                .filter(|model| !model.trim().is_empty())
                .cloned()
                .or_else(|| {
                    endpoint
                        .and_then(|endpoint| endpoint.models.first())
                        .map(|model| model.id.clone())
                })?;
            Some((api_type.clone(), model))
        })
        .collect()
}

fn api_type_model_options(
    profile: &ProfileDef,
    provider: Option<&'static catalog::ProviderCatalog>,
    api_type_models: &BTreeMap<String, String>,
) -> BTreeMap<String, Vec<catalog::ModelDef>> {
    profile
        .api_types
        .iter()
        .filter_map(|api_type| {
            let mut models = endpoint_for(profile, provider, api_type)
                .map(|endpoint| endpoint.models.clone())
                .unwrap_or_default();
            if let Some(model) = profile
                .overrides
                .get(api_type)
                .and_then(|overrides| overrides.model.as_ref())
                .filter(|model| !model.trim().is_empty())
            {
                if !models.iter().any(|item| item.id == *model) {
                    models.insert(
                        0,
                        catalog::ModelDef {
                            id: model.clone(),
                            label: None,
                            aliases: Vec::new(),
                            context_window: None,
                            capabilities: Default::default(),
                        },
                    );
                }
            }
            if models.is_empty() {
                if let Some(model) = api_type_models.get(api_type) {
                    models.push(catalog::ModelDef {
                        id: model.clone(),
                        label: None,
                        aliases: Vec::new(),
                        context_window: None,
                        capabilities: Default::default(),
                    });
                }
            }
            (!models.is_empty()).then_some((api_type.clone(), models))
        })
        .collect()
}

fn api_type_headers(
    profile: &ProfileDef,
    provider: Option<&'static catalog::ProviderCatalog>,
) -> BTreeMap<String, BTreeMap<String, String>> {
    profile
        .api_types
        .iter()
        .filter_map(|api_type| {
            let headers = endpoint_for(profile, provider, api_type)?.headers.clone();
            (!headers.is_empty()).then_some((api_type.clone(), headers))
        })
        .collect()
}

fn endpoint_for<'a>(
    profile: &'a ProfileDef,
    provider: Option<&'a catalog::ProviderCatalog>,
    api_type: &str,
) -> Option<&'a catalog::EndpointDef> {
    provider.and_then(|catalog| {
        let endpoint_id = profile
            .overrides
            .get(api_type)
            .and_then(|overrides| overrides.endpoint_id.as_deref());
        catalog::find_endpoint(catalog, api_type, endpoint_id)
    })
}
