//! Resolving which VibeAround server to talk to, and with which credential.
//!
//! The CLI and the TUI are separate binaries that reach the daemon over the
//! same authenticated HTTP/WS surface, so they answer the same two questions on
//! every start: where is the server, and what token proves we may use it. Both
//! used to answer them with their own copy of this logic.
//!
//! Reading the auth file stays with the callers — this crate owns no I/O — so
//! they load it (when it is there) and pass it in.

use std::path::{Path, PathBuf};

use crate::auth::{auth_file_matches_base_url, AuthFile};
use crate::endpoint::ServerEndpoint;

/// Where the daemon listens unless told otherwise.
pub const DEFAULT_BASE_URL: &str = "http://127.0.0.1:12358/va";

/// A caller-supplied value plus a label for where it came from, so commands can
/// report how they resolved without the resolver knowing about CLI flags.
#[derive(Debug, Clone, Copy)]
pub struct Sourced<'a> {
    pub value: &'a str,
    pub source: &'static str,
}

impl<'a> Sourced<'a> {
    pub fn new(value: &'a str, source: &'static str) -> Self {
        Self { value, source }
    }
}

/// Explicit choices that outrank the auth file.
#[derive(Debug, Default, Clone, Copy)]
pub struct EndpointOverrides<'a> {
    pub base_url: Option<Sourced<'a>>,
    pub token: Option<Sourced<'a>>,
}

#[derive(Debug, Clone)]
pub struct ResolvedEndpoint {
    pub endpoint: ServerEndpoint,
    pub base_url_source: &'static str,
    pub auth_source: &'static str,
    /// Whether the auth file took part in the decision — true even when it was
    /// absent, so callers can name the path they looked at.
    pub auth_file_consulted: bool,
}

#[derive(Debug, thiserror::Error)]
pub enum ResolveError {
    /// The stored token belongs to a specific local server; sending it anywhere
    /// else would hand the daemon's credential to a stranger.
    #[error("refusing to reuse local auth for {base_url}; pass --token explicitly")]
    ForeignBaseUrl { base_url: String },
    #[error("invalid base url: {base_url}")]
    InvalidBaseUrl { base_url: String },
    #[error("auth is required")]
    MissingAuth,
}

/// Path of the auth file, honouring an explicit override, then the data
/// directory, then the home directory.
pub fn auth_file_path(
    explicit: Option<&Path>,
    data_dir: Option<&str>,
    home_dir: Option<&Path>,
) -> PathBuf {
    if let Some(path) = explicit {
        return path.to_path_buf();
    }
    if let Some(dir) = data_dir {
        return PathBuf::from(dir).join("auth.json");
    }
    home_dir
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".vibearound")
        .join("auth.json")
}

/// Resolve the endpoint from explicit overrides plus the auth file, if loaded.
///
/// `require_auth` is the caller's answer to "does this command need a token at
/// all" — commands that do not are never given one they did not ask for.
pub fn resolve_endpoint(
    overrides: EndpointOverrides<'_>,
    auth_file: Option<&AuthFile>,
    require_auth: bool,
) -> Result<ResolvedEndpoint, ResolveError> {
    let explicit_base = overrides.base_url;
    let base_or_default = || match explicit_base {
        Some(base) => (base.value.to_string(), base.source),
        None => (DEFAULT_BASE_URL.to_string(), "default"),
    };

    // An explicit token wins outright, and never needs the auth file.
    if let Some(token) = overrides.token {
        let (base_url, base_url_source) = base_or_default();
        return Ok(ResolvedEndpoint {
            endpoint: ServerEndpoint::new(base_url).with_token(token.value),
            base_url_source,
            auth_source: token.source,
            auth_file_consulted: false,
        });
    }

    // Otherwise the auth file is the only credential, and when no base URL was
    // given it supplies the port too. A command pointed somewhere explicitly
    // that needs no auth never touches it.
    let consult_auth_file = explicit_base.is_none() || require_auth;
    if consult_auth_file {
        if let Some(auth) = auth_file {
            let (base_url, base_url_source) = match explicit_base {
                Some(base) => {
                    let matches = auth_file_matches_base_url(base.value, auth).map_err(|_| {
                        ResolveError::InvalidBaseUrl {
                            base_url: base.value.to_string(),
                        }
                    })?;
                    if !matches {
                        return Err(ResolveError::ForeignBaseUrl {
                            base_url: base.value.to_string(),
                        });
                    }
                    (base.value.to_string(), base.source)
                }
                None => (auth.base_url(), "auth-file"),
            };
            return Ok(ResolvedEndpoint {
                endpoint: ServerEndpoint::new(base_url).with_token(auth.token.clone()),
                base_url_source,
                auth_source: "auth-file",
                auth_file_consulted: true,
            });
        }
    }

    if require_auth {
        return Err(ResolveError::MissingAuth);
    }

    let (base_url, base_url_source) = base_or_default();
    Ok(ResolvedEndpoint {
        endpoint: ServerEndpoint::new(base_url),
        base_url_source,
        auth_source: "none",
        auth_file_consulted: consult_auth_file,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn auth(port: u16, token: &str) -> AuthFile {
        AuthFile {
            port,
            token: token.to_string(),
        }
    }

    #[test]
    fn an_explicit_token_wins_and_skips_the_auth_file() {
        let resolved = resolve_endpoint(
            EndpointOverrides {
                base_url: Some(Sourced::new("http://127.0.0.1:12358/va", "cli")),
                token: Some(Sourced::new("flag-token", "cli-token")),
            },
            Some(&auth(12358, "file-token")),
            true,
        )
        .expect("resolves");

        assert_eq!(resolved.endpoint.token(), Some("flag-token"));
        assert_eq!(resolved.auth_source, "cli-token");
        assert_eq!(resolved.base_url_source, "cli");
        assert!(!resolved.auth_file_consulted);
    }

    #[test]
    fn a_matching_local_base_url_takes_the_token_from_the_auth_file() {
        let resolved = resolve_endpoint(
            EndpointOverrides {
                base_url: Some(Sourced::new("http://127.0.0.1:12358/va", "cli")),
                token: None,
            },
            Some(&auth(12358, "file-token")),
            true,
        )
        .expect("resolves");

        assert_eq!(resolved.endpoint.token(), Some("file-token"));
        assert_eq!(resolved.auth_source, "auth-file");
        assert!(resolved.auth_file_consulted);
    }

    #[test]
    fn a_remote_base_url_never_reuses_the_local_token() {
        let error = resolve_endpoint(
            EndpointOverrides {
                base_url: Some(Sourced::new("https://example.com/va", "cli")),
                token: None,
            },
            Some(&auth(12358, "file-token")),
            true,
        )
        .expect_err("refuses");

        assert!(matches!(error, ResolveError::ForeignBaseUrl { .. }));
    }

    #[test]
    fn another_local_port_never_reuses_the_local_token() {
        let error = resolve_endpoint(
            EndpointOverrides {
                base_url: Some(Sourced::new("http://127.0.0.1:9000/va", "cli")),
                token: None,
            },
            Some(&auth(12358, "file-token")),
            true,
        )
        .expect_err("refuses");

        assert!(matches!(error, ResolveError::ForeignBaseUrl { .. }));
    }

    #[test]
    fn the_auth_file_supplies_the_port_when_no_base_url_was_given() {
        let resolved = resolve_endpoint(
            EndpointOverrides::default(),
            Some(&auth(9999, "file-token")),
            true,
        )
        .expect("resolves");

        assert_eq!(resolved.endpoint.base_url(), "http://127.0.0.1:9999/va");
        assert_eq!(resolved.base_url_source, "auth-file");
        assert_eq!(resolved.endpoint.token(), Some("file-token"));
    }

    #[test]
    fn a_token_without_a_base_url_falls_back_to_the_default_endpoint() {
        let resolved = resolve_endpoint(
            EndpointOverrides {
                base_url: None,
                token: Some(Sourced::new("env-token", "env-token")),
            },
            None,
            true,
        )
        .expect("resolves");

        assert_eq!(resolved.endpoint.base_url(), DEFAULT_BASE_URL);
        assert_eq!(resolved.base_url_source, "default");
        assert_eq!(resolved.endpoint.token(), Some("env-token"));
    }

    #[test]
    fn a_missing_auth_file_fails_only_when_the_command_needs_a_token() {
        let overrides = EndpointOverrides {
            base_url: Some(Sourced::new("http://127.0.0.1:12358/va", "cli")),
            token: None,
        };

        assert!(matches!(
            resolve_endpoint(overrides, None, true).expect_err("needs auth"),
            ResolveError::MissingAuth
        ));

        let resolved = resolve_endpoint(overrides, None, false).expect("resolves");
        assert_eq!(resolved.endpoint.token(), None);
        assert_eq!(resolved.auth_source, "none");
    }

    #[test]
    fn an_unauthenticated_command_with_a_base_url_is_never_handed_the_token() {
        let resolved = resolve_endpoint(
            EndpointOverrides {
                base_url: Some(Sourced::new("http://127.0.0.1:12358/va", "cli")),
                token: None,
            },
            Some(&auth(12358, "file-token")),
            false,
        )
        .expect("resolves");

        assert_eq!(resolved.endpoint.token(), None);
        assert!(!resolved.auth_file_consulted);
    }

    #[test]
    fn auth_file_path_prefers_explicit_then_data_dir_then_home() {
        assert_eq!(
            auth_file_path(Some(Path::new("/tmp/custom.json")), Some("/data"), None),
            PathBuf::from("/tmp/custom.json")
        );
        assert_eq!(
            auth_file_path(None, Some("/data"), Some(Path::new("/home/user"))),
            PathBuf::from("/data/auth.json")
        );
        assert_eq!(
            auth_file_path(None, None, Some(Path::new("/home/user"))),
            PathBuf::from("/home/user/.vibearound/auth.json")
        );
    }
}
