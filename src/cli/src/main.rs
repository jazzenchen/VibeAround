use std::env;
use std::path::PathBuf;

use serde_json::Value;
use va_client::endpoint::ServerEndpoint;
use va_client::http::{AuthRequirement, HttpMethod, RequestSpec, ResponseSpec};
use va_client::{ops, Operation};

const DEFAULT_BASE_URL: &str = "http://127.0.0.1:12358/va";

#[derive(Debug, thiserror::Error)]
enum CliError {
    #[error("{0}")]
    Usage(String),
    #[error("auth is required; pass --base-url and --token, or start VibeAround so auth.json exists at {0}")]
    MissingAuth(String),
    #[error("auth is required for this command; pass --token when using --base-url")]
    MissingToken,
    #[error("failed to read auth file {path}: {source}")]
    ReadAuth {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error("HTTP request failed: {0}")]
    Http(#[from] reqwest::Error),
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("client error: {0}")]
    Client(#[from] va_client::ClientError),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Command {
    Help,
    Health,
    Info,
    Status,
    Channels,
    Tunnels,
    Agents,
    Sessions,
    Workspaces,
    Previews,
    Profiles,
}

#[derive(Debug, Default)]
struct Options {
    command: Option<Command>,
    auth_file: Option<PathBuf>,
    base_url: Option<String>,
    token: Option<String>,
}

struct HttpTransport {
    endpoint: ServerEndpoint,
    client: reqwest::Client,
}

impl HttpTransport {
    fn new(endpoint: ServerEndpoint) -> Self {
        Self {
            endpoint,
            client: reqwest::Client::new(),
        }
    }

    async fn execute<T>(&self, operation: Operation<T>) -> Result<T, CliError> {
        let request = operation.request().clone();
        let response = self.send(request).await?;
        Ok(operation.decode(response)?)
    }

    async fn send(&self, request: RequestSpec) -> Result<ResponseSpec, CliError> {
        let method = match request.method {
            HttpMethod::Get => reqwest::Method::GET,
            HttpMethod::Post => reqwest::Method::POST,
            HttpMethod::Put => reqwest::Method::PUT,
            HttpMethod::Delete => reqwest::Method::DELETE,
        };
        let mut builder = self
            .client
            .request(method, self.endpoint.http_url(&request));
        if let Some(auth) = self.endpoint.authorization_header(&request) {
            builder = builder.header(reqwest::header::AUTHORIZATION, auth);
        }
        if let Some(body) = request.body {
            builder = builder.json(&body);
        }

        let response = builder.send().await?;
        let status = response.status().as_u16();
        let body = response.text().await?;
        let body = if body.trim().is_empty() {
            Value::Null
        } else {
            serde_json::from_str(&body).unwrap_or(Value::String(body))
        };
        Ok(ResponseSpec::json(status, body))
    }
}

#[tokio::main]
async fn main() {
    if let Err(error) = run().await {
        eprintln!("va: {error}");
        if matches!(error, CliError::Usage(_)) {
            eprintln!();
            eprintln!("{}", usage());
        }
        std::process::exit(2);
    }
}

async fn run() -> Result<(), CliError> {
    let options = parse_args(env::args().skip(1))?;
    let Some(command) = options.command else {
        return Err(CliError::Usage("missing command".into()));
    };

    match command {
        Command::Help => {
            println!("{}", usage());
        }
        Command::Health => {
            let transport = transport_for(&options, AuthRequirement::None)?;
            let health = transport.execute(ops::service_health()).await?;
            println!("{} {} ok={}", health.service, health.version, health.ok);
        }
        Command::Info => {
            let transport = transport_for(&options, AuthRequirement::BearerToken)?;
            let info = transport.execute(ops::service_info()).await?;
            println!("{} {}", info.service, info.version);
            println!("mode: {}", info.mode);
            println!("port: {}", info.port);
            println!("data: {}", info.data_dir);
            println!("settings: {}", info.settings_path);
        }
        Command::Status => run_status(&options).await?,
        Command::Channels => {
            let transport = transport_for(&options, AuthRequirement::BearerToken)?;
            for channel in transport.execute(ops::runtime_channels()).await? {
                let reason = channel
                    .reason
                    .as_deref()
                    .filter(|value| !value.is_empty())
                    .map(|value| format!(" ({value})"))
                    .unwrap_or_default();
                println!("{}\t{:?}{}", channel.kind, channel.status, reason);
            }
        }
        Command::Tunnels => {
            let transport = transport_for(&options, AuthRequirement::BearerToken)?;
            for tunnel in transport.execute(ops::runtime_tunnels()).await? {
                println!(
                    "{}\t{:?}\t{}",
                    tunnel.provider,
                    tunnel.status,
                    tunnel.url.unwrap_or_else(|| "-".into())
                );
            }
        }
        Command::Agents => {
            let transport = transport_for(&options, AuthRequirement::BearerToken)?;
            let agents = transport.execute(ops::runtime_agents()).await?;
            println!("default: {}", agents.default_agent);
            for agent in agents.agents {
                println!("{}\t{}\t{}", agent.id, agent.name, agent.description);
            }
        }
        Command::Sessions => {
            let transport = transport_for(&options, AuthRequirement::BearerToken)?;
            for session in transport.execute(ops::sessions()).await? {
                println!(
                    "{}\t{:?}\t{}",
                    session.session_id,
                    session.status,
                    session.project_path.unwrap_or_else(|| "-".into())
                );
            }
        }
        Command::Workspaces => {
            let transport = transport_for(&options, AuthRequirement::BearerToken)?;
            let workspaces = transport.execute(ops::workspaces()).await?;
            println!("default: {}", workspaces.default_workspace);
            for workspace in workspaces.workspaces {
                let marker = if workspace.is_default {
                    "default"
                } else if workspace.is_builtin {
                    "builtin"
                } else {
                    ""
                };
                println!("{}\t{}", workspace.path, marker);
            }
        }
        Command::Previews => {
            let transport = transport_for(&options, AuthRequirement::BearerToken)?;
            let previews = transport.execute(ops::previews()).await?;
            println!("tunnel: {}", previews.tunnel_url.as_deref().unwrap_or("-"));
            for preview in previews.previews {
                println!("{}\t{:?}\t{}", preview.slug, preview.kind, preview.title);
            }
        }
        Command::Profiles => {
            let transport = transport_for(&options, AuthRequirement::BearerToken)?;
            for profile in transport.execute(ops::model_profiles()).await? {
                println!(
                    "{}\t{}\t{}\t{}",
                    profile.id, profile.label, profile.provider, profile.provider_label
                );
            }
        }
    }

    Ok(())
}

async fn run_status(options: &Options) -> Result<(), CliError> {
    let transport = transport_for(options, AuthRequirement::BearerToken)?;
    let mut snapshot = va_client::state::RuntimeSnapshot::new();
    snapshot.apply_service_info(transport.execute(ops::service_info()).await?);
    snapshot.apply_channels(transport.execute(ops::runtime_channels()).await?);
    snapshot.apply_tunnels(transport.execute(ops::runtime_tunnels()).await?);
    snapshot.apply_agent_runtimes(transport.execute(ops::runtime_agent_hosts()).await?);
    snapshot.apply_sessions(transport.execute(ops::sessions()).await?);
    snapshot.apply_workspaces(transport.execute(ops::workspaces()).await?);
    snapshot.apply_previews(transport.execute(ops::previews()).await?);

    let service = snapshot.service.as_ref().expect("service applied");
    println!(
        "{} {} on :{}",
        service.service, service.version, service.port
    );
    println!(
        "channels: {} running, {} failed, {} total",
        snapshot.running_channels(),
        snapshot.failed_channels(),
        snapshot.channels.len()
    );
    println!(
        "agents: {} active, {} busy",
        snapshot.active_agents(),
        snapshot.busy_agents()
    );
    println!("sessions: {}", snapshot.sessions.len());
    println!("tunnels: {}", snapshot.tunnels.len());
    println!(
        "workspaces: {}",
        snapshot
            .workspaces
            .as_ref()
            .map(|workspaces| workspaces.workspaces.len())
            .unwrap_or(0)
    );
    println!(
        "previews: {}",
        snapshot
            .previews
            .as_ref()
            .map(|previews| previews.previews.len())
            .unwrap_or(0)
    );
    Ok(())
}

fn parse_args<I>(args: I) -> Result<Options, CliError>
where
    I: IntoIterator<Item = String>,
{
    let mut options = Options::default();
    let mut args = args.into_iter().peekable();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--help" | "-h" => options.command = Some(Command::Help),
            "--auth-file" => {
                options.auth_file = Some(PathBuf::from(next_value(&mut args, "--auth-file")?));
            }
            "--base-url" => {
                options.base_url = Some(next_value(&mut args, "--base-url")?);
            }
            "--token" => {
                options.token = Some(next_value(&mut args, "--token")?);
            }
            value if value.starts_with("--auth-file=") => {
                options.auth_file = Some(PathBuf::from(value.trim_start_matches("--auth-file=")));
            }
            value if value.starts_with("--base-url=") => {
                options.base_url = Some(value.trim_start_matches("--base-url=").to_string());
            }
            value if value.starts_with("--token=") => {
                options.token = Some(value.trim_start_matches("--token=").to_string());
            }
            value if value.starts_with('-') => {
                return Err(CliError::Usage(format!("unknown option: {value}")));
            }
            value => {
                if options.command.is_some() {
                    return Err(CliError::Usage(format!("unexpected argument: {value}")));
                }
                options.command = Some(parse_command(value)?);
            }
        }
    }
    Ok(options)
}

fn next_value<I>(args: &mut std::iter::Peekable<I>, flag: &str) -> Result<String, CliError>
where
    I: Iterator<Item = String>,
{
    args.next()
        .filter(|value| !value.is_empty())
        .ok_or_else(|| CliError::Usage(format!("missing value for {flag}")))
}

fn parse_command(value: &str) -> Result<Command, CliError> {
    match value {
        "help" => Ok(Command::Help),
        "health" => Ok(Command::Health),
        "info" => Ok(Command::Info),
        "status" => Ok(Command::Status),
        "channels" => Ok(Command::Channels),
        "tunnels" => Ok(Command::Tunnels),
        "agents" => Ok(Command::Agents),
        "sessions" => Ok(Command::Sessions),
        "workspaces" => Ok(Command::Workspaces),
        "previews" => Ok(Command::Previews),
        "profiles" => Ok(Command::Profiles),
        other => Err(CliError::Usage(format!("unknown command: {other}"))),
    }
}

fn transport_for(options: &Options, auth: AuthRequirement) -> Result<HttpTransport, CliError> {
    Ok(HttpTransport::new(endpoint_for(options, auth)?))
}

fn endpoint_for(options: &Options, auth: AuthRequirement) -> Result<ServerEndpoint, CliError> {
    if let Some(base_url) = &options.base_url {
        let endpoint = ServerEndpoint::new(base_url);
        if let Some(token) = &options.token {
            return Ok(endpoint.with_token(token.as_str()));
        }
        if matches!(auth, AuthRequirement::BearerToken) {
            return Err(CliError::MissingToken);
        }
        return Ok(endpoint);
    }

    let auth_path = options.auth_file.clone().unwrap_or_else(default_auth_path);
    if auth_path.exists() {
        let body = std::fs::read_to_string(&auth_path).map_err(|source| CliError::ReadAuth {
            path: auth_path.display().to_string(),
            source,
        })?;
        let auth = va_client::auth::parse_auth_file(&body)?;
        return Ok(ServerEndpoint::from_auth_file(&auth));
    }

    if matches!(auth, AuthRequirement::None) {
        return Ok(ServerEndpoint::new(DEFAULT_BASE_URL));
    }

    Err(CliError::MissingAuth(auth_path.display().to_string()))
}

fn default_auth_path() -> PathBuf {
    if let Ok(path) = env::var("VIBEAROUND_AUTH_FILE") {
        let path = path.trim();
        if !path.is_empty() {
            return PathBuf::from(path);
        }
    }
    if let Ok(path) = env::var("VIBEAROUND_DATA_DIR") {
        let path = path.trim();
        if !path.is_empty() {
            return PathBuf::from(path).join("auth.json");
        }
    }
    home_dir().join(".vibearound").join("auth.json")
}

fn home_dir() -> PathBuf {
    env::var_os("HOME")
        .or_else(|| env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
}

fn usage() -> &'static str {
    "Usage: va [--auth-file PATH] [--base-url URL] [--token TOKEN] <command>\n\nCommands:\n  help        Show this help\n  health      Check public server liveness\n  info        Show server metadata\n  status      Show a compact runtime summary\n  channels    List channel plugin runtimes\n  tunnels     List tunnel runtimes\n  agents      List enabled agents\n  sessions    List PTY sessions\n  workspaces  List registered workspaces\n  previews    List live previews\n  profiles    List model profiles"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_command_and_global_options() {
        let options = parse_args([
            "--base-url".to_string(),
            "http://localhost:12358/va".to_string(),
            "--token=abc".to_string(),
            "status".to_string(),
        ])
        .expect("options");

        assert_eq!(options.command, Some(Command::Status));
        assert_eq!(
            options.base_url.as_deref(),
            Some("http://localhost:12358/va")
        );
        assert_eq!(options.token.as_deref(), Some("abc"));
    }

    #[test]
    fn rejects_unknown_command() {
        let error = parse_args(["bogus".to_string()]).expect_err("error");
        assert!(matches!(error, CliError::Usage(_)));
    }

    #[test]
    fn parses_help_as_command() {
        let options = parse_args(["--help".to_string()]).expect("options");
        assert_eq!(options.command, Some(Command::Help));
    }

    #[test]
    fn requires_token_for_authenticated_base_url() {
        let options = parse_args([
            "--base-url=http://localhost:12358/va".to_string(),
            "status".to_string(),
        ])
        .expect("options");

        let result = endpoint_for(&options, AuthRequirement::BearerToken);
        assert!(matches!(result, Err(CliError::MissingToken)));
    }
}
