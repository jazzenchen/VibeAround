use serde::de::DeserializeOwned;
use serde_json::Value;

use crate::http::ResponseSpec;

pub type Result<T> = std::result::Result<T, ClientError>;

#[derive(Debug, thiserror::Error)]
pub enum ClientError {
    #[error("failed to encode request body: {0}")]
    Encode(#[source] serde_json::Error),
    #[error("failed to decode response body: {0}")]
    Decode(#[source] serde_json::Error),
    #[error("server returned HTTP {status}: {body}")]
    UnexpectedStatus { status: u16, body: Value },
}

pub(crate) fn encode_body<T: serde::Serialize>(value: T) -> Result<Value> {
    serde_json::to_value(value).map_err(ClientError::Encode)
}

pub(crate) fn decode_json<T: DeserializeOwned>(response: ResponseSpec) -> Result<T> {
    ensure_success(response.status, &response.body)?;
    serde_json::from_value(response.body).map_err(ClientError::Decode)
}

pub(crate) fn ensure_success(status: u16, body: &Value) -> Result<()> {
    if (200..300).contains(&status) {
        Ok(())
    } else {
        Err(ClientError::UnexpectedStatus {
            status,
            body: body.clone(),
        })
    }
}
