mod chat;
mod previews;
mod profiles;
mod runtime;
mod serve;
mod service;
mod sessions;
mod workspaces;

use serde_json::Value;
use va_client::http::AuthRequirement;
use va_client::Operation;

use crate::args::{usage, Command, Options};
use crate::config::endpoint_for;
use crate::error::CliError;
use crate::transport::HttpTransport;

pub(crate) async fn dispatch(options: &Options, command: Command) -> Result<(), CliError> {
    match command {
        Command::Help => {
            println!("{}", usage());
        }
        Command::Health => service::health(options).await?,
        Command::Info => service::info(options).await?,
        Command::Status => service::status(options).await?,
        Command::Doctor => service::doctor(options).await?,
        Command::Serve(args) => serve::run(options, &args)?,
        Command::Channels => runtime::channels(options).await?,
        Command::Tunnels => runtime::tunnels(options).await?,
        Command::Agents => runtime::agents(options).await?,
        Command::Sessions => sessions::list(options).await?,
        Command::Workspaces => workspaces::list(options).await?,
        Command::Previews => previews::list(options).await?,
        Command::Profiles => profiles::list(options).await?,
        Command::ChatSend(args) => chat::send(options, &args).await?,
        Command::ChatRepl(args) => chat::repl(options, &args).await?,
        Command::ChatSessions => chat::sessions(options)?,
        Command::ChatForget(args) => chat::forget(options, &args)?,
        Command::PairStart(args) => {
            crate::pair::start(options, &args).await?;
        }
        Command::PairStatus { sid, save } => {
            crate::pair::status(options, &sid, save).await?;
        }
        Command::PairWait(args) => {
            crate::pair::wait(options, &args).await?;
        }
        Command::AuthStatus => {
            crate::auth::status(options)?;
        }
        Command::AuthClear => {
            crate::auth::clear(options)?;
        }
        Command::TmuxSessions => runtime::tmux_sessions(options).await?,
        Command::LaunchSessions(args) => sessions::launch_sessions(options, &args).await?,
        Command::LaunchSessionArchive(args) => {
            sessions::launch_session_mutation(options, &args, true).await?;
        }
        Command::LaunchSessionUnarchive(args) => {
            sessions::launch_session_mutation(options, &args, false).await?;
        }
        Command::SettingsReload => runtime::reload_settings(options).await?,
        Command::ChannelSync => runtime::sync_channels(options).await?,
        Command::ChannelStart { kind } => runtime::start_channel(options, &kind).await?,
        Command::ChannelStop { kind } => runtime::stop_channel(options, &kind).await?,
        Command::ChannelRestart { kind } => runtime::restart_channel(options, &kind).await?,
        Command::TunnelKill { provider } => runtime::kill_tunnel(options, &provider).await?,
        Command::AgentKill { route_key } => runtime::kill_agent(options, &route_key).await?,
        Command::SessionCreate(create) => sessions::create(options, &create).await?,
        Command::SessionAttach { session_id } => {
            crate::attach::attach_session(options, &session_id).await?;
        }
        Command::SessionKill { session_id } => sessions::kill(options, &session_id).await?,
        Command::PtyKill { session_id } => runtime::kill_pty(options, &session_id).await?,
        Command::PreviewDelete { slug } => previews::delete(options, &slug).await?,
        Command::WorkspaceAdd { path } => workspaces::add(options, &path).await?,
        Command::WorkspaceRemove { path } => workspaces::remove(options, &path).await?,
        Command::WorkspaceDefault { path } => workspaces::set_default(options, &path).await?,
        Command::WorkspaceCreate { name } => workspaces::create(options, &name).await?,
    }

    Ok(())
}

pub(super) fn transport_for(
    options: &Options,
    auth: AuthRequirement,
) -> Result<HttpTransport, CliError> {
    Ok(HttpTransport::new(endpoint_for(options, auth)?))
}

pub(crate) fn print_json(value: Value) -> Result<(), CliError> {
    println!("{}", serde_json::to_string_pretty(&value)?);
    Ok(())
}

pub(super) async fn run_unit(
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
