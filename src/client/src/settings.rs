use serde::Deserialize;
use serde_json::Value;

use crate::error::encode_body;
use crate::http::{AuthRequirement, HttpMethod, RequestSpec, ResponseSpec};
use crate::Result;

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct SettingsWriteResponse {
    pub ok: bool,
}

pub fn get() -> RequestSpec {
    RequestSpec::new(
        HttpMethod::Get,
        "/api/settings",
        AuthRequirement::BearerToken,
    )
}

pub fn decode_get(response: ResponseSpec) -> Result<Value> {
    response.decode()
}

pub fn put(settings: Value) -> Result<RequestSpec> {
    Ok(RequestSpec::new(
        HttpMethod::Put,
        "/api/settings",
        AuthRequirement::BearerToken,
    )
    .with_body(encode_body(settings)?))
}

pub fn decode_put(response: ResponseSpec) -> Result<SettingsWriteResponse> {
    response.decode()
}
