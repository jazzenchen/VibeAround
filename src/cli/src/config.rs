use std::env;
use std::path::PathBuf;

use va_client::endpoint::ServerEndpoint;
use va_client::http::AuthRequirement;

use crate::args::Options;
use crate::error::CliError;

pub(crate) const DEFAULT_BASE_URL: &str = "http://127.0.0.1:12358/va";

#[derive(Debug, Default)]
pub(crate) struct RuntimeEnv {
    base_url: Option<String>,
    token: Option<String>,
    auth_file: Option<String>,
    data_dir: Option<String>,
    home_dir: Option<PathBuf>,
}

pub(crate) struct ResolvedEndpoint {
    pub(crate) endpoint: ServerEndpoint,
    pub(crate) base_url_source: &'static str,
    pub(crate) auth_source: &'static str,
    pub(crate) auth_file: Option<PathBuf>,
}

impl RuntimeEnv {
    pub(crate) fn current() -> Self {
        Self {
            base_url: env_value("VIBEAROUND_BASE_URL"),
            token: env_value("VIBEAROUND_TOKEN").or_else(|| env_value("VIBEAROUND_AUTH_TOKEN")),
            auth_file: env_value("VIBEAROUND_AUTH_FILE"),
            data_dir: env_value("VIBEAROUND_DATA_DIR"),
            home_dir: env::var_os("HOME")
                .or_else(|| env::var_os("USERPROFILE"))
                .map(PathBuf::from),
        }
    }
}

pub(crate) fn endpoint_for(
    options: &Options,
    auth: AuthRequirement,
) -> Result<ServerEndpoint, CliError> {
    Ok(resolve_endpoint_env(options, auth, &RuntimeEnv::current())?.endpoint)
}

pub(crate) fn resolve_endpoint_env(
    options: &Options,
    auth: AuthRequirement,
    runtime_env: &RuntimeEnv,
) -> Result<ResolvedEndpoint, CliError> {
    let base_url = options
        .base_url
        .as_deref()
        .or(runtime_env.base_url.as_deref());
    let base_url_source = if options.base_url.is_some() {
        "cli"
    } else if runtime_env.base_url.is_some() {
        "env"
    } else {
        "default"
    };
    let token = options
        .token
        .as_deref()
        .map(|token| ("cli-token", token))
        .or_else(|| {
            runtime_env
                .token
                .as_deref()
                .map(|token| ("env-token", token))
        });

    if let Some(base_url) = base_url {
        let endpoint = ServerEndpoint::new(base_url);
        if let Some((auth_source, token)) = token {
            return Ok(ResolvedEndpoint {
                endpoint: endpoint.with_token(token),
                base_url_source,
                auth_source,
                auth_file: None,
            });
        }
        if matches!(auth, AuthRequirement::BearerToken) {
            return Err(CliError::MissingToken);
        }
        return Ok(ResolvedEndpoint {
            endpoint,
            base_url_source,
            auth_source: "none",
            auth_file: None,
        });
    }

    if let Some((auth_source, token)) = token {
        return Ok(ResolvedEndpoint {
            endpoint: ServerEndpoint::new(DEFAULT_BASE_URL).with_token(token),
            base_url_source: "default",
            auth_source,
            auth_file: None,
        });
    }

    let auth_path = options
        .auth_file
        .clone()
        .unwrap_or_else(|| default_auth_path_with_env(runtime_env));
    if auth_path.exists() {
        let body = std::fs::read_to_string(&auth_path).map_err(|source| CliError::ReadAuth {
            path: auth_path.display().to_string(),
            source,
        })?;
        let auth = va_client::auth::parse_auth_file(&body)?;
        return Ok(ResolvedEndpoint {
            endpoint: ServerEndpoint::from_auth_file(&auth),
            base_url_source: "auth-file",
            auth_source: "auth-file",
            auth_file: Some(auth_path),
        });
    }

    if matches!(auth, AuthRequirement::None) {
        return Ok(ResolvedEndpoint {
            endpoint: ServerEndpoint::new(DEFAULT_BASE_URL),
            base_url_source: "default",
            auth_source: "none",
            auth_file: Some(auth_path),
        });
    }

    Err(CliError::MissingAuth(auth_path.display().to_string()))
}

fn default_auth_path_with_env(runtime_env: &RuntimeEnv) -> PathBuf {
    if let Some(path) = &runtime_env.auth_file {
        return PathBuf::from(path);
    }
    if let Some(path) = &runtime_env.data_dir {
        return PathBuf::from(path).join("auth.json");
    }
    home_dir_with_env(runtime_env)
        .join(".vibearound")
        .join("auth.json")
}

fn home_dir_with_env(runtime_env: &RuntimeEnv) -> PathBuf {
    runtime_env
        .home_dir
        .clone()
        .unwrap_or_else(|| PathBuf::from("."))
}

fn env_value(key: &str) -> Option<String> {
    env::var(key)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requires_token_for_authenticated_base_url() {
        let options = Options {
            base_url: Some("http://localhost:12358/va".into()),
            ..Default::default()
        };

        let result = resolve_endpoint_env(
            &options,
            AuthRequirement::BearerToken,
            &RuntimeEnv::default(),
        );
        assert!(matches!(result, Err(CliError::MissingToken)));
    }

    #[test]
    fn token_without_base_url_uses_default_endpoint() {
        let options = Options {
            token: Some("abc".into()),
            ..Default::default()
        };

        let endpoint = resolve_endpoint_env(
            &options,
            AuthRequirement::BearerToken,
            &RuntimeEnv::default(),
        )
        .expect("endpoint");
        assert_eq!(endpoint.endpoint.base_url(), DEFAULT_BASE_URL);
        assert_eq!(endpoint.endpoint.token(), Some("abc"));
    }

    #[test]
    fn env_base_url_and_token_build_endpoint() {
        let runtime_env = RuntimeEnv {
            base_url: Some("http://localhost:9000/va".into()),
            token: Some("env-token".into()),
            ..Default::default()
        };

        let endpoint = resolve_endpoint_env(
            &Options::default(),
            AuthRequirement::BearerToken,
            &runtime_env,
        )
        .expect("url");
        assert_eq!(endpoint.endpoint.base_url(), "http://localhost:9000/va");
        assert_eq!(endpoint.endpoint.token(), Some("env-token"));
        assert_eq!(endpoint.base_url_source, "env");
        assert_eq!(endpoint.auth_source, "env-token");
    }

    #[test]
    fn cli_token_overrides_env_token() {
        let options = Options {
            token: Some("cli-token".into()),
            ..Default::default()
        };
        let runtime_env = RuntimeEnv {
            base_url: Some("http://localhost:9000/va".into()),
            token: Some("env-token".into()),
            ..Default::default()
        };

        let endpoint = resolve_endpoint_env(&options, AuthRequirement::BearerToken, &runtime_env)
            .expect("url");
        assert_eq!(endpoint.endpoint.token(), Some("cli-token"));
        assert_eq!(endpoint.auth_source, "cli-token");
    }

    #[test]
    fn default_auth_path_uses_env_without_mutating_process_env() {
        let auth_file_env = RuntimeEnv {
            auth_file: Some("/tmp/va-auth.json".into()),
            home_dir: Some(PathBuf::from("/home/test")),
            ..Default::default()
        };
        assert_eq!(
            default_auth_path_with_env(&auth_file_env),
            PathBuf::from("/tmp/va-auth.json")
        );

        let data_dir_env = RuntimeEnv {
            data_dir: Some("/tmp/va".into()),
            home_dir: Some(PathBuf::from("/home/test")),
            ..Default::default()
        };
        assert_eq!(
            default_auth_path_with_env(&data_dir_env),
            PathBuf::from("/tmp/va/auth.json")
        );
    }
}
