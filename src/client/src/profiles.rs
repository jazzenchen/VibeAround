use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::error::encode_body;
use crate::http::{join_path, AuthRequirement, HttpMethod, RequestSpec, ResponseSpec};
use crate::Result;

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct ProfileLaunchTarget {
    pub id: String,
    pub label: String,
    pub api_type: String,
    pub bridge_target_api_type: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct ProfileLaunchOption {
    pub id: String,
    pub label: String,
    pub provider: String,
    pub launch_targets: Vec<ProfileLaunchTarget>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AuthMode {
    ApiKey,
    OauthViaCli,
    GoogleOauth,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelProfileSummary {
    pub id: String,
    pub label: String,
    pub provider: String,
    pub provider_label: String,
    pub provider_icon: Option<String>,
    pub auth_mode: AuthMode,
    pub api_types: Vec<String>,
    pub launch_targets: Vec<ModelProfileLaunchTarget>,
    pub api_type_warnings: BTreeMap<String, String>,
    pub api_type_models: BTreeMap<String, String>,
    pub api_type_model_options: BTreeMap<String, Vec<Value>>,
    pub api_type_headers: BTreeMap<String, BTreeMap<String, String>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelProfileLaunchTarget {
    pub id: String,
    pub label: String,
    pub api_type: String,
    pub warning: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProfileDef {
    pub id: String,
    pub label: String,
    pub provider: String,
    pub auth_mode: AuthMode,
    #[serde(default)]
    pub credentials: BTreeMap<String, String>,
    #[serde(default)]
    pub api_configs: BTreeMap<String, Value>,
    #[serde(default)]
    pub use_settings_proxy: bool,
    #[serde(default)]
    pub provider_settings: Value,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ModelProfileDraft {
    pub label: String,
    pub provider: String,
    pub auth_mode: AuthMode,
    #[serde(default)]
    pub credentials: BTreeMap<String, String>,
    #[serde(default)]
    pub api_configs: BTreeMap<String, Value>,
    #[serde(default)]
    pub use_settings_proxy: bool,
    #[serde(default)]
    pub provider_settings: Value,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct ProfileOrderBody<'a> {
    profile_ids: &'a [&'a str],
}

pub fn list_launch_options() -> RequestSpec {
    RequestSpec::new(
        HttpMethod::Get,
        "/api/profiles",
        AuthRequirement::BearerToken,
    )
}

pub fn decode_launch_options(response: ResponseSpec) -> Result<Vec<ProfileLaunchOption>> {
    response.decode()
}

pub fn list_model_profiles() -> RequestSpec {
    RequestSpec::new(
        HttpMethod::Get,
        "/api/model-profiles",
        AuthRequirement::BearerToken,
    )
}

pub fn decode_model_profiles(response: ResponseSpec) -> Result<Vec<ModelProfileSummary>> {
    response.decode()
}

pub fn get_model_profile(id: &str) -> RequestSpec {
    RequestSpec::new(
        HttpMethod::Get,
        join_path("/api/model-profiles", id),
        AuthRequirement::BearerToken,
    )
}

pub fn decode_model_profile(response: ResponseSpec) -> Result<ProfileDef> {
    response.decode()
}

pub fn create_model_profile(draft: &ModelProfileDraft) -> Result<RequestSpec> {
    Ok(RequestSpec::new(
        HttpMethod::Post,
        "/api/model-profiles",
        AuthRequirement::BearerToken,
    )
    .with_body(encode_body(draft)?))
}

pub fn update_model_profile(id: &str, draft: &ModelProfileDraft) -> Result<RequestSpec> {
    Ok(RequestSpec::new(
        HttpMethod::Put,
        join_path("/api/model-profiles", id),
        AuthRequirement::BearerToken,
    )
    .with_body(encode_body(draft)?))
}

pub fn delete_model_profile(id: &str) -> RequestSpec {
    RequestSpec::new(
        HttpMethod::Delete,
        join_path("/api/model-profiles", id),
        AuthRequirement::BearerToken,
    )
}

pub fn reorder_model_profiles(profile_ids: &[&str]) -> Result<RequestSpec> {
    Ok(RequestSpec::new(
        HttpMethod::Put,
        "/api/model-profiles/order",
        AuthRequirement::BearerToken,
    )
    .with_body(encode_body(ProfileOrderBody { profile_ids })?))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn model_profile_path_encodes_id() {
        let request = get_model_profile("openai/default");
        assert_eq!(request.path, "/api/model-profiles/openai%2Fdefault");
    }

    #[test]
    fn reorder_profiles_uses_profile_ids_body() {
        let request = reorder_model_profiles(&["p1", "p2"]).expect("request");
        assert_eq!(request.method, HttpMethod::Put);
        assert_eq!(request.path, "/api/model-profiles/order");
        assert_eq!(request.body, Some(json!({ "profile_ids": ["p1", "p2"] })));
    }

    #[test]
    fn model_profile_writes_only_canonical_api_configs() {
        let draft = ModelProfileDraft {
            label: "OpenAI".to_string(),
            provider: "openai".to_string(),
            auth_mode: AuthMode::ApiKey,
            credentials: BTreeMap::new(),
            api_configs: [(
                "openai-responses".to_string(),
                json!({ "enabled": true, "model": "gpt-5" }),
            )]
            .into_iter()
            .collect(),
            use_settings_proxy: false,
            provider_settings: Value::Null,
        };

        let request = update_model_profile("openai", &draft).expect("request");
        let body = request.body.expect("body");
        assert!(body.get("api_configs").is_some());
        assert!(body.get("api_types").is_none());
        assert!(body.get("overrides").is_none());
    }
}
