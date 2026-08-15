use std::collections::BTreeMap;

use axum::{extract::Path, http::StatusCode, Json};
use common::agent_state;
use common::profiles::{catalog, runtime, schema, AuthMode, ProfileDef, ProfileStoreError};
use serde::Deserialize;

/// GET /api/profiles -- list saved profiles and the CLI targets each can launch.
pub async fn list_profiles_handler(
) -> Result<Json<Vec<crate::api_types::ProfileLaunchOption>>, (StatusCode, String)> {
    super::run_blocking_io(|| {
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
        Ok(Json(profiles))
    })
    .await
}

#[derive(Debug, Deserialize)]
pub struct ModelProfileDraft {
    pub label: String,
    pub provider: String,
    pub auth_mode: AuthMode,
    #[serde(default)]
    pub credentials: BTreeMap<String, String>,
    #[serde(default)]
    pub api_configs: BTreeMap<String, schema::ProfileApiConfig>,
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
pub async fn list_model_profiles_handler(
) -> Result<Json<Vec<crate::api_types::ModelProfileSummary>>, (StatusCode, String)> {
    super::run_blocking_io(|| {
        Ok(Json(
            common::profiles::ordered_profiles()
                .into_iter()
                .map(model_profile_summary)
                .collect(),
        ))
    })
    .await
}

/// GET /api/model-profiles/:id -- return one full profile, including credentials.
pub async fn get_model_profile_handler(
    Path(id): Path<String>,
) -> Result<Json<ProfileDef>, (StatusCode, String)> {
    super::run_blocking_io(move || load_profile(&id).map(Json)).await
}

/// POST /api/model-profiles -- create a profile from a draft.
pub async fn create_model_profile_handler(
    Json(draft): Json<ModelProfileDraft>,
) -> Result<Json<ProfileDef>, (StatusCode, String)> {
    super::run_blocking_io(move || {
        let id = schema::generate_unique_id(&draft.provider)
            .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;
        let profile = draft.into_profile(id);
        common::profiles::save_profile(&profile).map_err(profile_store_error)?;
        Ok(Json(profile))
    })
    .await
}

/// PUT /api/model-profiles/:id -- replace a profile definition.
pub async fn update_model_profile_handler(
    Path(id): Path<String>,
    Json(draft): Json<ModelProfileDraft>,
) -> Result<Json<ProfileDef>, (StatusCode, String)> {
    super::run_blocking_io(move || {
        let profile = draft.into_profile(id);
        common::profiles::save_profile(&profile).map_err(profile_store_error)?;
        Ok(Json(profile))
    })
    .await
}

/// DELETE /api/model-profiles/:id -- delete a profile and clear references.
pub async fn delete_model_profile_handler(
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    super::run_blocking_io(move || {
        common::profiles::delete_profile(&id).map_err(profile_store_error)?;
        Ok(Json(serde_json::json!({ "deleted": id })))
    })
    .await
}

/// PUT /api/model-profiles/order -- persist profile display order.
pub async fn reorder_model_profiles_handler(
    Json(body): Json<ProfileOrderBody>,
) -> Result<Json<Vec<crate::api_types::ModelProfileSummary>>, (StatusCode, String)> {
    super::run_blocking_io(move || {
        common::profiles::reorder_profiles(&body.profile_ids).map_err(profile_store_error)?;
        Ok(Json(
            common::profiles::ordered_profiles()
                .into_iter()
                .map(model_profile_summary)
                .collect(),
        ))
    })
    .await
}

impl ModelProfileDraft {
    fn into_profile(self, id: String) -> ProfileDef {
        ProfileDef {
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

fn load_profile(id: &str) -> Result<ProfileDef, (StatusCode, String)> {
    common::profiles::load_profile(id)
        .ok_or_else(|| (StatusCode::NOT_FOUND, format!("profile '{id}' not found")))
}

fn profile_store_error(error: ProfileStoreError) -> (StatusCode, String) {
    match error {
        ProfileStoreError::Invalid(message) => (StatusCode::BAD_REQUEST, message),
        ProfileStoreError::Storage(message) => (StatusCode::INTERNAL_SERVER_ERROR, message),
    }
}

fn model_profile_summary(profile: ProfileDef) -> crate::api_types::ModelProfileSummary {
    let provider = catalog::get(&profile.provider);
    let enabled_api_types = schema::enabled_api_types(&profile);
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
        launch_targets: runtime::launch_targets_for_api_types(&enabled_api_types)
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
        api_types: enabled_api_types,
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
    for api_type in schema::enabled_api_types(profile) {
        let endpoint_id = selected_endpoint_id(profile, &api_type);
        if let Some(endpoint) = catalog::find_endpoint(provider, &api_type, endpoint_id.as_deref())
        {
            if let Some(warning) = &endpoint.compatibility_warning {
                warnings.insert(api_type, warning.clone());
            }
        }
    }
    warnings
}

fn api_type_models(
    profile: &ProfileDef,
    provider: Option<&'static catalog::ProviderCatalog>,
) -> BTreeMap<String, String> {
    schema::enabled_api_types(profile)
        .iter()
        .filter_map(|api_type| {
            let endpoint = endpoint_for(profile, provider, api_type);
            let config = api_config_for(profile, api_type);
            let model = config
                .as_ref()
                .and_then(|config| clean_string(config.model.as_deref()))
                .or_else(|| {
                    config.as_ref().and_then(|config| {
                        config
                            .models
                            .iter()
                            .filter(|model| model.enabled)
                            .find_map(|model| clean_string(Some(&model.id)))
                    })
                })
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
    schema::enabled_api_types(profile)
        .iter()
        .filter_map(|api_type| {
            let config = api_config_for(profile, api_type);
            let mut models = config
                .as_ref()
                .map(|config| {
                    config
                        .models
                        .iter()
                        .filter(|model| model.enabled)
                        .map(profile_model_to_catalog)
                        .collect::<Vec<_>>()
                })
                .filter(|models| !models.is_empty())
                .unwrap_or_else(|| {
                    endpoint_for(profile, provider, api_type)
                        .map(|endpoint| endpoint.models.clone())
                        .unwrap_or_default()
                });
            if let Some(model) = config
                .as_ref()
                .and_then(|config| clean_string(config.model.as_deref()))
            {
                if !models.iter().any(|item| item.id == model) {
                    models.insert(
                        0,
                        catalog::ModelDef {
                            id: model,
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
    schema::enabled_api_types(profile)
        .iter()
        .filter_map(|api_type| {
            let headers = api_config_for(profile, api_type)
                .map(|config| {
                    config
                        .headers
                        .into_iter()
                        .filter(|header| header.enabled)
                        .filter_map(|header| {
                            let name = header.name.trim().to_string();
                            (!name.is_empty()).then_some((name, header.value))
                        })
                        .collect::<BTreeMap<_, _>>()
                })
                .filter(|headers| !headers.is_empty())
                .or_else(|| {
                    endpoint_for(profile, provider, api_type)
                        .map(|endpoint| endpoint.headers.clone())
                })?;
            (!headers.is_empty()).then_some((api_type.clone(), headers))
        })
        .collect()
}

fn profile_model_to_catalog(model: &schema::ProfileModelConfig) -> catalog::ModelDef {
    catalog::ModelDef {
        id: model.id.clone(),
        label: model.label.clone(),
        aliases: Vec::new(),
        context_window: model.context_window,
        capabilities: model.capabilities.clone(),
    }
}

fn endpoint_for<'a>(
    profile: &'a ProfileDef,
    provider: Option<&'a catalog::ProviderCatalog>,
    api_type: &str,
) -> Option<&'a catalog::EndpointDef> {
    provider.and_then(|catalog| {
        let endpoint_id = selected_endpoint_id(profile, api_type);
        catalog::find_endpoint(catalog, api_type, endpoint_id.as_deref())
    })
}

fn api_config_for(profile: &ProfileDef, api_type: &str) -> Option<schema::ProfileApiConfig> {
    schema::api_config_for(profile, api_type).filter(|config| config.enabled)
}

fn selected_endpoint_id(profile: &ProfileDef, api_type: &str) -> Option<String> {
    api_config_for(profile, api_type).and_then(|config| config.endpoint_id)
}

fn clean_string(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}
