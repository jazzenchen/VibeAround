use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use serde_json::Value;
use va_client::auth::{local_server_port, AuthFile};
use va_client::endpoint::ServerEndpoint;
use va_client::http::AuthRequirement;
use va_client::local_endpoint::{self, EndpointOverrides, ResolveError, Sourced};

use crate::args::Options;
use crate::error::CliError;

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
    let overrides = EndpointOverrides {
        base_url: options
            .base_url
            .as_deref()
            .map(|value| Sourced::new(value, "cli"))
            .or_else(|| {
                runtime_env
                    .base_url
                    .as_deref()
                    .map(|value| Sourced::new(value, "env"))
            }),
        token: options
            .token
            .as_deref()
            .map(|value| Sourced::new(value, "cli-token"))
            .or_else(|| {
                runtime_env
                    .token
                    .as_deref()
                    .map(|value| Sourced::new(value, "env-token"))
            }),
    };

    let auth_path = auth_file_path_with_env(options, runtime_env);
    let auth_file = auth_path
        .exists()
        .then(|| read_auth_file(&auth_path))
        .transpose()?;

    let resolved = local_endpoint::resolve_endpoint(
        overrides,
        auth_file.as_ref(),
        matches!(auth, AuthRequirement::BearerToken),
    )
    .map_err(|error| match error {
        ResolveError::MissingAuth => CliError::MissingAuth(auth_path.display().to_string()),
        other => CliError::Usage(other.to_string()),
    })?;

    Ok(ResolvedEndpoint {
        endpoint: resolved.endpoint,
        base_url_source: resolved.base_url_source,
        auth_source: resolved.auth_source,
        auth_file: resolved.auth_file_consulted.then_some(auth_path),
    })
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
    let Some(port) = local_server_port(base_url)
        .map_err(|_| CliError::Usage(format!("invalid base url: {base_url}")))?
    else {
        return Err(CliError::Usage(
            "saving auth files is only supported for local VibeAround servers".into(),
        ));
    };
    Ok(port)
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
    // The daemon keeps its scoped credentials in this same file, and one of
    // them survives restarts by design. Replace only what pairing owns.
    let mut record = fs::read_to_string(&path)
        .ok()
        .and_then(|body| serde_json::from_str::<serde_json::Map<String, Value>>(&body).ok())
        .unwrap_or_default();
    record.insert("port".to_string(), serde_json::json!(port));
    record.insert("token".to_string(), serde_json::json!(token));
    let body = serde_json::to_string_pretty(&Value::Object(record))?;
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
    local_endpoint::auth_file_path(
        runtime_env.auth_file.as_deref().map(Path::new),
        runtime_env.data_dir.as_deref(),
        runtime_env.home_dir.as_deref(),
    )
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
            base_url: Some("http://127.0.0.1:12358/va".into()),
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
            base_url: Some("http://127.0.0.1:12358/va".into()),
            auth_file: Some(path.clone()),
            ..Default::default()
        };

        let endpoint = resolve_endpoint_env(
            &options,
            AuthRequirement::BearerToken,
            &RuntimeEnv::default(),
        )
        .expect("endpoint");
        assert_eq!(endpoint.endpoint.base_url(), "http://127.0.0.1:12358/va");
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
            base_url: Some("http://127.0.0.1:9000/va".into()),
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
        assert_eq!(
            endpoint.endpoint.base_url(),
            va_client::local_endpoint::DEFAULT_BASE_URL
        );
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
        assert!(matches!(
            local_auth_port("http://localhost:3000/va"),
            Err(CliError::Usage(_))
        ));
        assert!(matches!(
            local_auth_port("http://[::1]:3000/va"),
            Err(CliError::Usage(_))
        ));
        assert!(matches!(
            local_auth_port("https://127.0.0.1:12358/va"),
            Err(CliError::Usage(_))
        ));
        assert!(matches!(
            local_auth_port("https://example.test/va"),
            Err(CliError::Usage(_))
        ));
    }

    #[test]
    fn saving_paired_auth_keeps_the_daemon_scoped_tokens() {
        let path = std::env::temp_dir().join(format!(
            "va-cli-auth-test-{}-{}.json",
            std::process::id(),
            line!()
        ));
        // The daemon's file: one of these credentials is pasted into provider
        // profiles by hand and must survive pairing.
        fs::write(
            &path,
            r#"{"port":12358,"token":"old","mcp_token":"m","bridge_token":"b","agent_token":"a"}"#,
        )
        .expect("seed");
        let options = Options {
            auth_file: Some(path.clone()),
            ..Default::default()
        };

        save_auth_file_with_env(&options, &RuntimeEnv::default(), 12358, "paired").expect("save");

        let record: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&path).expect("body")).expect("json");
        assert_eq!(record["token"], "paired");
        assert_eq!(record["agent_token"], "a");
        assert_eq!(record["mcp_token"], "m");
        assert_eq!(record["bridge_token"], "b");

        let _ = fs::remove_file(&path);
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
