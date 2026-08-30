//! Profile summaries sent to the Launch UI.

use std::collections::BTreeMap;

use common::profiles::{catalog, runtime, schema, AuthMode, ProfileDef};
use serde::Serialize;

/// List item — does NOT include credentials. Used to render the Launch tab
/// without ever shipping API keys to the webview after the initial save.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProfileSummary {
    pub id: String,
    pub label: String,
    pub provider: String,
    /// Provider's display label, resolved from the catalog. Falls back to
    /// the raw provider id when the catalog entry is missing.
    pub provider_label: String,
    pub provider_icon: Option<String>,
    pub auth_mode: AuthMode,
    /// API kinds this provider credential declares, e.g. `anthropic`,
    /// `openai-chat`, `gemini`.
    pub api_types: Vec<String>,
    /// Concrete CLI buttons the Launch tab should render.
    pub launch_targets: Vec<LaunchTargetSummary>,
    /// `api_type -> model id`, sanitized for manual client setup.
    pub api_type_models: BTreeMap<String, String>,
    /// `api_type -> catalog model options`, used by bridge route model selection.
    pub api_type_model_options: BTreeMap<String, Vec<catalog::ModelDef>>,
    /// `api_type -> provider catalog headers`, displayed as immutable defaults
    /// in bridge route settings.
    pub api_type_headers: BTreeMap<String, BTreeMap<String, String>>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LaunchTargetSummary {
    pub id: String,
    pub label: String,
    pub api_type: String,
}

/// Catalog entry sent to the UI. Nested catalog types intentionally keep their
/// own wire casing so frontend template keys stay consistent end-to-end.
#[derive(Debug, Serialize)]
pub struct CatalogEntry {
    pub id: String,
    pub label: String,
    pub icon: Option<String>,
    pub homepage: Option<String>,
    pub endpoints: Vec<catalog::EndpointDef>,
}

pub(super) fn profile_summaries() -> Vec<ProfileSummary> {
    super::store::ordered_profiles()
        .into_iter()
        .map(profile_summary)
        .collect()
}

pub(super) fn catalog_entries() -> Vec<CatalogEntry> {
    let mut entries: Vec<_> = catalog::all()
        .iter()
        .filter(|c| !c.hidden_from_picker)
        .map(|c| CatalogEntry {
            id: c.id.clone(),
            label: c.label.clone(),
            icon: c.icon.clone(),
            homepage: c.homepage.clone(),
            endpoints: c.endpoints.clone(),
        })
        .collect();
    entries.sort_by(|a, b| {
        a.label
            .to_ascii_lowercase()
            .cmp(&b.label.to_ascii_lowercase())
            .then_with(|| a.id.cmp(&b.id))
    });
    entries
}

fn profile_summary(profile: ProfileDef) -> ProfileSummary {
    let provider = catalog::get(&profile.provider);
    let enabled_api_types = schema::enabled_api_types(&profile);
    let (label, icon) = match provider {
        Some(catalog) => (catalog.label.clone(), catalog.icon.clone()),
        None => (profile.provider.clone(), None),
    };
    let api_type_models = api_type_models(&profile, provider);
    let api_type_model_options = api_type_model_options(&profile, provider, &api_type_models);
    let api_type_headers = api_type_headers(&profile, provider);

    ProfileSummary {
        id: profile.id,
        label: profile.label,
        provider: profile.provider,
        provider_label: label,
        provider_icon: icon,
        auth_mode: profile.auth_mode,
        launch_targets: runtime::launch_targets_for_api_types(&enabled_api_types)
            .into_iter()
            .map(|(id, label, api_type)| LaunchTargetSummary {
                id: id.to_string(),
                label: label.to_string(),
                api_type: api_type.to_string(),
            })
            .collect(),
        api_types: enabled_api_types,
        api_type_models,
        api_type_model_options,
        api_type_headers,
    }
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
            let headers = endpoint_for(profile, provider, api_type)
                .map(|endpoint| endpoint.headers.clone())?;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_entries_are_sorted_by_label() {
        let entries = catalog_entries();
        let labels: Vec<_> = entries
            .iter()
            .map(|entry| entry.label.to_ascii_lowercase())
            .collect();
        let mut sorted = labels.clone();
        sorted.sort();

        assert_eq!(labels, sorted);
    }
}
