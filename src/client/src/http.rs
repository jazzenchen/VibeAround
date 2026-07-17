use serde_json::Value;

use crate::error::{decode_json, ensure_success, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HttpMethod {
    Get,
    Post,
    Put,
    Patch,
    Delete,
}

impl HttpMethod {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Get => "GET",
            Self::Post => "POST",
            Self::Put => "PUT",
            Self::Patch => "PATCH",
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
    pub headers: Vec<(String, String)>,
}

impl RequestSpec {
    pub fn new(method: HttpMethod, path: impl Into<String>, auth: AuthRequirement) -> Self {
        Self {
            method,
            path: path.into(),
            body: None,
            auth,
            headers: Vec::new(),
        }
    }

    pub fn with_body(mut self, body: Value) -> Self {
        self.body = Some(body);
        self
    }

    pub fn with_header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers.push((name.into(), value.into()));
        self
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ResponseSpec {
    pub status: u16,
    pub body: Value,
    pub headers: Vec<(String, String)>,
}

impl ResponseSpec {
    pub fn json(status: u16, body: Value) -> Self {
        Self {
            status,
            body,
            headers: Vec::new(),
        }
    }

    pub fn json_with_headers(status: u16, body: Value, headers: Vec<(String, String)>) -> Self {
        Self {
            status,
            body,
            headers,
        }
    }

    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(candidate, _)| candidate.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.as_str())
    }

    pub fn decode<T: serde::de::DeserializeOwned>(self) -> Result<T> {
        decode_json(self)
    }

    pub fn ensure_success(&self) -> Result<()> {
        ensure_success(self.status, &self.body)
    }
}

pub fn join_path(base: &str, segment: &str) -> String {
    format!("{}/{}", base.trim_end_matches('/'), path_segment(segment))
}

pub fn path_with_query(base: &str, params: &[(&str, Option<String>)]) -> String {
    let mut query = String::new();
    for (key, value) in params {
        let Some(value) = value else {
            continue;
        };
        if query.is_empty() {
            query.push('?');
        } else {
            query.push('&');
        }
        query.push_str(&path_segment(key));
        query.push('=');
        query.push_str(&path_segment(value));
    }
    format!("{base}{query}")
}

pub fn append_query_param(path: &str, key: &str, value: &str) -> String {
    let separator = if path.contains('?') { '&' } else { '?' };
    format!(
        "{}{}{}={}",
        path,
        separator,
        path_segment(key),
        path_segment(value)
    )
}

pub fn path_segment(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.bytes() {
        if is_unreserved(byte) {
            out.push(byte as char);
        } else {
            out.push('%');
            out.push(hex(byte >> 4));
            out.push(hex(byte & 0x0f));
        }
    }
    out
}

fn is_unreserved(byte: u8) -> bool {
    matches!(
        byte,
        b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~'
    )
}

fn hex(nibble: u8) -> char {
    match nibble {
        0..=9 => (b'0' + nibble) as char,
        10..=15 => (b'A' + (nibble - 10)) as char,
        _ => unreachable!("nibble is masked to four bits"),
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
        assert_eq!(HttpMethod::Patch.as_str(), "PATCH");
        assert_eq!(HttpMethod::Delete.as_str(), "DELETE");
    }

    #[test]
    fn response_headers_are_case_insensitive() {
        let response = ResponseSpec::json_with_headers(
            200,
            Value::Null,
            vec![("etag".into(), "\"abc\"".into())],
        );
        assert_eq!(response.header("ETag"), Some("\"abc\""));
    }

    #[test]
    fn path_segment_percent_encodes_route_keys() {
        assert_eq!(path_segment("telegram:room 1/a"), "telegram%3Aroom%201%2Fa");
    }

    #[test]
    fn path_with_query_omits_empty_params() {
        assert_eq!(
            path_with_query(
                "/api/items",
                &[("workspace_path", Some("/tmp/a b".into())), ("limit", None)]
            ),
            "/api/items?workspace_path=%2Ftmp%2Fa%20b"
        );
    }

    #[test]
    fn append_query_param_preserves_existing_query() {
        assert_eq!(
            append_query_param("/ws/chat?channel=web", "token", "a/b"),
            "/ws/chat?channel=web&token=a%2Fb"
        );
    }
}
