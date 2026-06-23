use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use clap::Parser;
use va_client::endpoint::ServerEndpoint;

use crate::transport::TuiError;

pub(crate) const DEFAULT_BASE_URL: &str = "http://127.0.0.1:12358/va";

#[derive(Debug, Parser)]
#[command(name = "va-tui", version, about = "VibeAround terminal dashboard")]
pub(crate) struct Args {
    #[arg(long)]
    pub(crate) auth_file: Option<PathBuf>,
    #[arg(long)]
    pub(crate) base_url: Option<String>,
    #[arg(long)]
    pub(crate) token: Option<String>,
    #[arg(long)]
    pub(crate) once: bool,
}

#[derive(Debug, Default)]
pub(crate) struct RuntimeEnv {
    base_url: Option<String>,
    token: Option<String>,
    auth_file: Option<String>,
    data_dir: Option<String>,
    home_dir: Option<PathBuf>,
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

pub(crate) fn resolve_endpoint(
    args: &Args,
    runtime_env: &RuntimeEnv,
) -> Result<ServerEndpoint, TuiError> {
    let base_url = args.base_url.as_deref().or(runtime_env.base_url.as_deref());
    let token = args.token.as_deref().or(runtime_env.token.as_deref());
    let auth_path = auth_file_path(args, runtime_env);

    if let Some(base_url) = base_url {
        let endpoint = ServerEndpoint::new(base_url);
        if let Some(token) = token {
            return Ok(endpoint.with_token(token));
        }
        if auth_path.exists() {
            let auth = read_auth_file(&auth_path)?;
            return Ok(endpoint.with_token(auth.token));
        }
        return Err(TuiError::MissingAuth(auth_path.display().to_string()));
    }

    if let Some(token) = token {
        return Ok(ServerEndpoint::new(DEFAULT_BASE_URL).with_token(token));
    }

    if auth_path.exists() {
        let auth = read_auth_file(&auth_path)?;
        return Ok(ServerEndpoint::from_auth_file(&auth));
    }

    Err(TuiError::MissingAuth(auth_path.display().to_string()))
}

fn read_auth_file(path: &Path) -> Result<va_client::auth::AuthFile, TuiError> {
    let body = fs::read_to_string(path).map_err(|source| TuiError::ReadAuth {
        path: path.display().to_string(),
        source,
    })?;
    va_client::auth::parse_auth_file(&body).map_err(TuiError::from)
}

fn auth_file_path(args: &Args, runtime_env: &RuntimeEnv) -> PathBuf {
    args.auth_file
        .clone()
        .unwrap_or_else(|| default_auth_path(runtime_env))
}

fn default_auth_path(runtime_env: &RuntimeEnv) -> PathBuf {
    if let Some(path) = &runtime_env.auth_file {
        return PathBuf::from(path);
    }
    if let Some(path) = &runtime_env.data_dir {
        return PathBuf::from(path).join("auth.json");
    }
    runtime_env
        .home_dir
        .clone()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".vibearound")
        .join("auth.json")
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
    fn resolves_base_url_with_auth_file_token() {
        let path = std::env::temp_dir().join(format!(
            "va-tui-auth-test-{}-{}.json",
            std::process::id(),
            line!()
        ));
        let _ = fs::remove_file(&path);
        fs::write(&path, r#"{ "port": 12358, "token": "secret" }"#).expect("write auth");
        let args = Args {
            auth_file: Some(path.clone()),
            base_url: Some("http://localhost:9000/va".into()),
            token: None,
            once: false,
        };

        let endpoint = resolve_endpoint(&args, &RuntimeEnv::default()).expect("endpoint");

        assert_eq!(endpoint.base_url(), "http://localhost:9000/va");
        assert_eq!(endpoint.token(), Some("secret"));

        let _ = fs::remove_file(&path);
    }

    #[test]
    fn default_auth_path_uses_env_shape() {
        let env = RuntimeEnv {
            data_dir: Some("/tmp/va".into()),
            home_dir: Some(PathBuf::from("/home/test")),
            ..Default::default()
        };

        assert_eq!(default_auth_path(&env), PathBuf::from("/tmp/va/auth.json"));
    }
}
