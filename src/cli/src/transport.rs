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

pub(crate) fn redact_token_query(url: &str) -> String {
    let Ok(mut parsed) = reqwest::Url::parse(url) else {
        return url.to_string();
    };
    let pairs = parsed
        .query_pairs()
        .map(|(key, value)| {
            let value = if key == "token" {
                "redacted".to_string()
            } else {
                value.into_owned()
            };
            (key.into_owned(), value)
        })
        .collect::<Vec<_>>();
    if !pairs.iter().any(|(key, _)| key == "token") {
        return url.to_string();
    }
    parsed.query_pairs_mut().clear().extend_pairs(
        pairs
            .iter()
            .map(|(key, value)| (key.as_str(), value.as_str())),
    );
    parsed.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redact_token_query_hides_websocket_token() {
        assert_eq!(
            redact_token_query("ws://127.0.0.1:12358/va/ws/chat?token=secret&mode=chat"),
            "ws://127.0.0.1:12358/va/ws/chat?token=redacted&mode=chat"
        );
    }

    #[test]
    fn redact_token_query_leaves_plain_urls_unchanged() {
        assert_eq!(
            redact_token_query("ws://127.0.0.1:12358/va/ws/chat"),
            "ws://127.0.0.1:12358/va/ws/chat"
        );
    }
}
