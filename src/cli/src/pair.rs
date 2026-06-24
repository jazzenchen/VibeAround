use std::{
    path::PathBuf,
    time::{Duration, Instant},
};

use serde_json::Value;
use va_client::auth::{PairStartResponse, PairStatusResponse};
use va_client::http::AuthRequirement;
use va_client::ops;

use crate::args::{Options, PairStartArgs, PairWaitArgs};
use crate::config::{
    endpoint_for, local_auth_port, resolve_endpoint_env, save_auth_file, RuntimeEnv,
};
use crate::error::CliError;
use crate::transport::HttpTransport;

pub(crate) async fn start(options: &Options, args: &PairStartArgs) -> Result<(), CliError> {
    let transport = transport_for(options, AuthRequirement::None)?;
    let pair = transport.execute(ops::pair_start()).await?;
    if !args.wait {
        if options.json {
            crate::print_json(pair_start_json(&pair))?;
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

    let verified = wait_for_verification(
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
        crate::print_json(value)?;
        return Ok(());
    }
    print_pair_verified(&verified);
    Ok(())
}

pub(crate) async fn status(options: &Options, sid: &str, save: bool) -> Result<(), CliError> {
    let transport = transport_for(options, AuthRequirement::None)?;
    let status = transport.execute(ops::pair_status(sid)).await?;
    let saved_path = if save {
        match &status {
            PairStatusResponse::Verified { token } => Some(save_verified_token(options, token)?),
            PairStatusResponse::Pending | PairStatusResponse::Expired => None,
        }
    } else {
        None
    };

    if options.json {
        crate::print_json(pair_status_json(&status, saved_path.as_ref()))?;
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

pub(crate) async fn wait(options: &Options, args: &PairWaitArgs) -> Result<(), CliError> {
    if !options.json {
        println!("waiting: {}s", args.timeout_secs);
    }
    let verified = wait_for_verification(
        options,
        &args.sid,
        args.save,
        args.timeout_secs,
        args.interval_ms,
    )
    .await?;
    if options.json {
        crate::print_json(pair_verified_json(&verified))?;
        return Ok(());
    }
    print_pair_verified(&verified);
    Ok(())
}

struct VerifiedPair {
    token: String,
    saved_path: Option<PathBuf>,
}

async fn wait_for_verification(
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
                    Some(save_verified_token(options, &token)?)
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

fn pair_status_json(status: &PairStatusResponse, saved_path: Option<&PathBuf>) -> Value {
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

fn print_pair_verified(verified: &VerifiedPair) {
    println!("verified");
    if let Some(path) = &verified.saved_path {
        println!("saved: {}", path.display());
    } else {
        println!("token: {}", verified.token);
    }
}

fn save_verified_token(options: &Options, token: &str) -> Result<PathBuf, CliError> {
    let public_endpoint =
        resolve_endpoint_env(options, AuthRequirement::None, &RuntimeEnv::current())?;
    let port = local_auth_port(public_endpoint.endpoint.base_url())?;
    save_auth_file(options, port, token)
}

fn transport_for(options: &Options, auth: AuthRequirement) -> Result<HttpTransport, CliError> {
    Ok(HttpTransport::new(endpoint_for(options, auth)?))
}
