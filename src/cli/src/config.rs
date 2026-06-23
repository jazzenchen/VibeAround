use std::env;
use std::fs;
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

    let auth_path = auth_file_path_with_env(options, runtime_env);
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

pub(crate) fn auth_file_path(options: &Options) -> PathBuf {
    auth_file_path_with_env(options, &RuntimeEnv::current())
}

pub(crate) fn save_auth_file(
    options: &Options,
    port: u16,
    token: &str,
) -> Result<PathBuf, CliError> {
    save_auth_file_with_env(options, &RuntimeEnv::current(), port, token)
}

pub(crate) fn local_auth_port(base_url: &str) -> Result<u16, CliError> {
    let url = reqwest::Url::parse(base_url)
        .map_err(|_| CliError::Usage(format!("invalid base url: {base_url}")))?;
    let host = url.host_str().unwrap_or_default();
    if !matches!(host, "127.0.0.1" | "localhost" | "::1") {
        return Err(CliError::Usage(
            "saving auth files is only supported for local VibeAround servers".into(),
        ));
    }
    url.port_or_known_default()
        .ok_or_else(|| CliError::Usage(format!("base url has no port: {base_url}")))
}

pub(crate) fn remove_auth_file(options: &Options) -> Result<Option<PathBuf>, CliError> {
    let path = auth_file_path(options);
    match fs::remove_file(&path) {
        Ok(()) => Ok(Some(path)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(source) => Err(CliError::Io {
            action: "removing auth file",
            source,
        }),
    }
}

pub(crate) fn auth_file_path_with_env(options: &Options, runtime_env: &RuntimeEnv) -> PathBuf {
    options
        .auth_file
        .clone()
        .unwrap_or_else(|| default_auth_path_with_env(runtime_env))
}

fn save_auth_file_with_env(
    options: &Options,
    runtime_env: &RuntimeEnv,
    port: u16,
    token: &str,
) -> Result<PathBuf, CliError> {
    let token = token.trim();
    if token.is_empty() {
        return Err(CliError::Usage("auth token cannot be empty".into()));
    }
    let path = auth_file_path_with_env(options, runtime_env);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|source| CliError::Io {
            action: "creating auth file directory",
            source,
        })?;
    }
    let body = serde_json::to_string_pretty(&serde_json::json!({
        "port": port,
        "token": token
    }))?;
    fs::write(&path, body).map_err(|source| CliError::Io {
        action: "writing auth file",
        source,
    })?;
    set_owner_only(&path).map_err(|source| CliError::Io {
        action: "securing auth file",
        source,
    })?;
    Ok(path)
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

fn set_owner_only(path: &std::path::Path) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    }
    #[cfg(not(unix))]
    {
        let _ = path;
    }
    Ok(())
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

    #[test]
    fn local_auth_port_accepts_local_urls_only() {
        assert_eq!(
            local_auth_port("http://127.0.0.1:12358/va").expect("port"),
            12358
        );
        assert_eq!(
            local_auth_port("http://localhost:3000/va").expect("port"),
            3000
        );
        assert!(matches!(
            local_auth_port("https://example.test/va"),
            Err(CliError::Usage(_))
        ));
    }

    #[test]
    fn save_auth_file_writes_server_auth_shape() {
        let path = std::env::temp_dir().join(format!(
            "va-cli-auth-test-{}-{}.json",
            std::process::id(),
            line!()
        ));
        let _ = fs::remove_file(&path);
        let options = Options {
            auth_file: Some(path.clone()),
            ..Default::default()
        };
        let saved = save_auth_file_with_env(&options, &RuntimeEnv::default(), 12358, " secret ")
            .expect("save auth");

        assert_eq!(saved, path);
        let body = fs::read_to_string(&saved).expect("body");
        let auth = va_client::auth::parse_auth_file(&body).expect("auth");
        assert_eq!(auth.port, 12358);
        assert_eq!(auth.token, "secret");

        let _ = fs::remove_file(&saved);
    }
}
