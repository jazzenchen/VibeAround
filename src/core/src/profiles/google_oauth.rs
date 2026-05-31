use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{anyhow, Context};
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use url::Url;

use crate::config;

const OAUTH_CLIENT_ID_ENV: &str = "VIBEAROUND_GOOGLE_OAUTH_CLIENT_ID";
const OAUTH_CLIENT_SECRET_ENV: &str = "VIBEAROUND_GOOGLE_OAUTH_CLIENT_SECRET";
const AUTH_URL: &str = "https://accounts.google.com/o/oauth2/v2/auth";
const TOKEN_URL: &str = "https://oauth2.googleapis.com/token";
const TOKEN_REFRESH_SKEW_MS: u64 = 60_000;
const LOGIN_TIMEOUT_SECS: u64 = 300;
const CALLBACK_PATH: &str = "/oauth2callback";
const GOOGLE_OAUTH_SCOPES: &[&str] = &[
    "https://www.googleapis.com/auth/cloud-platform",
    "https://www.googleapis.com/auth/userinfo.email",
    "https://www.googleapis.com/auth/userinfo.profile",
];

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct GoogleOAuthCredentials {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client_secret: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub access_token: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub refresh_token: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub token_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expiry_date: Option<u64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GoogleOAuthStatus {
    pub signed_in: bool,
    pub path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct TokenResponse {
    access_token: String,
    #[serde(default)]
    expires_in: Option<u64>,
    #[serde(default)]
    token_type: Option<String>,
    #[serde(default)]
    scope: Option<String>,
    #[serde(default)]
    refresh_token: Option<String>,
}

struct CallbackResult {
    code: String,
}

struct OAuthClientConfig {
    client_id: String,
    client_secret: String,
}

pub fn vibearound_credentials_path() -> PathBuf {
    config::data_dir().join("google-oauth").join("gemini.json")
}

pub fn gemini_cli_credentials_path() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .map(|home| PathBuf::from(home).join(".gemini").join("oauth_creds.json"))
}

pub fn google_application_credentials_path() -> Option<PathBuf> {
    std::env::var("GOOGLE_APPLICATION_CREDENTIALS")
        .ok()
        .map(|path| path.trim().to_string())
        .filter(|path| !path.is_empty())
        .map(PathBuf::from)
}

pub fn status() -> GoogleOAuthStatus {
    let path = vibearound_credentials_path();
    let signed_in = read_credentials(&path)
        .ok()
        .map(|credentials| {
            credentials
                .refresh_token
                .as_deref()
                .map(str::trim)
                .is_some_and(|token| !token.is_empty())
                || valid_access_token(&credentials).is_some()
        })
        .unwrap_or(false);
    let expires_at = read_credentials(&path)
        .ok()
        .and_then(|credentials| credentials.expiry_date);
    GoogleOAuthStatus {
        signed_in,
        path: path.display().to_string(),
        expires_at,
    }
}

pub async fn login_with_browser(client: &reqwest::Client) -> anyhow::Result<GoogleOAuthStatus> {
    let oauth_client = oauth_client_config(None)?;
    let state = uuid::Uuid::new_v4().to_string();
    let listener = TcpListener::bind(("127.0.0.1", 0))
        .await
        .context("failed to bind Google OAuth callback listener")?;
    let port = listener
        .local_addr()
        .context("failed to read Google OAuth callback listener address")?
        .port();
    let redirect_uri = format!("http://127.0.0.1:{port}{CALLBACK_PATH}");
    let auth_url = authorization_url(&oauth_client, &redirect_uri, &state)?;

    open_browser(&auth_url)?;

    let callback = tokio::time::timeout(
        Duration::from_secs(LOGIN_TIMEOUT_SECS),
        wait_for_callback(listener, &state),
    )
    .await
    .map_err(|_| anyhow!("Google OAuth login timed out"))??;

    let response =
        exchange_authorization_code(client, &oauth_client, &callback.code, &redirect_uri).await?;
    let credentials = credentials_from_token_response(response, Some(&oauth_client));
    write_credentials(&vibearound_credentials_path(), &credentials)
        .context("failed to save Google OAuth credentials")?;
    Ok(status())
}

pub async fn login_with_browser_default_client() -> anyhow::Result<GoogleOAuthStatus> {
    let client = reqwest::Client::builder()
        .build()
        .context("failed to create Google OAuth HTTP client")?;
    login_with_browser(&client).await
}

pub async fn vibearound_access_token(client: &reqwest::Client) -> anyhow::Result<String> {
    let path = vibearound_credentials_path();
    access_token_from_path(client, &path)
        .await
        .with_context(|| format!("VibeAround Google OAuth credentials at {}", path.display()))
}

pub async fn access_token_from_path(
    client: &reqwest::Client,
    path: &Path,
) -> anyhow::Result<String> {
    let mut credentials = read_credentials(path)?;
    if let Some(token) = valid_access_token(&credentials) {
        return Ok(token.to_string());
    }

    let refresh_token = credentials
        .refresh_token
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| anyhow!("OAuth credentials do not include a refresh token"))?
        .to_string();

    let refreshed = refresh_access_token(client, &credentials, &refresh_token).await?;
    credentials.access_token = Some(refreshed.access_token.clone());
    credentials.refresh_token = refreshed.refresh_token.or(credentials.refresh_token);
    credentials.token_type = refreshed.token_type.or(credentials.token_type);
    credentials.scope = refreshed.scope.or(credentials.scope);
    credentials.expiry_date = refreshed
        .expires_in
        .map(|seconds| now_ms().saturating_add(seconds.saturating_mul(1000)))
        .or(credentials.expiry_date);
    write_credentials(path, &credentials)
        .with_context(|| format!("failed to update OAuth credentials at {}", path.display()))?;
    Ok(refreshed.access_token)
}

pub fn read_credentials(path: &Path) -> anyhow::Result<GoogleOAuthCredentials> {
    let body = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read OAuth credentials at {}", path.display()))?;
    serde_json::from_str(&body)
        .with_context(|| format!("failed to parse OAuth credentials at {}", path.display()))
}

pub fn write_credentials(path: &Path, credentials: &GoogleOAuthCredentials) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(parent, std::fs::Permissions::from_mode(0o700));
        }
    }
    let body = serde_json::to_string_pretty(credentials)
        .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))?;
    std::fs::write(path, body)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
    }
    Ok(())
}

fn authorization_url(
    oauth_client: &OAuthClientConfig,
    redirect_uri: &str,
    state: &str,
) -> anyhow::Result<String> {
    let mut url = Url::parse(AUTH_URL)?;
    url.query_pairs_mut()
        .append_pair("client_id", &oauth_client.client_id)
        .append_pair("redirect_uri", redirect_uri)
        .append_pair("response_type", "code")
        .append_pair("scope", &GOOGLE_OAUTH_SCOPES.join(" "))
        .append_pair("access_type", "offline")
        .append_pair("prompt", "consent")
        .append_pair("state", state);
    Ok(url.to_string())
}

fn open_browser(url: &str) -> anyhow::Result<()> {
    let status = platform_open_command(url)
        .status()
        .context("failed to start default browser for Google OAuth")?;
    if status.success() {
        Ok(())
    } else {
        Err(anyhow!("default browser command exited with {status}"))
    }
}

fn platform_open_command(url: &str) -> std::process::Command {
    #[cfg(target_os = "macos")]
    {
        let mut command = std::process::Command::new("open");
        command.arg(url);
        command
    }
    #[cfg(target_os = "windows")]
    {
        let mut command = std::process::Command::new("cmd");
        command.args(["/C", "start", "", url]);
        command
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        let mut command = std::process::Command::new("xdg-open");
        command.arg(url);
        command
    }
}

async fn wait_for_callback(
    listener: TcpListener,
    expected_state: &str,
) -> anyhow::Result<CallbackResult> {
    loop {
        let (stream, _) = listener
            .accept()
            .await
            .context("failed to accept Google OAuth callback")?;
        match handle_callback_stream(stream, expected_state).await {
            Ok(Some(result)) => return Ok(result),
            Ok(None) => continue,
            Err(error) => return Err(error),
        }
    }
}

async fn handle_callback_stream(
    mut stream: TcpStream,
    expected_state: &str,
) -> anyhow::Result<Option<CallbackResult>> {
    let mut buffer = vec![0_u8; 8192];
    let read = stream
        .read(&mut buffer)
        .await
        .context("failed to read Google OAuth callback")?;
    let request = String::from_utf8_lossy(&buffer[..read]);
    let Some(target) = request
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
    else {
        send_callback_response(
            &mut stream,
            400,
            "Bad Request",
            "VibeAround could not read the Google OAuth callback.",
        )
        .await?;
        return Err(anyhow!("Google OAuth callback request was malformed"));
    };
    let callback_url = Url::parse(&format!("http://127.0.0.1{target}"))
        .context("failed to parse Google OAuth callback URL")?;
    if callback_url.path() != CALLBACK_PATH {
        send_callback_response(&mut stream, 404, "Not Found", "Not found.").await?;
        return Ok(None);
    }
    if let Some(error) = callback_url
        .query_pairs()
        .find(|(key, _)| key == "error")
        .map(|(_, value)| value.into_owned())
    {
        send_callback_response(
            &mut stream,
            400,
            "OAuth Error",
            "Google sign-in was cancelled or denied.",
        )
        .await?;
        return Err(anyhow!("Google OAuth returned error: {error}"));
    }
    let state = callback_url
        .query_pairs()
        .find(|(key, _)| key == "state")
        .map(|(_, value)| value.into_owned())
        .unwrap_or_default();
    if state != expected_state {
        send_callback_response(
            &mut stream,
            400,
            "OAuth Error",
            "Google sign-in state did not match.",
        )
        .await?;
        return Err(anyhow!("Google OAuth callback state mismatch"));
    }
    let code = callback_url
        .query_pairs()
        .find(|(key, _)| key == "code")
        .map(|(_, value)| value.into_owned())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| anyhow!("Google OAuth callback did not include an authorization code"))?;

    send_callback_response(
        &mut stream,
        200,
        "OK",
        "Google sign-in complete. You can close this window.",
    )
    .await?;
    Ok(Some(CallbackResult { code }))
}

async fn send_callback_response(
    stream: &mut TcpStream,
    status: u16,
    reason: &str,
    message: &str,
) -> std::io::Result<()> {
    let body = format!(
        "<!doctype html><html><head><meta charset=\"utf-8\"><title>VibeAround</title></head><body><p>{message}</p></body></html>"
    );
    let response = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    stream.write_all(response.as_bytes()).await?;
    stream.shutdown().await
}

async fn exchange_authorization_code(
    client: &reqwest::Client,
    oauth_client: &OAuthClientConfig,
    code: &str,
    redirect_uri: &str,
) -> anyhow::Result<TokenResponse> {
    let body = form_body(&[
        ("client_id", oauth_client.client_id.as_str()),
        ("client_secret", oauth_client.client_secret.as_str()),
        ("code", code),
        ("redirect_uri", redirect_uri),
        ("grant_type", "authorization_code"),
    ]);
    let response = client
        .post(TOKEN_URL)
        .header(
            reqwest::header::CONTENT_TYPE,
            "application/x-www-form-urlencoded",
        )
        .body(body)
        .send()
        .await
        .context("failed to exchange Google OAuth authorization code")?;
    parse_token_response(response, "Google OAuth authorization code exchange").await
}

async fn refresh_access_token(
    client: &reqwest::Client,
    credentials: &GoogleOAuthCredentials,
    refresh_token: &str,
) -> anyhow::Result<TokenResponse> {
    let oauth_client = oauth_client_config(Some(credentials))?;
    let body = form_body(&[
        ("client_id", oauth_client.client_id.as_str()),
        ("client_secret", oauth_client.client_secret.as_str()),
        ("refresh_token", refresh_token),
        ("grant_type", "refresh_token"),
    ]);
    let response = client
        .post(TOKEN_URL)
        .header(
            reqwest::header::CONTENT_TYPE,
            "application/x-www-form-urlencoded",
        )
        .body(body)
        .send()
        .await
        .context("failed to refresh Google OAuth token")?;
    parse_token_response(response, "Google OAuth token refresh").await
}

async fn parse_token_response(
    response: reqwest::Response,
    context: &str,
) -> anyhow::Result<TokenResponse> {
    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(anyhow!("{context} failed with {status}: {body}"));
    }
    let bytes = response
        .bytes()
        .await
        .with_context(|| format!("failed to read {context} response"))?;
    serde_json::from_slice::<TokenResponse>(&bytes)
        .with_context(|| format!("{context} returned invalid JSON"))
}

fn credentials_from_token_response(
    response: TokenResponse,
    oauth_client: Option<&OAuthClientConfig>,
) -> GoogleOAuthCredentials {
    GoogleOAuthCredentials {
        client_id: oauth_client.map(|client| client.client_id.clone()),
        client_secret: oauth_client.map(|client| client.client_secret.clone()),
        access_token: Some(response.access_token),
        refresh_token: response.refresh_token,
        token_type: response.token_type,
        scope: response.scope,
        expiry_date: response
            .expires_in
            .map(|seconds| now_ms().saturating_add(seconds.saturating_mul(1000))),
    }
}

fn oauth_client_config(
    credentials: Option<&GoogleOAuthCredentials>,
) -> anyhow::Result<OAuthClientConfig> {
    let client_id = env_or_credentials_value(
        OAUTH_CLIENT_ID_ENV,
        credentials.and_then(|credentials| credentials.client_id.as_deref()),
    );
    let client_secret = env_or_credentials_value(
        OAUTH_CLIENT_SECRET_ENV,
        credentials.and_then(|credentials| credentials.client_secret.as_deref()),
    );

    match (client_id, client_secret) {
        (Some(client_id), Some(client_secret)) => Ok(OAuthClientConfig {
            client_id,
            client_secret,
        }),
        _ => Err(anyhow!(
            "Google OAuth client is not configured; set {OAUTH_CLIENT_ID_ENV} and {OAUTH_CLIENT_SECRET_ENV}"
        )),
    }
}

fn env_or_credentials_value(env_key: &str, value: Option<&str>) -> Option<String> {
    std::env::var(env_key)
        .ok()
        .or_else(|| value.map(ToOwned::to_owned))
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn form_body(params: &[(&str, &str)]) -> String {
    let mut serializer = url::form_urlencoded::Serializer::new(String::new());
    for (key, value) in params {
        serializer.append_pair(key, value);
    }
    serializer.finish()
}

fn valid_access_token(credentials: &GoogleOAuthCredentials) -> Option<&str> {
    let token = credentials
        .access_token
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())?;
    match credentials.expiry_date {
        Some(expiry) if expiry <= now_ms().saturating_add(TOKEN_REFRESH_SKEW_MS) => None,
        _ => Some(token),
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}
