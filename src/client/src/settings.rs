use serde::Deserialize;
use serde_json::Value;

use crate::error::{encode_body, ClientError};
use crate::http::{AuthRequirement, HttpMethod, RequestSpec, ResponseSpec};
use crate::Result;

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct SettingsWriteResponse {
    pub ok: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SettingsSnapshot {
    pub settings: Value,
    pub revision: String,
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

pub fn decode_snapshot(response: ResponseSpec) -> Result<SettingsSnapshot> {
    response.ensure_success()?;
    let revision = response
        .header("etag")
        .and_then(strong_etag_revision)
        .ok_or_else(|| ClientError::Protocol("settings response is missing ETag".into()))?
        .to_string();
    let settings = response.decode()?;
    Ok(SettingsSnapshot { settings, revision })
}

pub fn replace(settings: Value, revision: &str) -> Result<RequestSpec> {
    Ok(RequestSpec::new(
        HttpMethod::Put,
        "/api/settings",
        AuthRequirement::BearerToken,
    )
    .with_header("If-Match", format!("\"{revision}\""))
    .with_body(encode_body(settings)?))
}

pub fn patch(patch: Value) -> Result<RequestSpec> {
    Ok(RequestSpec::new(
        HttpMethod::Patch,
        "/api/settings",
        AuthRequirement::BearerToken,
    )
    .with_body(encode_body(patch)?))
}

pub fn decode_write(response: ResponseSpec) -> Result<SettingsWriteResponse> {
    response.decode()
}

fn strong_etag_revision(value: &str) -> Option<&str> {
    value
        .strip_prefix('"')
        .and_then(|value| value.strip_suffix('"'))
        .filter(|value| value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_snapshot_reads_revision_from_etag() {
        let revision = "a".repeat(64);
        let snapshot = decode_snapshot(ResponseSpec::json_with_headers(
            200,
            serde_json::json!({ "onboarded": true }),
            vec![("ETag".into(), format!("\"{revision}\""))],
        ))
        .unwrap();

        assert_eq!(snapshot.revision, revision);
        assert_eq!(snapshot.settings["onboarded"], true);
    }

    #[test]
    fn settings_snapshot_reports_http_error_before_missing_etag() {
        let error = decode_snapshot(ResponseSpec::json(
            401,
            serde_json::json!({ "error": "unauthorized" }),
        ))
        .unwrap_err();

        assert!(matches!(
            error,
            ClientError::UnexpectedStatus { status: 401, .. }
        ));
    }

    #[test]
    fn replace_carries_strong_if_match_header() {
        let revision = "b".repeat(64);
        let request = replace(serde_json::json!({}), &revision).unwrap();
        assert_eq!(request.method, HttpMethod::Put);
        assert_eq!(
            request.headers,
            vec![("If-Match".into(), format!("\"{revision}\""))]
        );
    }

    #[test]
    fn patch_uses_patch_method() {
        let request = patch(serde_json::json!({ "onboarded": true })).unwrap();
        assert_eq!(request.method, HttpMethod::Patch);
    }
}
