use std::io;

use serde_json::Value;
use va_client::endpoint::ServerEndpoint;
use va_client::http::{HttpMethod, RequestSpec, ResponseSpec};
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
    endpoint: ServerEndpoint,
    client: reqwest::Client,
}

impl HttpTransport {
    pub(crate) fn new(endpoint: ServerEndpoint) -> Self {
        Self {
            endpoint,
            client: reqwest::Client::new(),
        }
    }

    pub(crate) async fn execute<T>(&self, operation: Operation<T>) -> Result<T, TuiError> {
        let request = operation.request().clone();
        let response = self.send(request).await?;
        Ok(operation.decode(response)?)
    }

    async fn send(&self, request: RequestSpec) -> Result<ResponseSpec, TuiError> {
        let method = match request.method {
            HttpMethod::Get => reqwest::Method::GET,
            HttpMethod::Post => reqwest::Method::POST,
            HttpMethod::Put => reqwest::Method::PUT,
            HttpMethod::Patch => reqwest::Method::PATCH,
            HttpMethod::Delete => reqwest::Method::DELETE,
        };
        let url = self.endpoint.http_url(&request);
        let mut builder = self.client.request(method, &url);
        if let Some(auth) = self.endpoint.authorization_header(&request) {
            builder = builder.header(reqwest::header::AUTHORIZATION, auth);
        }
        for (name, value) in &request.headers {
            builder = builder.header(name.as_str(), value.as_str());
        }
        if let Some(body) = request.body {
            builder = builder.json(&body);
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
