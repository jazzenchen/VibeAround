mod args;
mod config;
mod error;
mod transport;

use std::env;

use serde_json::Value;
use va_client::auth::PairStatusResponse;
use va_client::http::AuthRequirement;
use va_client::{ops, Operation};

use args::{parse_args, usage, Command, Options};
use config::{endpoint_for, resolve_endpoint_env, RuntimeEnv};
use error::CliError;
use transport::HttpTransport;

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

fn transport_for(options: &Options, auth: AuthRequirement) -> Result<HttpTransport, CliError> {
    Ok(HttpTransport::new(endpoint_for(options, auth)?))
}
