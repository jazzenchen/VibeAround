use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use va_client::auth::{auth_file_matches_base_url, loopback_port, AuthFile};
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

    let auth_path = auth_file_path_with_env(options, runtime_env);

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
            if auth_path.exists() {
                let auth_file = read_auth_file(&auth_path)?;
                require_matching_local_auth(base_url, &auth_file)?;
                return Ok(ResolvedEndpoint {
                    endpoint: endpoint.with_token(auth_file.token),
                    base_url_source,
                    auth_source: "auth-file",
                    auth_file: Some(auth_path),
                });
            }
            return Err(CliError::MissingAuth(auth_path.display().to_string()));
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

    if auth_path.exists() {
        let auth = read_auth_file(&auth_path)?;
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

pub(crate) fn chat_sessions_path(options: &Options) -> PathBuf {
    let auth_path = auth_file_path(options);
    auth_path
        .parent()
        .unwrap_or_else(|| std::path::Path::new("."))
        .join("chat-sessions.json")
}

pub(crate) fn save_auth_file(
    options: &Options,
    port: u16,
    token: &str,
) -> Result<PathBuf, CliError> {
    save_auth_file_with_env(options, &RuntimeEnv::current(), port, token)
}

pub(crate) fn local_auth_port(base_url: &str) -> Result<u16, CliError> {
    let Some(port) = loopback_port(base_url)
        .map_err(|_| CliError::Usage(format!("invalid base url: {base_url}")))?
    else {
        return Err(CliError::Usage(
            "saving auth files is only supported for local VibeAround servers".into(),
        ));
    };
    Ok(port)
}

fn require_matching_local_auth(base_url: &str, auth_file: &AuthFile) -> Result<(), CliError> {
    let matches = auth_file_matches_base_url(base_url, auth_file)
        .map_err(|_| CliError::Usage(format!("invalid base url: {base_url}")))?;
    if matches {
        return Ok(());
    }
    Err(CliError::Usage(format!(
        "refusing to reuse local auth for {base_url}; pass --token explicitly"
    )))
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

pub(crate) fn set_owner_only(path: &std::path::Path) -> std::io::Result<()> {
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

fn read_auth_file(path: &Path) -> Result<AuthFile, CliError> {
    let body = fs::read_to_string(path).map_err(|source| CliError::ReadAuth {
        path: path.display().to_string(),
        source,
    })?;
    va_client::auth::parse_auth_file(&body).map_err(CliError::from)
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
        assert!(matches!(result, Err(CliError::MissingAuth(_))));
    }

    #[test]
    fn matching_local_base_url_reads_token_from_auth_file_when_token_missing() {
        let path = std::env::temp_dir().join(format!(
            "va-cli-auth-base-url-test-{}-{}.json",
            std::process::id(),
            line!()
        ));
        let _ = fs::remove_file(&path);
        fs::write(&path, r#"{ "port": 12358, "token": "local-secret" }"#).expect("write auth");
        let options = Options {
            base_url: Some("http://localhost:12358/va".into()),
            auth_file: Some(path.clone()),
            ..Default::default()
        };

        let endpoint = resolve_endpoint_env(
            &options,
            AuthRequirement::BearerToken,
            &RuntimeEnv::default(),
        )
        .expect("endpoint");
        assert_eq!(endpoint.endpoint.base_url(), "http://localhost:12358/va");
        assert_eq!(endpoint.endpoint.token(), Some("local-secret"));
        assert_eq!(endpoint.base_url_source, "cli");
        assert_eq!(endpoint.auth_source, "auth-file");
        assert_eq!(endpoint.auth_file.as_deref(), Some(path.as_path()));

        let _ = fs::remove_file(&path);
    }

    #[test]
    fn remote_base_url_cannot_reuse_local_auth_file() {
        let path = std::env::temp_dir().join(format!(
            "va-cli-auth-remote-test-{}-{}.json",
            std::process::id(),
            line!()
        ));
        let _ = fs::remove_file(&path);
        fs::write(&path, r#"{ "port": 12358, "token": "local-secret" }"#).expect("write auth");
        let options = Options {
            base_url: Some("https://example.test/va".into()),
            auth_file: Some(path.clone()),
            ..Default::default()
        };

        let result = resolve_endpoint_env(
            &options,
            AuthRequirement::BearerToken,
            &RuntimeEnv::default(),
        );
        assert!(matches!(result, Err(CliError::Usage(_))));

        let _ = fs::remove_file(&path);
    }

    #[test]
    fn different_local_port_cannot_reuse_auth_file() {
        let path = std::env::temp_dir().join(format!(
            "va-cli-auth-port-test-{}-{}.json",
            std::process::id(),
            line!()
        ));
        let _ = fs::remove_file(&path);
        fs::write(&path, r#"{ "port": 12358, "token": "local-secret" }"#).expect("write auth");
        let options = Options {
            base_url: Some("http://localhost:9000/va".into()),
            auth_file: Some(path.clone()),
            ..Default::default()
        };

        let result = resolve_endpoint_env(
            &options,
            AuthRequirement::BearerToken,
            &RuntimeEnv::default(),
        );
        assert!(matches!(result, Err(CliError::Usage(_))));

        let _ = fs::remove_file(&path);
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
