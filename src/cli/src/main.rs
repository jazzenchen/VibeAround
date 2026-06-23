mod args;
mod attach;
mod config;
mod error;
mod transport;

use std::{
    env,
    path::PathBuf,
    time::{Duration, Instant},
};

use serde_json::Value;
use va_client::auth::{PairStartResponse, PairStatusResponse};
use va_client::http::AuthRequirement;
use va_client::sessions::{CreateSessionBody, LaunchSessionInfo, PtyTool};
use va_client::{ops, Operation};

use args::{
    parse_args, usage, Command, LaunchSessionMutationArgs, LaunchSessionsArgs, Options,
    PairStartArgs, PairWaitArgs, SessionCreateArgs,
};
use config::{
    auth_file_path, endpoint_for, local_auth_port, remove_auth_file, resolve_endpoint_env,
    save_auth_file, RuntimeEnv,
};
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
        Command::PairStart(args) => {
            run_pair_start(&options, &args).await?;
        }
        Command::PairStatus { sid, save } => {
            run_pair_status(&options, &sid, save).await?;
        }
        Command::PairWait(args) => {
            run_pair_wait(&options, &args).await?;
        }
        Command::AuthStatus => {
            run_auth_status(&options)?;
        }
        Command::AuthClear => {
            run_auth_clear(&options)?;
        }
        Command::TmuxSessions => {
            let transport = transport_for(&options, AuthRequirement::BearerToken)?;
            if options.json {
                print_json(transport.execute_json(ops::tmux_sessions()).await?)?;
                return Ok(());
            }
            let tmux = transport.execute(ops::tmux_sessions()).await?;
            println!("available: {}", tmux.available);
            for session in tmux.sessions {
                println!("{session}");
            }
        }
        Command::LaunchSessions(args) => {
            run_launch_sessions(&options, &args).await?;
        }
        Command::LaunchSessionArchive(args) => {
            run_launch_session_mutation(&options, &args, true).await?;
        }
        Command::LaunchSessionUnarchive(args) => {
            run_launch_session_mutation(&options, &args, false).await?;
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
        Command::SessionCreate(create) => {
            run_session_create(&options, &create).await?;
        }
        Command::SessionAttach { session_id } => {
            attach::attach_session(&options, &session_id).await?;
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

async fn run_pair_start(options: &Options, args: &PairStartArgs) -> Result<(), CliError> {
    let transport = transport_for(options, AuthRequirement::None)?;
    let pair = transport.execute(ops::pair_start()).await?;
    if !args.wait {
        if options.json {
            print_json(pair_start_json(&pair))?;
            return Ok(());
        }
        println!("code: {}", pair.code);
        println!("sid: {}", pair.sid);
        return Ok(());
    }

    if !options.json {
        println!("code: {}", pair.code);
        println!("sid: {}", pair.sid);
        println!("waiting: {}s", args.timeout_secs);
    }

    let verified = wait_for_pair_verification(
        options,
        &pair.sid,
        args.save,
        args.timeout_secs,
        args.interval_ms,
    )
    .await?;
    if options.json {
        let mut value = pair_verified_json(&verified);
        value["code"] = serde_json::json!(pair.code);
        value["sid"] = serde_json::json!(pair.sid);
        print_json(value)?;
        return Ok(());
    }
    print_pair_verified(&verified);
    Ok(())
}

async fn run_pair_status(options: &Options, sid: &str, save: bool) -> Result<(), CliError> {
    let transport = transport_for(options, AuthRequirement::None)?;
    let status = transport.execute(ops::pair_status(sid)).await?;
    let saved_path = if save {
        match &status {
            PairStatusResponse::Verified { token } => {
                Some(save_verified_pair_token(options, token)?)
            }
            PairStatusResponse::Pending | PairStatusResponse::Expired => None,
        }
    } else {
        None
    };

    if options.json {
        print_json(pair_status_json(&status, saved_path.as_ref()))?;
        return Ok(());
    }

    match status {
        PairStatusResponse::Pending => println!("pending"),
        PairStatusResponse::Expired => println!("expired"),
        PairStatusResponse::Verified { token } => {
            println!("verified");
            if let Some(path) = saved_path {
                println!("saved: {}", path.display());
            } else {
                println!("token: {token}");
            }
        }
    }
    Ok(())
}

async fn run_pair_wait(options: &Options, args: &PairWaitArgs) -> Result<(), CliError> {
    if !options.json {
        println!("waiting: {}s", args.timeout_secs);
    }
    let verified = wait_for_pair_verification(
        options,
        &args.sid,
        args.save,
        args.timeout_secs,
        args.interval_ms,
    )
    .await?;
    if options.json {
        print_json(pair_verified_json(&verified))?;
        return Ok(());
    }
    print_pair_verified(&verified);
    Ok(())
}

struct VerifiedPair {
    token: String,
    saved_path: Option<PathBuf>,
}

async fn wait_for_pair_verification(
    options: &Options,
    sid: &str,
    save: bool,
    timeout_secs: u64,
    interval_ms: u64,
) -> Result<VerifiedPair, CliError> {
    let transport = transport_for(options, AuthRequirement::None)?;
    let deadline = Instant::now() + Duration::from_secs(timeout_secs);
    loop {
        match transport.execute(ops::pair_status(sid)).await? {
            PairStatusResponse::Pending => {
                let now = Instant::now();
                if now >= deadline {
                    return Err(CliError::PairTimeout(timeout_secs));
                }
                let remaining = deadline.saturating_duration_since(now);
                tokio::time::sleep(Duration::from_millis(interval_ms).min(remaining)).await;
            }
            PairStatusResponse::Expired => return Err(CliError::PairExpired),
            PairStatusResponse::Verified { token } => {
                let saved_path = if save {
                    Some(save_verified_pair_token(options, &token)?)
                } else {
                    None
                };
                return Ok(VerifiedPair { token, saved_path });
            }
        }
    }
}

fn pair_start_json(pair: &PairStartResponse) -> Value {
    serde_json::json!({
        "code": pair.code,
        "sid": pair.sid
    })
}

fn pair_verified_json(verified: &VerifiedPair) -> Value {
    let mut value = serde_json::json!({
        "status": "verified",
        "token": verified.token
    });
    if let Some(path) = &verified.saved_path {
        value["saved_auth_file"] = serde_json::json!(path.display().to_string());
    }
    value
}

fn print_pair_verified(verified: &VerifiedPair) {
    println!("verified");
    if let Some(path) = &verified.saved_path {
        println!("saved: {}", path.display());
    } else {
        println!("token: {}", verified.token);
    }
}

fn save_verified_pair_token(
    options: &Options,
    token: &str,
) -> Result<std::path::PathBuf, CliError> {
    let public_endpoint =
        resolve_endpoint_env(options, AuthRequirement::None, &RuntimeEnv::current())?;
    let port = local_auth_port(public_endpoint.endpoint.base_url())?;
    save_auth_file(options, port, token)
}

fn pair_status_json(status: &PairStatusResponse, saved_path: Option<&std::path::PathBuf>) -> Value {
    let mut value = match status {
        PairStatusResponse::Pending => serde_json::json!({ "status": "pending" }),
        PairStatusResponse::Expired => serde_json::json!({ "status": "expired" }),
        PairStatusResponse::Verified { token } => {
            serde_json::json!({ "status": "verified", "token": token })
        }
    };
    if let Some(path) = saved_path {
        value["saved_auth_file"] = serde_json::json!(path.display().to_string());
    }
    value
}

fn run_auth_status(options: &Options) -> Result<(), CliError> {
    let runtime_env = RuntimeEnv::current();
    let path = auth_file_path(options);
    let resolved = resolve_endpoint_env(options, AuthRequirement::BearerToken, &runtime_env);

    if options.json {
        match resolved {
            Ok(endpoint) => {
                print_json(serde_json::json!({
                    "configured": true,
                    "endpoint": endpoint.endpoint.base_url(),
                    "base_url_source": endpoint.base_url_source,
                    "auth_source": endpoint.auth_source,
                    "auth_file": endpoint.auth_file.as_ref().map(|path| path.display().to_string()),
                    "resolved_auth_file": path.display().to_string()
                }))?;
            }
            Err(CliError::MissingAuth(_)) | Err(CliError::MissingToken) => {
                print_json(serde_json::json!({
                    "configured": false,
                    "auth_file": path.display().to_string()
                }))?;
            }
            Err(error) => return Err(error),
        }
        return Ok(());
    }

    println!("auth file: {}", path.display());
    match resolved {
        Ok(endpoint) => {
            println!("configured: yes");
            println!("endpoint: {}", endpoint.endpoint.base_url());
            println!("base url source: {}", endpoint.base_url_source);
            println!("auth source: {}", endpoint.auth_source);
        }
        Err(CliError::MissingAuth(_)) | Err(CliError::MissingToken) => {
            println!("configured: no");
        }
        Err(error) => return Err(error),
    }
    Ok(())
}

fn run_auth_clear(options: &Options) -> Result<(), CliError> {
    let path = auth_file_path(options);
    let removed = remove_auth_file(options)?;
    if options.json {
        print_json(serde_json::json!({
            "removed": removed.is_some(),
            "auth_file": path.display().to_string()
        }))?;
        return Ok(());
    }
    match removed {
        Some(path) => println!("removed: {}", path.display()),
        None => println!("not found: {}", path.display()),
    }
    Ok(())
}

fn print_json(value: Value) -> Result<(), CliError> {
    println!("{}", serde_json::to_string_pretty(&value)?);
    Ok(())
}

async fn run_launch_sessions(options: &Options, args: &LaunchSessionsArgs) -> Result<(), CliError> {
    let transport = transport_for(options, AuthRequirement::BearerToken)?;
    let agent_ids = if args.agent_ids.is_empty() {
        transport
            .execute(ops::runtime_agents())
            .await?
            .agents
            .into_iter()
            .map(|agent| agent.id)
            .collect::<Vec<_>>()
    } else {
        args.agent_ids.clone()
    };
    let agent_refs = agent_ids.iter().map(String::as_str).collect::<Vec<_>>();
    let workspace_refs = args
        .workspace_paths
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>();
    let operation = ops::launch_sessions_batch(
        &agent_refs,
        &workspace_refs,
        Some(args.include_archived),
        args.limit,
    )?;

    if options.json {
        print_json(transport.execute_json(operation).await?)?;
        return Ok(());
    }

    for session in transport.execute(operation).await? {
        print_launch_session(session);
    }
    Ok(())
}

async fn run_launch_session_mutation(
    options: &Options,
    args: &LaunchSessionMutationArgs,
    archived: bool,
) -> Result<(), CliError> {
    let operation = if archived {
        ops::launch_session_archive(
            &args.agent_id,
            &args.session_id,
            args.workspace_path.as_deref(),
        )?
    } else {
        ops::launch_session_unarchive(
            &args.agent_id,
            &args.session_id,
            args.workspace_path.as_deref(),
        )?
    };
    run_unit(
        options,
        operation,
        if archived {
            "launch session archived"
        } else {
            "launch session unarchived"
        },
    )
    .await
}

fn print_launch_session(session: LaunchSessionInfo) {
    println!("{}", launch_session_line(&session));
}

fn launch_session_line(session: &LaunchSessionInfo) -> String {
    let state = if session.active {
        "active"
    } else if session.archived {
        "archived"
    } else {
        "available"
    };

    format!(
        "{}\t{}\t{}\t{}\t{}\t{}\t{}",
        session.agent_id,
        session.short_id,
        session.session_id,
        state,
        session.updated_at,
        session.workspace,
        session.title
    )
}

async fn run_session_create(options: &Options, create: &SessionCreateArgs) -> Result<(), CliError> {
    if create.attach && options.json {
        return Err(CliError::Usage(
            "session create --attach does not support --json".into(),
        ));
    }

    let transport = transport_for(options, AuthRequirement::BearerToken)?;
    let operation = ops::session_create(CreateSessionBody {
        tool: create.tool,
        profile_id: create.profile_id.as_deref(),
        launch_target: create.launch_target.as_deref(),
        resume_session_id: create.resume_session_id.as_deref(),
        project_path: create.project_path.as_deref(),
        tmux_session: create.tmux_session.as_deref(),
        theme: create.theme.as_deref(),
        cols: create.cols,
        rows: create.rows,
    })?;
    if options.json {
        print_json(transport.execute_json(operation).await?)?;
        return Ok(());
    }

    let session = transport.execute(operation).await?;
    if create.attach {
        eprintln!("created session {}", session.session_id);
        return attach::attach_session(options, &session.session_id).await;
    }

    println!("session: {}", session.session_id);
    println!("tool: {}", pty_tool_name(session.tool));
    println!("created_at: {}", session.created_at);
    if let Some(path) = session.project_path {
        println!("project: {path}");
    }
    if let Some(profile) = session.profile_label.or(session.profile_id) {
        println!("profile: {profile}");
    }
    if let Some(target) = session.launch_target {
        println!("target: {target}");
    }
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

fn pty_tool_name(tool: PtyTool) -> &'static str {
    match tool {
        PtyTool::Generic => "generic",
        PtyTool::Claude => "claude",
        PtyTool::Codex => "codex",
        PtyTool::Pi => "pi",
        PtyTool::Gemini => "gemini",
        PtyTool::OpenCode => "opencode",
        PtyTool::Cursor => "cursor",
        PtyTool::Kiro => "kiro",
        PtyTool::QwenCode => "qwen-code",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn launch_session_line_includes_full_session_id() {
        let line = launch_session_line(&LaunchSessionInfo {
            agent_id: "codex".to_string(),
            session_id: "full-session-id".to_string(),
            title: "Fix bug".to_string(),
            workspace: "/tmp/project".to_string(),
            updated_at: 42,
            short_id: "abc123".to_string(),
            archived: false,
            active: false,
        });

        assert_eq!(
            line,
            "codex\tabc123\tfull-session-id\tavailable\t42\t/tmp/project\tFix bug"
        );
    }
}
