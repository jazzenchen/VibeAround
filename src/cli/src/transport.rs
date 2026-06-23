use serde_json::Value;
use va_client::endpoint::ServerEndpoint;
use va_client::http::{HttpMethod, RequestSpec, ResponseSpec};
use va_client::Operation;

use crate::error::CliError;

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

    pub(crate) async fn execute<T>(&self, operation: Operation<T>) -> Result<T, CliError> {
        let request = operation.request().clone();
        let response = self.send(request).await?;
        Ok(operation.decode(response)?)
    }

    pub(crate) async fn execute_json<T>(&self, operation: Operation<T>) -> Result<Value, CliError> {
        let response = self.send(operation.into_request()).await?;
        response.ensure_success()?;
        Ok(response.body)
    }

    async fn send(&self, request: RequestSpec) -> Result<ResponseSpec, CliError> {
        let method = match request.method {
            HttpMethod::Get => reqwest::Method::GET,
            HttpMethod::Post => reqwest::Method::POST,
            HttpMethod::Put => reqwest::Method::PUT,
            HttpMethod::Delete => reqwest::Method::DELETE,
        };
        let url = self.endpoint.http_url(&request);
        let mut builder = self.client.request(method, &url);
        if let Some(auth) = self.endpoint.authorization_header(&request) {
            builder = builder.header(reqwest::header::AUTHORIZATION, auth);
        }
        if let Some(body) = request.body {
            builder = builder.json(&body);
        }

        let response = builder.send().await.map_err(|source| CliError::Http {
            url: url.clone(),
            source,
        })?;
        let status = response.status().as_u16();
        let body = response.text().await.map_err(|source| CliError::Http {
            url: url.clone(),
            source,
        })?;
        let body = if body.trim().is_empty() {
            Value::Null
        } else {
            serde_json::from_str(&body).unwrap_or(Value::String(body))
        };
        Ok(ResponseSpec::json(status, body))
    }
}
