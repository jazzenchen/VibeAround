use va_client::http::AuthRequirement;
use va_client::ops;

use super::{print_json, transport_for};
use crate::args::Options;
use crate::config::{resolve_endpoint_env, RuntimeEnv};
use crate::error::CliError;
use crate::transport::HttpTransport;

pub(super) async fn health(options: &Options) -> Result<(), CliError> {
    let transport = transport_for(options, AuthRequirement::None)?;
    if options.json {
        print_json(transport.execute_json(ops::service_health()).await?)?;
        return Ok(());
    }
    let health = transport.execute(ops::service_health()).await?;
    println!("{} {} ok={}", health.service, health.version, health.ok);
    Ok(())
}

pub(super) async fn info(options: &Options) -> Result<(), CliError> {
    let transport = transport_for(options, AuthRequirement::BearerToken)?;
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
    Ok(())
}

pub(super) async fn doctor(options: &Options) -> Result<(), CliError> {
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

pub(super) async fn status(options: &Options) -> Result<(), CliError> {
    let transport = transport_for(options, AuthRequirement::BearerToken)?;
    let mut snapshot = va_client::state::RuntimeSnapshot::new();
    if options.json {
        let service = transport.execute_json(ops::service_info()).await?;
        let channels = transport.execute_json(ops::runtime_channels()).await?;
        let tunnels = transport.execute_json(ops::runtime_tunnels()).await?;
        let agent_runtimes = transport.execute_json(ops::runtime_agent_hosts()).await?;
        let workspaces = transport.execute_json(ops::workspaces()).await?;
        let previews = transport.execute_json(ops::previews()).await?;
        print_json(serde_json::json!({
            "service": service,
            "channels": channels,
            "tunnels": tunnels,
            "agent_runtimes": agent_runtimes,
            "workspaces": workspaces,
            "previews": previews
        }))?;
        return Ok(());
    }

    snapshot.apply_service_info(transport.execute(ops::service_info()).await?);
    snapshot.apply_channels(transport.execute(ops::runtime_channels()).await?);
    snapshot.apply_tunnels(transport.execute(ops::runtime_tunnels()).await?);
    snapshot.apply_agent_runtimes(transport.execute(ops::runtime_agent_hosts()).await?);
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
