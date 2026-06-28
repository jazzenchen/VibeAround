use crate::{RequestSpec, ResponseSpec, Result};

/// A prepared server operation: request metadata plus a typed response decoder.
///
/// `Operation` intentionally does not know how the request is sent. Desktop,
/// CLI, and TUI hosts can pass `request()` to their own transport, then feed
/// the returned `ResponseSpec` back into `decode()`.
#[derive(Clone)]
pub struct Operation<T> {
    request: RequestSpec,
    decoder: fn(ResponseSpec) -> Result<T>,
}

impl<T> Operation<T> {
    pub fn new(request: RequestSpec, decoder: fn(ResponseSpec) -> Result<T>) -> Self {
        Self { request, decoder }
    }

    pub fn request(&self) -> &RequestSpec {
        &self.request
    }

    pub fn into_request(self) -> RequestSpec {
        self.request
    }

    pub fn decode(&self, response: ResponseSpec) -> Result<T> {
        (self.decoder)(response)
    }
}

#[cfg(test)]
mod tests {
    use serde::Deserialize;
    use serde_json::json;

    use super::*;
    use crate::{AuthRequirement, HttpMethod};

    #[derive(Debug, Deserialize, PartialEq, Eq)]
    struct Pong {
        pong: bool,
    }

    fn decode_pong(response: ResponseSpec) -> Result<Pong> {
        response.decode()
    }

    #[test]
    fn binds_request_to_decoder_without_transport() {
        let op = Operation::new(
            RequestSpec::new(HttpMethod::Get, "/ping", AuthRequirement::None),
            decode_pong,
        );

        assert_eq!(op.request().path, "/ping");
        let pong = op
            .decode(ResponseSpec::json(200, json!({ "pong": true })))
            .expect("decode");
        assert_eq!(pong, Pong { pong: true });
    }
}
