use crate::events::WebSocketSpec;
use crate::http::{append_query_param, AuthRequirement, RequestSpec};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerEndpoint {
    base_url: String,
    token: Option<String>,
}

impl ServerEndpoint {
    pub fn new(base_url: impl Into<String>) -> Self {
        Self {
            base_url: normalize_base_url(base_url.into()),
            token: None,
        }
    }

    pub fn with_token(mut self, token: impl Into<String>) -> Self {
        self.token = Some(token.into());
        self
    }

    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    pub fn token(&self) -> Option<&str> {
        self.token.as_deref()
    }

    pub fn http_url(&self, request: &RequestSpec) -> String {
        self.join(&request.path)
    }

    pub fn authorization_header(&self, request: &RequestSpec) -> Option<String> {
        if matches!(request.auth, AuthRequirement::BearerToken) {
            self.token.as_deref().map(crate::auth::bearer_header_value)
        } else {
            None
        }
    }

    pub fn websocket_url(&self, socket: &WebSocketSpec) -> String {
        let path = if matches!(socket.auth, AuthRequirement::BearerToken) {
            if let Some(token) = &self.token {
                append_query_param(&socket.path, "token", token)
            } else {
                socket.path.clone()
            }
        } else {
            socket.path.clone()
        };
        http_to_ws(&self.join(&path))
    }

    fn join(&self, path: &str) -> String {
        format!(
            "{}/{}",
            self.base_url.trim_end_matches('/'),
            path.trim_start_matches('/')
        )
    }
}

fn normalize_base_url(base_url: String) -> String {
    base_url.trim_end_matches('/').to_string()
}

fn http_to_ws(url: &str) -> String {
    if let Some(rest) = url.strip_prefix("https://") {
        format!("wss://{rest}")
    } else if let Some(rest) = url.strip_prefix("http://") {
        format!("ws://{rest}")
    } else {
        url.to_string()
    }
}

#[cfg(test)]
mod tests {
    use crate::auth::AuthFile;
    use crate::events;
    use crate::service;

    use super::*;

    #[test]
    fn public_request_has_no_authorization_header() {
        let endpoint = ServerEndpoint::new("http://localhost:12358/va").with_token("secret");
        assert_eq!(endpoint.authorization_header(&service::health()), None);
    }

    #[test]
    fn websocket_url_uses_token_query() {
        let endpoint = ServerEndpoint::new("https://example.test/va").with_token("tok/en");
        assert_eq!(
            endpoint.websocket_url(&events::chat_ws()),
            "wss://example.test/va/ws/chat?token=tok%2Fen"
        );
    }

    #[test]
    fn websocket_url_appends_token_to_existing_query() {
        let endpoint = ServerEndpoint::new("https://example.test/va").with_token("tok/en");
        assert_eq!(
            endpoint.websocket_url(&events::chat_ws_for_channel("tui")),
            "wss://example.test/va/ws/chat?channel=tui&token=tok%2Fen"
        );
    }
}
