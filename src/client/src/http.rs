use serde_json::Value;

use crate::error::{decode_json, ensure_success, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HttpMethod {
    Get,
    Post,
    Put,
    Delete,
}

impl HttpMethod {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Get => "GET",
            Self::Post => "POST",
            Self::Put => "PUT",
            Self::Delete => "DELETE",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthRequirement {
    None,
    BearerToken,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RequestSpec {
    pub method: HttpMethod,
    pub path: String,
    pub body: Option<Value>,
    pub auth: AuthRequirement,
}

impl RequestSpec {
    pub fn new(method: HttpMethod, path: impl Into<String>, auth: AuthRequirement) -> Self {
        Self {
            method,
            path: path.into(),
            body: None,
            auth,
        }
    }

    pub fn with_body(mut self, body: Value) -> Self {
        self.body = Some(body);
        self
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ResponseSpec {
    pub status: u16,
    pub body: Value,
}

impl ResponseSpec {
    pub fn json(status: u16, body: Value) -> Self {
        Self { status, body }
    }

    pub fn decode<T: serde::de::DeserializeOwned>(self) -> Result<T> {
        decode_json(self)
    }

    pub fn ensure_success(&self) -> Result<()> {
        ensure_success(self.status, &self.body)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn method_as_str_matches_http_tokens() {
        assert_eq!(HttpMethod::Get.as_str(), "GET");
        assert_eq!(HttpMethod::Post.as_str(), "POST");
        assert_eq!(HttpMethod::Put.as_str(), "PUT");
        assert_eq!(HttpMethod::Delete.as_str(), "DELETE");
    }
}
