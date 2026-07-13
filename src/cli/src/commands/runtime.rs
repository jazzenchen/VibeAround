use va_client::http::AuthRequirement;
use va_client::ops;

use super::{print_json, run_unit, transport_for};
use crate::args::Options;
use crate::error::CliError;

pub(super) async fn channels(options: &Options) -> Result<(), CliError> {
    let transport = transport_for(options, AuthRequirement::BearerToken)?;
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
        println!("{}\t{:?}{}", channel.instance_id, channel.status, reason);
    }
    Ok(())
}

pub(super) async fn tunnels(options: &Options) -> Result<(), CliError> {
    let transport = transport_for(options, AuthRequirement::BearerToken)?;
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
    Ok(())
}

pub(super) async fn agents(options: &Options) -> Result<(), CliError> {
    let transport = transport_for(options, AuthRequirement::BearerToken)?;
    if options.json {
        print_json(transport.execute_json(ops::runtime_agents()).await?)?;
        return Ok(());
    }
    let agents = transport.execute(ops::runtime_agents()).await?;
    println!("default: {}", agents.default_agent);
    for agent in agents.agents {
        println!("{}\t{}\t{}", agent.id, agent.name, agent.description);
    }
    Ok(())
}

pub(super) async fn tmux_sessions(options: &Options) -> Result<(), CliError> {
    let transport = transport_for(options, AuthRequirement::BearerToken)?;
    if options.json {
        print_json(transport.execute_json(ops::tmux_sessions()).await?)?;
        return Ok(());
    }
    let tmux = transport.execute(ops::tmux_sessions()).await?;
    println!("available: {}", tmux.available);
    for session in tmux.sessions {
        println!("{session}");
    }
    Ok(())
}

pub(super) async fn reload_settings(options: &Options) -> Result<(), CliError> {
    run_unit(options, ops::runtime_reload_settings(), "settings reloaded").await
}

pub(super) async fn sync_channels(options: &Options) -> Result<(), CliError> {
    run_unit(options, ops::runtime_sync_channels(), "channels synced").await
}

pub(super) async fn start_channel(options: &Options, kind: &str) -> Result<(), CliError> {
    run_unit(options, ops::runtime_start_channel(kind), "channel started").await
}

pub(super) async fn stop_channel(options: &Options, kind: &str) -> Result<(), CliError> {
    run_unit(options, ops::runtime_stop_channel(kind), "channel stopped").await
}

pub(super) async fn restart_channel(options: &Options, kind: &str) -> Result<(), CliError> {
    run_unit(
        options,
        ops::runtime_restart_channel(kind),
        "channel restarted",
    )
    .await
}

pub(super) async fn kill_tunnel(options: &Options, provider: &str) -> Result<(), CliError> {
    run_unit(options, ops::runtime_kill_tunnel(provider), "tunnel killed").await
}

pub(super) async fn kill_agent(options: &Options, thread_id: &str) -> Result<(), CliError> {
    run_unit(options, ops::runtime_kill_agent(thread_id), "agent killed").await
}

pub(super) async fn kill_pty(options: &Options, session_id: &str) -> Result<(), CliError> {
    run_unit(options, ops::runtime_kill_pty(session_id), "pty killed").await
}
