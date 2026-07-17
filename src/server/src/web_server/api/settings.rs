//! Runtime settings API.

use axum::http::header::{ETAG, IF_MATCH};
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;

#[derive(Debug)]
pub struct SettingsApiError(Box<Response>);

impl SettingsApiError {
    fn new(response: impl IntoResponse) -> Self {
        Self(Box::new(response.into_response()))
    }
}

impl IntoResponse for SettingsApiError {
    fn into_response(self) -> Response {
        *self.0
    }
}

type ApiResult = Result<Response, SettingsApiError>;

/// GET /api/settings -- return raw settings.json with its current ETag.
pub async fn get_settings_handler() -> ApiResult {
    let snapshot = tokio::task::spawn_blocking(common::config::read_settings_snapshot)
        .await
        .map_err(internal_join_error)?
        .map_err(internal_error)?;
    response_with_etag(Json(snapshot.settings).into_response(), &snapshot.revision)
}

/// PUT /api/settings -- replace settings.json if the supplied ETag is current.
pub async fn put_settings_handler(
    headers: HeaderMap,
    Json(settings): Json<serde_json::Value>,
) -> ApiResult {
    if !settings.is_object() {
        return Err(SettingsApiError::new((
            StatusCode::BAD_REQUEST,
            "settings body must be a JSON object",
        )));
    }
    let expected_revision = parse_if_match(&headers)?;
    let result = tokio::task::spawn_blocking(move || {
        let result =
            common::config::replace_settings_json_if_revision(&expected_revision, &settings)?;
        if matches!(result, common::config::SettingsReplaceResult::Replaced(_)) {
            common::config::reload();
        }
        Ok::<_, String>(result)
    })
    .await
    .map_err(internal_join_error)?
    .map_err(internal_error)?;

    match result {
        common::config::SettingsReplaceResult::Replaced(snapshot) => response_with_etag(
            Json(crate::api_types::SettingsWriteResponse { ok: true }).into_response(),
            &snapshot.revision,
        ),
        common::config::SettingsReplaceResult::Conflict(snapshot) => {
            let response = (
                StatusCode::PRECONDITION_FAILED,
                "settings changed since the supplied If-Match revision",
            )
                .into_response();
            let response = response_with_etag(response, &snapshot.revision)?;
            Err(SettingsApiError::new(response))
        }
    }
}

/// PATCH /api/settings -- atomically apply an RFC 6902 JSON Patch.
pub async fn patch_settings_handler(Json(patch): Json<serde_json::Value>) -> ApiResult {
    let snapshot = tokio::task::spawn_blocking(move || {
        let snapshot = common::config::patch_settings_json(&patch)?;
        common::config::reload();
        Ok::<_, String>(snapshot)
    })
    .await
    .map_err(internal_join_error)?
    .map_err(|error| match error.as_str() {
        error if error.starts_with("invalid settings patch:") => {
            SettingsApiError::new((StatusCode::BAD_REQUEST, error.to_string()))
        }
        error if error.starts_with("settings patch conflict:") => {
            SettingsApiError::new((StatusCode::CONFLICT, error.to_string()))
        }
        _ => internal_error(error),
    })?;

    response_with_etag(
        Json(crate::api_types::SettingsWriteResponse { ok: true }).into_response(),
        &snapshot.revision,
    )
}

fn parse_if_match(headers: &HeaderMap) -> Result<String, SettingsApiError> {
    let raw = headers.get(IF_MATCH).ok_or_else(|| {
        SettingsApiError::new((
            StatusCode::PRECONDITION_REQUIRED,
            "PUT /api/settings requires If-Match",
        ))
    })?;
    let raw = raw
        .to_str()
        .map_err(|_| SettingsApiError::new((StatusCode::BAD_REQUEST, "invalid If-Match header")))?;
    let revision = raw
        .strip_prefix('"')
        .and_then(|value| value.strip_suffix('"'))
        .filter(|value| value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit()))
        .ok_or_else(|| {
            SettingsApiError::new((StatusCode::BAD_REQUEST, "invalid If-Match header"))
        })?;
    Ok(revision.to_ascii_lowercase())
}

fn response_with_etag(mut response: Response, revision: &str) -> ApiResult {
    let value = HeaderValue::from_str(&format!("\"{revision}\""))
        .map_err(|_| internal_error("invalid settings revision".to_string()))?;
    response.headers_mut().insert(ETAG, value);
    Ok(response)
}

fn internal_error(error: String) -> SettingsApiError {
    SettingsApiError::new((StatusCode::INTERNAL_SERVER_ERROR, error))
}

fn internal_join_error(error: tokio::task::JoinError) -> SettingsApiError {
    internal_error(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn if_match_requires_one_strong_sha256_etag() {
        let revision = "a".repeat(64);
        let mut headers = HeaderMap::new();
        headers.insert(
            IF_MATCH,
            HeaderValue::from_str(&format!("\"{revision}\"")).unwrap(),
        );
        assert_eq!(parse_if_match(&headers).unwrap(), revision);

        for invalid in [revision.as_str(), "*", "W/\"abc\"", "\"abc\""] {
            let mut headers = HeaderMap::new();
            headers.insert(IF_MATCH, HeaderValue::from_str(invalid).unwrap());
            assert!(parse_if_match(&headers).is_err());
        }
        assert!(parse_if_match(&HeaderMap::new()).is_err());
    }
}
