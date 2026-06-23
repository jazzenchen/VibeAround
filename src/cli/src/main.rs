use std::env;
use std::path::PathBuf;

use serde_json::Value;
use va_client::auth::PairStatusResponse;
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
    #[error("auth is required for this command; pass --token or set VIBEAROUND_TOKEN when using --base-url")]
    MissingToken,
    #[error("failed to read auth file {path}: {source}")]
    ReadAuth {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to reach {url}: {source}\ntry starting the server with `bun server:dev`, or pass --base-url")]
    Http {
        url: String,
        #[source]
        source: reqwest::Error,
    },
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("client error: {0}")]
    Client(#[from] va_client::ClientError),
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Command {
    Help,
    Health,
    Info,
    Status,
    Doctor,
    Channels,
    Tunnels,
    Agents,
    Sessions,
    Workspaces,
    Previews,
    Profiles,
    PairStart,
    PairStatus { sid: String },
    SettingsReload,
    ChannelSync,
    ChannelStart { kind: String },
    ChannelStop { kind: String },
    ChannelRestart { kind: String },
    TunnelKill { provider: String },
    AgentKill { route_key: String },
    SessionKill { session_id: String },
    PtyKill { session_id: String },
    PreviewDelete { slug: String },
    WorkspaceAdd { path: String },
    WorkspaceRemove { path: String },
    WorkspaceDefault { path: String },
    WorkspaceCreate { name: String },
}

#[derive(Debug, Default)]
struct Options {
    command: Option<Command>,
    auth_file: Option<PathBuf>,
    base_url: Option<String>,
    token: Option<String>,
    json: bool,
}

#[derive(Debug, Default)]
struct RuntimeEnv {
    base_url: Option<String>,
    token: Option<String>,
    auth_file: Option<String>,
    data_dir: Option<String>,
    home_dir: Option<PathBuf>,
}

struct ResolvedEndpoint {
    endpoint: ServerEndpoint,
    base_url_source: &'static str,
    auth_source: &'static str,
    auth_file: Option<PathBuf>,
}

impl RuntimeEnv {
    fn current() -> Self {
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

    async fn execute_json<T>(&self, operation: Operation<T>) -> Result<Value, CliError> {
        let response = self.send(operation.into_request()).await?;
        response.ensure_success()?;
        Ok(response.body)
    }

    async fn send(&self, request: RequestSpec) -> Result<ResponseSpec, CliError> {
        let method = match request.method {
            HttpMethod::Get => reqwest::Method::GET,
            HttpMethod::Post => reqwest::Method::POST,
            HttpMethod::Put => reqwest::Method::PUT,
            HttpMethod::Delete => reqwest::Method::DELETE,
        };
        let url = self.endpoint.http_url(&request);
        let mut builder = self.client.request(method, &url);
        if let Some(auth) = self.endpoint.authorization_header(&request) {
            builder = builder.header(reqwest::header::AUTHORIZATION, auth);
        }
        if let Some(body) = request.body {
            builder = builder.json(&body);
        }

        let response = builder.send().await.map_err(|source| CliError::Http {
            url: url.clone(),
            source,
        })?;
        let status = response.status().as_u16();
        let body = response.text().await.map_err(|source| CliError::Http {
            url: url.clone(),
            source,
        })?;
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
    let command = options
        .command
        .clone()
        .ok_or_else(|| CliError::Usage("missing command".into()))?;

    match command {
        Command::Help => {
            println!("{}", usage());
        }
        Command::Health => {
            let transport = transport_for(&options, AuthRequirement::None)?;
            if options.json {
                print_json(transport.execute_json(ops::service_health()).await?)?;
                return Ok(());
            }
            let health = transport.execute(ops::service_health()).await?;
            println!("{} {} ok={}", health.service, health.version, health.ok);
        }
        Command::Info => {
            let transport = transport_for(&options, AuthRequirement::BearerToken)?;
            if options.json {
                print_json(transport.execute_json(ops::service_info()).await?)?;
                return Ok(());
            }
            let info = transport.execute(ops::service_info()).await?;
            println!("{} {}", info.service, info.version);
            println!("mode: {}", info.mode);
            println!("port: {}", info.port);
            println!("data: {}", info.data_dir);
            println!("settings: {}", info.settings_path);
        }
        Command::Status => run_status(&options).await?,
        Command::Doctor => run_doctor(&options).await?,
        Command::Channels => {
            let transport = transport_for(&options, AuthRequirement::BearerToken)?;
            if options.json {
                print_json(transport.execute_json(ops::runtime_channels()).await?)?;
                return Ok(());
            }
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
            if options.json {
                print_json(transport.execute_json(ops::runtime_tunnels()).await?)?;
                return Ok(());
            }
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
            if options.json {
                print_json(transport.execute_json(ops::runtime_agents()).await?)?;
                return Ok(());
            }
            let agents = transport.execute(ops::runtime_agents()).await?;
            println!("default: {}", agents.default_agent);
            for agent in agents.agents {
                println!("{}\t{}\t{}", agent.id, agent.name, agent.description);
            }
        }
        Command::Sessions => {
            let transport = transport_for(&options, AuthRequirement::BearerToken)?;
            if options.json {
                print_json(transport.execute_json(ops::sessions()).await?)?;
                return Ok(());
            }
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
            if options.json {
                print_json(transport.execute_json(ops::workspaces()).await?)?;
                return Ok(());
            }
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
            if options.json {
                print_json(transport.execute_json(ops::previews()).await?)?;
                return Ok(());
            }
            let previews = transport.execute(ops::previews()).await?;
            println!("tunnel: {}", previews.tunnel_url.as_deref().unwrap_or("-"));
            for preview in previews.previews {
                println!("{}\t{:?}\t{}", preview.slug, preview.kind, preview.title);
            }
        }
        Command::Profiles => {
            let transport = transport_for(&options, AuthRequirement::BearerToken)?;
            if options.json {
                print_json(transport.execute_json(ops::model_profiles()).await?)?;
                return Ok(());
            }
            for profile in transport.execute(ops::model_profiles()).await? {
                println!(
                    "{}\t{}\t{}\t{}",
                    profile.id, profile.label, profile.provider, profile.provider_label
                );
            }
        }
        Command::PairStart => {
            let transport = transport_for(&options, AuthRequirement::None)?;
            if options.json {
                print_json(transport.execute_json(ops::pair_start()).await?)?;
                return Ok(());
            }
            let pair = transport.execute(ops::pair_start()).await?;
            println!("code: {}", pair.code);
            println!("sid: {}", pair.sid);
        }
        Command::PairStatus { sid } => {
            let transport = transport_for(&options, AuthRequirement::None)?;
            if options.json {
                print_json(transport.execute_json(ops::pair_status(&sid)).await?)?;
                return Ok(());
            }
            match transport.execute(ops::pair_status(&sid)).await? {
                PairStatusResponse::Pending => println!("pending"),
                PairStatusResponse::Expired => println!("expired"),
                PairStatusResponse::Verified { token } => {
                    println!("verified");
                    println!("token: {token}");
                }
            }
        }
        Command::SettingsReload => {
            run_unit(
                &options,
                ops::runtime_reload_settings(),
                "settings reloaded",
            )
            .await?;
        }
        Command::ChannelSync => {
            run_unit(&options, ops::runtime_sync_channels(), "channels synced").await?;
        }
        Command::ChannelStart { kind } => {
            run_unit(
                &options,
                ops::runtime_start_channel(&kind),
                "channel started",
            )
            .await?;
        }
        Command::ChannelStop { kind } => {
            run_unit(
                &options,
                ops::runtime_stop_channel(&kind),
                "channel stopped",
            )
            .await?;
        }
        Command::ChannelRestart { kind } => {
            run_unit(
                &options,
                ops::runtime_restart_channel(&kind),
                "channel restarted",
            )
            .await?;
        }
        Command::TunnelKill { provider } => {
            run_unit(
                &options,
                ops::runtime_kill_tunnel(&provider),
                "tunnel killed",
            )
            .await?;
        }
        Command::AgentKill { route_key } => {
            run_unit(
                &options,
                ops::runtime_kill_agent(&route_key),
                "agent killed",
            )
            .await?;
        }
        Command::SessionKill { session_id } => {
            run_unit(&options, ops::session_delete(&session_id), "session killed").await?;
        }
        Command::PtyKill { session_id } => {
            run_unit(&options, ops::runtime_kill_pty(&session_id), "pty killed").await?;
        }
        Command::PreviewDelete { slug } => {
            run_unit(&options, ops::preview_delete(&slug), "preview deleted").await?;
        }
        Command::WorkspaceAdd { path } => {
            run_unit(&options, ops::workspace_add(&path)?, "workspace added").await?;
        }
        Command::WorkspaceRemove { path } => {
            run_unit(&options, ops::workspace_remove(&path)?, "workspace removed").await?;
        }
        Command::WorkspaceDefault { path } => {
            let transport = transport_for(&options, AuthRequirement::BearerToken)?;
            if options.json {
                print_json(
                    transport
                        .execute_json(ops::workspace_set_default(&path)?)
                        .await?,
                )?;
                return Ok(());
            }
            let workspaces = transport
                .execute(ops::workspace_set_default(&path)?)
                .await?;
            println!("default: {}", workspaces.default_workspace);
            println!("workspaces: {}", workspaces.workspaces.len());
        }
        Command::WorkspaceCreate { name } => {
            let transport = transport_for(&options, AuthRequirement::BearerToken)?;
            if options.json {
                print_json(
                    transport
                        .execute_json(ops::workspace_create(&name)?)
                        .await?,
                )?;
                return Ok(());
            }
            let response = transport.execute(ops::workspace_create(&name)?).await?;
            println!("created: {}", response.workspace.path);
            println!("default: {}", response.default_workspace);
            println!("workspaces: {}", response.workspaces.len());
        }
    }

    Ok(())
}

async fn run_doctor(options: &Options) -> Result<(), CliError> {
    let runtime_env = RuntimeEnv::current();
    let public_endpoint = resolve_endpoint_env(options, AuthRequirement::None, &runtime_env)?;
    let health_result = HttpTransport::new(public_endpoint.endpoint.clone())
        .execute_json(ops::service_health())
        .await;
    let protected_endpoint =
        resolve_endpoint_env(options, AuthRequirement::BearerToken, &runtime_env);

    if options.json {
        let health = match &health_result {
            Ok(response) => serde_json::json!({
                "ok": true,
                "response": response
            }),
            Err(error) => serde_json::json!({
                "ok": false,
                "error": error.to_string()
            }),
        };
        let auth = match &protected_endpoint {
            Ok(endpoint) => serde_json::json!({
                "ok": true,
                "source": endpoint.auth_source,
                "auth_file": endpoint.auth_file.as_ref().map(|path| path.display().to_string())
            }),
            Err(error) => serde_json::json!({
                "ok": false,
                "error": error.to_string()
            }),
        };
        print_json(serde_json::json!({
            "endpoint": {
                "base_url": public_endpoint.endpoint.base_url(),
                "base_url_source": public_endpoint.base_url_source,
                "auth_source": public_endpoint.auth_source,
                "auth_file": public_endpoint.auth_file.as_ref().map(|path| path.display().to_string())
            },
            "health": health,
            "auth": auth
        }))?;
        return Ok(());
    }

    println!("endpoint: {}", public_endpoint.endpoint.base_url());
    println!("endpoint source: {}", public_endpoint.base_url_source);
    println!("auth source: {}", public_endpoint.auth_source);
    if let Some(path) = &public_endpoint.auth_file {
        println!("auth file: {}", path.display());
    }

    match health_result {
        Ok(response) => println!("health: ok {}", response),
        Err(error) => println!("health: failed ({error})"),
    }

    match protected_endpoint {
        Ok(endpoint) => println!("protected auth: configured ({})", endpoint.auth_source),
        Err(error) => println!("protected auth: not ready ({error})"),
    }

    Ok(())
}

fn print_json(value: Value) -> Result<(), CliError> {
    println!("{}", serde_json::to_string_pretty(&value)?);
    Ok(())
}

async fn run_unit(
    options: &Options,
    operation: Operation<()>,
    message: &'static str,
) -> Result<(), CliError> {
    let transport = transport_for(options, AuthRequirement::BearerToken)?;
    if options.json {
        let response = transport.execute_json(operation).await?;
        print_json(serde_json::json!({
            "ok": true,
            "message": message,
            "response": response
        }))?;
    } else {
        transport.execute(operation).await?;
        println!("{message}");
    }
    Ok(())
}

async fn run_status(options: &Options) -> Result<(), CliError> {
    let transport = transport_for(options, AuthRequirement::BearerToken)?;
    let mut snapshot = va_client::state::RuntimeSnapshot::new();
    if options.json {
        let service = transport.execute_json(ops::service_info()).await?;
        let channels = transport.execute_json(ops::runtime_channels()).await?;
        let tunnels = transport.execute_json(ops::runtime_tunnels()).await?;
        let agent_runtimes = transport.execute_json(ops::runtime_agent_hosts()).await?;
        let sessions = transport.execute_json(ops::sessions()).await?;
        let workspaces = transport.execute_json(ops::workspaces()).await?;
        let previews = transport.execute_json(ops::previews()).await?;
        print_json(serde_json::json!({
            "service": service,
            "channels": channels,
            "tunnels": tunnels,
            "agent_runtimes": agent_runtimes,
            "sessions": sessions,
            "workspaces": workspaces,
            "previews": previews
        }))?;
        return Ok(());
    }

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
    let mut positionals = Vec::new();
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
            "--json" => {
                options.json = true;
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
            value => positionals.push(value.to_string()),
        }
    }
    if !positionals.is_empty() {
        if options.command.is_some() {
            return Err(CliError::Usage(format!(
                "unexpected argument: {}",
                positionals[0]
            )));
        }
        options.command = Some(parse_command(&positionals)?);
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

fn parse_command(args: &[String]) -> Result<Command, CliError> {
    let Some(command) = args.first().map(String::as_str) else {
        return Err(CliError::Usage("missing command".into()));
    };
    let rest = &args[1..];
    match command {
        "help" => no_args(rest, "help").map(|()| Command::Help),
        "health" => no_args(rest, "health").map(|()| Command::Health),
        "info" => no_args(rest, "info").map(|()| Command::Info),
        "status" => no_args(rest, "status").map(|()| Command::Status),
        "doctor" => no_args(rest, "doctor").map(|()| Command::Doctor),
        "channels" => no_args(rest, "channels").map(|()| Command::Channels),
        "tunnels" => no_args(rest, "tunnels").map(|()| Command::Tunnels),
        "agents" => no_args(rest, "agents").map(|()| Command::Agents),
        "sessions" => no_args(rest, "sessions").map(|()| Command::Sessions),
        "workspaces" => no_args(rest, "workspaces").map(|()| Command::Workspaces),
        "previews" => no_args(rest, "previews").map(|()| Command::Previews),
        "profiles" => no_args(rest, "profiles").map(|()| Command::Profiles),
        "pair" => parse_pair_command(rest),
        "settings" => match rest {
            [action] if action == "reload" => Ok(Command::SettingsReload),
            _ => Err(CliError::Usage("usage: va settings reload".to_string())),
        },
        "channel" => parse_channel_command(rest),
        "tunnel" => match rest {
            [action, provider] if action == "kill" => Ok(Command::TunnelKill {
                provider: provider.to_string(),
            }),
            _ => Err(CliError::Usage("usage: va tunnel kill PROVIDER".into())),
        },
        "agent" => match rest {
            [action, route_key] if action == "kill" => Ok(Command::AgentKill {
                route_key: route_key.to_string(),
            }),
            _ => Err(CliError::Usage("usage: va agent kill ROUTE_KEY".into())),
        },
        "session" => match rest {
            [action, session_id] if action == "kill" => Ok(Command::SessionKill {
                session_id: session_id.to_string(),
            }),
            _ => Err(CliError::Usage("usage: va session kill SESSION_ID".into())),
        },
        "pty" => match rest {
            [action, session_id] if action == "kill" => Ok(Command::PtyKill {
                session_id: session_id.to_string(),
            }),
            _ => Err(CliError::Usage("usage: va pty kill SESSION_ID".into())),
        },
        "preview" => match rest {
            [action, slug] if action == "delete" => Ok(Command::PreviewDelete {
                slug: slug.to_string(),
            }),
            _ => Err(CliError::Usage("usage: va preview delete SLUG".into())),
        },
        "workspace" => parse_workspace_command(rest),
        other => Err(CliError::Usage(format!("unknown command: {other}"))),
    }
}

fn parse_pair_command(args: &[String]) -> Result<Command, CliError> {
    match args {
        [action] if action == "start" => Ok(Command::PairStart),
        [action, sid] if action == "status" => Ok(Command::PairStatus {
            sid: sid.to_string(),
        }),
        _ => Err(CliError::Usage(
            "usage: va pair start; va pair status SID".into(),
        )),
    }
}

fn parse_channel_command(args: &[String]) -> Result<Command, CliError> {
    match args {
        [action] if action == "sync" => Ok(Command::ChannelSync),
        [action, kind] if action == "start" => Ok(Command::ChannelStart {
            kind: kind.to_string(),
        }),
        [action, kind] if action == "stop" => Ok(Command::ChannelStop {
            kind: kind.to_string(),
        }),
        [action, kind] if action == "restart" => Ok(Command::ChannelRestart {
            kind: kind.to_string(),
        }),
        _ => Err(CliError::Usage(
            "usage: va channel sync|start|stop|restart [KIND]".into(),
        )),
    }
}

fn parse_workspace_command(args: &[String]) -> Result<Command, CliError> {
    match args {
        [action, path] if action == "add" => Ok(Command::WorkspaceAdd {
            path: path.to_string(),
        }),
        [action, path] if action == "remove" => Ok(Command::WorkspaceRemove {
            path: path.to_string(),
        }),
        [action, path] if action == "default" => Ok(Command::WorkspaceDefault {
            path: path.to_string(),
        }),
        [action, name] if action == "create" => Ok(Command::WorkspaceCreate {
            name: name.to_string(),
        }),
        _ => Err(CliError::Usage(
            "usage: va workspace add|remove|default PATH; va workspace create NAME".into(),
        )),
    }
}

fn no_args(args: &[String], command: &str) -> Result<(), CliError> {
    if args.is_empty() {
        Ok(())
    } else {
        Err(CliError::Usage(format!("usage: va {command}")))
    }
}

fn transport_for(options: &Options, auth: AuthRequirement) -> Result<HttpTransport, CliError> {
    Ok(HttpTransport::new(endpoint_for(options, auth)?))
}

fn endpoint_for(options: &Options, auth: AuthRequirement) -> Result<ServerEndpoint, CliError> {
    Ok(resolve_endpoint_env(options, auth, &RuntimeEnv::current())?.endpoint)
}

fn resolve_endpoint_env(
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

fn usage() -> &'static str {
    "Usage: va [--auth-file PATH] [--base-url URL] [--token TOKEN] [--json] <command>\n\nCommands:\n  help                         Show this help\n  health                       Check public server liveness\n  info                         Show server metadata\n  status                       Show a compact runtime summary\n  doctor                       Diagnose endpoint, auth, and server health\n  pair start                   Start browser/IM pairing\n  pair status SID              Poll a pairing session\n  channels                     List channel plugin runtimes\n  channel sync                 Reconcile channel plugins with settings\n  channel start KIND           Start a stopped channel plugin\n  channel stop KIND            Stop a channel plugin\n  channel restart KIND         Restart a channel plugin\n  tunnels                      List tunnel runtimes\n  tunnel kill PROVIDER         Stop a tunnel runtime\n  agents                       List enabled agents\n  agent kill ROUTE_KEY         Kill an attached agent runtime\n  sessions                     List PTY sessions\n  session kill SESSION_ID      Kill and remove a PTY session\n  pty kill SESSION_ID          Kill a PTY process by session id\n  workspaces                   List registered workspaces\n  workspace add PATH           Register a workspace path\n  workspace remove PATH        Remove a workspace path\n  workspace default PATH       Set the default workspace\n  workspace create NAME        Create a workspace under the default root\n  previews                     List live previews\n  preview delete SLUG          Close a live preview\n  profiles                     List model profiles\n  settings reload              Reload server settings"
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
    fn parses_channel_action_command() {
        let options = parse_args([
            "channel".to_string(),
            "restart".to_string(),
            "feishu".to_string(),
        ])
        .expect("options");

        assert_eq!(
            options.command,
            Some(Command::ChannelRestart {
                kind: "feishu".into()
            })
        );
    }

    #[test]
    fn parses_workspace_action_command() {
        let options = parse_args([
            "workspace".to_string(),
            "add".to_string(),
            "/tmp/project".to_string(),
        ])
        .expect("options");

        assert_eq!(
            options.command,
            Some(Command::WorkspaceAdd {
                path: "/tmp/project".into()
            })
        );
    }

    #[test]
    fn parses_pair_commands() {
        let start = parse_args(["pair".to_string(), "start".to_string()]).expect("start");
        assert_eq!(start.command, Some(Command::PairStart));

        let status = parse_args([
            "pair".to_string(),
            "status".to_string(),
            "sid-1".to_string(),
        ])
        .expect("status");
        assert_eq!(
            status.command,
            Some(Command::PairStatus {
                sid: "sid-1".into()
            })
        );
    }

    #[test]
    fn parses_json_flag() {
        let options = parse_args(["--json".to_string(), "channels".to_string()]).expect("options");
        assert!(options.json);
        assert_eq!(options.command, Some(Command::Channels));
    }

    #[test]
    fn parses_doctor_command() {
        let options = parse_args(["doctor".to_string()]).expect("options");
        assert_eq!(options.command, Some(Command::Doctor));
    }

    #[test]
    fn rejects_unexpected_subcommand_args() {
        let error = parse_args(["status".to_string(), "extra".to_string()]).expect_err("error");
        assert!(matches!(error, CliError::Usage(_)));
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

    #[test]
    fn token_without_base_url_uses_default_endpoint() {
        let options =
            parse_args(["--token=abc".to_string(), "status".to_string()]).expect("options");

        let endpoint = endpoint_for(&options, AuthRequirement::BearerToken).expect("endpoint");
        assert_eq!(endpoint.base_url(), DEFAULT_BASE_URL);
        assert_eq!(endpoint.token(), Some("abc"));
    }

    #[test]
    fn env_base_url_and_token_build_endpoint() {
        let options = parse_args(["status".to_string()]).expect("options");
        let runtime_env = RuntimeEnv {
            base_url: Some("http://localhost:9000/va".into()),
            token: Some("env-token".into()),
            ..Default::default()
        };

        let endpoint = resolve_endpoint_env(&options, AuthRequirement::BearerToken, &runtime_env)
            .expect("url");
        assert_eq!(endpoint.endpoint.base_url(), "http://localhost:9000/va");
        assert_eq!(endpoint.endpoint.token(), Some("env-token"));
        assert_eq!(endpoint.base_url_source, "env");
        assert_eq!(endpoint.auth_source, "env-token");
    }

    #[test]
    fn cli_token_overrides_env_token() {
        let options =
            parse_args(["--token=cli-token".to_string(), "status".to_string()]).expect("options");
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
