use std::io;

use serde_json::Value;
use va_client::endpoint::ServerEndpoint;
use va_client::http::{AuthRequirement, HttpMethod, RequestSpec, ResponseSpec};
use va_client::Operation;

#[derive(Debug, thiserror::Error)]
pub(crate) enum TuiError {
    #[error("{0}")]
    Usage(String),
    #[error("auth is required; pass --token or start VibeAround so auth.json exists at {0}")]
    MissingAuth(String),
    #[error("failed to read auth file {path}: {source}")]
    ReadAuth {
        path: String,
        #[source]
        source: io::Error,
    },
    #[error("failed to reach {url}: {source}")]
    Http {
        url: String,
        #[source]
        source: reqwest::Error,
    },
    #[error("I/O error while {action}: {source}")]
    Io {
        action: &'static str,
        #[source]
        source: io::Error,
    },
    #[error("client error: {0}")]
    Client(#[from] va_client::ClientError),
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
}

pub(crate) struct HttpTransport {
    endpoint: std::sync::RwLock<ServerEndpoint>,
    /// Where the current token came from, when it came from a file the daemon
    /// rewrites on every start.
    auth_file: Option<std::path::PathBuf>,
    client: reqwest::Client,
}

impl HttpTransport {
    pub(crate) fn new(endpoint: ServerEndpoint) -> Self {
        Self {
            endpoint: std::sync::RwLock::new(endpoint),
            auth_file: None,
            client: reqwest::Client::new(),
        }
    }

    /// Re-read this auth file and retry once when the daemon answers 401.
    pub(crate) fn refreshing_from(mut self, auth_file: Option<std::path::PathBuf>) -> Self {
        self.auth_file = auth_file;
        self
    }

    fn endpoint(&self) -> ServerEndpoint {
        self.endpoint
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }

    /// Pick up a token the daemon minted after we started. Returns whether the
    /// stored credential actually changed, so a genuine 401 is not retried.
    fn refresh_token(&self) -> bool {
        let Some(path) = self.auth_file.as_deref() else {
            return false;
        };
        let Ok(body) = std::fs::read_to_string(path) else {
            return false;
        };
        let Ok(auth) = va_client::auth::parse_auth_file(&body) else {
            return false;
        };
        let mut endpoint = self
            .endpoint
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if endpoint.token() == Some(auth.token.as_str()) {
            return false;
        }
        *endpoint = endpoint.clone().with_token(auth.token);
        true
    }

    pub(crate) async fn execute<T>(&self, operation: Operation<T>) -> Result<T, TuiError> {
        let request = operation.request().clone();
        let response = self.send(request).await?;
        Ok(operation.decode(response)?)
    }

    async fn send(&self, request: RequestSpec) -> Result<ResponseSpec, TuiError> {
        let response = self.send_once(&request).await?;
        // The daemon rotates its token on every start, so a 401 on an
        // authenticated call usually means it restarted under us.
        if response.status != 401 || request.auth != AuthRequirement::BearerToken {
            return Ok(response);
        }
        if !self.refresh_token() {
            return Ok(response);
        }
        self.send_once(&request).await
    }

    async fn send_once(&self, request: &RequestSpec) -> Result<ResponseSpec, TuiError> {
        let endpoint = self.endpoint();
        let method = match request.method {
            HttpMethod::Get => reqwest::Method::GET,
            HttpMethod::Post => reqwest::Method::POST,
            HttpMethod::Put => reqwest::Method::PUT,
            HttpMethod::Patch => reqwest::Method::PATCH,
            HttpMethod::Delete => reqwest::Method::DELETE,
        };
        let url = endpoint.http_url(request);
        let mut builder = self.client.request(method, &url);
        if let Some(auth) = endpoint.authorization_header(request) {
            builder = builder.header(reqwest::header::AUTHORIZATION, auth);
        }
        for (name, value) in &request.headers {
            builder = builder.header(name.as_str(), value.as_str());
        }
        if let Some(body) = &request.body {
            builder = builder.json(body);
        }

        let response = builder.send().await.map_err(|source| TuiError::Http {
            url: url.clone(),
            source,
        })?;
        let status = response.status().as_u16();
        let headers = response
            .headers()
            .iter()
            .filter_map(|(name, value)| {
                value
                    .to_str()
                    .ok()
                    .map(|value| (name.as_str().to_string(), value.to_string()))
            })
            .collect();
        let body = response.text().await.map_err(|source| TuiError::Http {
            url: url.clone(),
            source,
        })?;
        let body = if body.trim().is_empty() {
            Value::Null
        } else {
            serde_json::from_str(&body).unwrap_or(Value::String(body))
        };
        Ok(ResponseSpec::json_with_headers(status, body, headers))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_auth_file(name: &str, token: &str) -> std::path::PathBuf {
        let path = std::env::temp_dir().join(format!(
            "va-transport-{name}-{}-{}.json",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|elapsed| elapsed.as_nanos())
                .unwrap_or(0)
        ));
        std::fs::write(&path, format!(r#"{{"port":12358,"token":"{token}"}}"#)).expect("write");
        path
    }

    #[test]
    fn a_rotated_token_is_picked_up_from_the_auth_file() {
        let path = temp_auth_file("rotated", "second");
        let transport = HttpTransport::new(
            ServerEndpoint::new("http://127.0.0.1:12358/va").with_token("first"),
        )
        .refreshing_from(Some(path.clone()));

        assert!(transport.refresh_token());
        assert_eq!(transport.endpoint().token(), Some("second"));

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn an_unchanged_token_is_not_a_reason_to_retry() {
        let path = temp_auth_file("unchanged", "same");
        let transport =
            HttpTransport::new(ServerEndpoint::new("http://127.0.0.1:12358/va").with_token("same"))
                .refreshing_from(Some(path.clone()));

        assert!(!transport.refresh_token());

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn an_explicit_token_is_never_swapped_out() {
        let path = temp_auth_file("explicit", "from-disk");
        // No refreshing_from: the caller chose this token themselves.
        let transport = HttpTransport::new(
            ServerEndpoint::new("http://127.0.0.1:12358/va").with_token("chosen"),
        );

        assert!(!transport.refresh_token());
        assert_eq!(transport.endpoint().token(), Some("chosen"));

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn a_missing_auth_file_is_not_an_error() {
        let transport = HttpTransport::new(
            ServerEndpoint::new("http://127.0.0.1:12358/va").with_token("first"),
        )
        .refreshing_from(Some(std::path::PathBuf::from("/nonexistent/va-auth.json")));

        assert!(!transport.refresh_token());
        assert_eq!(transport.endpoint().token(), Some("first"));
    }
}
