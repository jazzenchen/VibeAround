use crate::operation::Operation;
use crate::profiles::{ModelProfileDraft, ModelProfileSummary, ProfileDef, ProfileLaunchOption};
use crate::Result;

use super::decode_success;

pub fn profile_launch_options() -> Operation<Vec<ProfileLaunchOption>> {
    Operation::new(
        crate::profiles::list_launch_options(),
        crate::profiles::decode_launch_options,
    )
}

pub fn model_profiles() -> Operation<Vec<ModelProfileSummary>> {
    Operation::new(
        crate::profiles::list_model_profiles(),
        crate::profiles::decode_model_profiles,
    )
}

pub fn model_profile(id: &str) -> Operation<ProfileDef> {
    Operation::new(
        crate::profiles::get_model_profile(id),
        crate::profiles::decode_model_profile,
    )
}

pub fn model_profile_create(draft: &ModelProfileDraft) -> Result<Operation<ProfileDef>> {
    Ok(Operation::new(
        crate::profiles::create_model_profile(draft)?,
        crate::profiles::decode_model_profile,
    ))
}

pub fn model_profile_update(id: &str, profile: &ProfileDef) -> Result<Operation<ProfileDef>> {
    Ok(Operation::new(
        crate::profiles::update_model_profile(id, profile)?,
        crate::profiles::decode_model_profile,
    ))
}

pub fn model_profile_delete(id: &str) -> Operation<()> {
    Operation::new(crate::profiles::delete_model_profile(id), decode_success)
}

pub fn model_profiles_reorder(profile_ids: &[&str]) -> Result<Operation<Vec<ModelProfileSummary>>> {
    Ok(Operation::new(
        crate::profiles::reorder_model_profiles(profile_ids)?,
        crate::profiles::decode_model_profiles,
    ))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::http::HttpMethod;

    #[test]
    fn dynamic_model_profile_operation_encodes_path() {
        let op = model_profile("openai/default");
        assert_eq!(op.request().path, "/api/model-profiles/openai%2Fdefault");
    }

    #[test]
    fn model_profile_reorder_decodes_summaries() {
        let op = model_profiles_reorder(&["p2", "p1"]).expect("operation");
        assert_eq!(op.request().method, HttpMethod::Put);
        assert_eq!(op.request().path, "/api/model-profiles/order");
        assert_eq!(
            op.request().body,
            Some(json!({ "profile_ids": ["p2", "p1"] }))
        );
    }
}
