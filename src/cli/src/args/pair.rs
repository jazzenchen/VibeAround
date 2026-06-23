use crate::error::CliError;

use super::{next_ref, parse_bool, parse_u64, Command};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PairStartArgs {
    pub(crate) wait: bool,
    pub(crate) save: bool,
    pub(crate) timeout_secs: u64,
    pub(crate) interval_ms: u64,
}

impl Default for PairStartArgs {
    fn default() -> Self {
        Self {
            wait: false,
            save: false,
            timeout_secs: 60,
            interval_ms: 2_000,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PairWaitArgs {
    pub(crate) sid: String,
    pub(crate) save: bool,
    pub(crate) timeout_secs: u64,
    pub(crate) interval_ms: u64,
}

impl Default for PairWaitArgs {
    fn default() -> Self {
        Self {
            sid: String::new(),
            save: false,
            timeout_secs: 60,
            interval_ms: 2_000,
        }
    }
}

pub(super) fn parse_pair_command(args: &[String]) -> Result<Command, CliError> {
    match args {
        [action, rest @ ..] if action == "start" => {
            parse_pair_start_args(rest).map(Command::PairStart)
        }
        [action, rest @ ..] if action == "status" => parse_pair_status_args(rest),
        [action, rest @ ..] if action == "wait" => {
            parse_pair_wait_args(rest).map(Command::PairWait)
        }
        _ => Err(CliError::Usage(
            "usage: va pair start [--wait] [--save]; va pair status SID [--save]; va pair wait SID [--save]".into(),
        )),
    }
}

fn parse_pair_start_args(args: &[String]) -> Result<PairStartArgs, CliError> {
    let mut parsed = PairStartArgs::default();
    let mut args = args.iter().peekable();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--wait" => parsed.wait = true,
            "--save" => {
                parsed.save = true;
                parsed.wait = true;
            }
            "--timeout" | "--timeout-secs" => {
                parsed.timeout_secs = parse_u64(next_ref(&mut args, arg)?, arg)?;
            }
            "--interval-ms" => {
                parsed.interval_ms = parse_u64(next_ref(&mut args, arg)?, arg)?;
            }
            value if value.starts_with("--wait=") => {
                parsed.wait = parse_bool(value.trim_start_matches("--wait="), "--wait")?;
            }
            value if value.starts_with("--save=") => {
                parsed.save = parse_bool(value.trim_start_matches("--save="), "--save")?;
                if parsed.save {
                    parsed.wait = true;
                }
            }
            value if value.starts_with("--timeout=") => {
                parsed.timeout_secs =
                    parse_u64(value.trim_start_matches("--timeout="), "--timeout")?;
            }
            value if value.starts_with("--timeout-secs=") => {
                parsed.timeout_secs = parse_u64(
                    value.trim_start_matches("--timeout-secs="),
                    "--timeout-secs",
                )?;
            }
            value if value.starts_with("--interval-ms=") => {
                parsed.interval_ms =
                    parse_u64(value.trim_start_matches("--interval-ms="), "--interval-ms")?;
            }
            value if value.starts_with('-') => {
                return Err(CliError::Usage(format!(
                    "unknown pair start option: {value}"
                )));
            }
            value => return Err(CliError::Usage(format!("unexpected argument: {value}"))),
        }
    }
    Ok(parsed)
}

fn parse_pair_status_args(args: &[String]) -> Result<Command, CliError> {
    let mut sid = None;
    let mut save = false;
    for arg in args {
        match arg.as_str() {
            "--save" => save = true,
            value if value.starts_with("--save=") => {
                save = parse_bool(value.trim_start_matches("--save="), "--save")?;
            }
            value if value.starts_with('-') => {
                return Err(CliError::Usage(format!(
                    "unknown pair status option: {value}"
                )));
            }
            value => {
                if sid.is_some() {
                    return Err(CliError::Usage("usage: va pair status SID [--save]".into()));
                }
                sid = Some(value.to_string());
            }
        }
    }
    Ok(Command::PairStatus {
        sid: sid.ok_or_else(|| CliError::Usage("usage: va pair status SID [--save]".into()))?,
        save,
    })
}

fn parse_pair_wait_args(args: &[String]) -> Result<PairWaitArgs, CliError> {
    let mut parsed = PairWaitArgs::default();
    let mut args = args.iter().peekable();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--save" => parsed.save = true,
            "--timeout" | "--timeout-secs" => {
                parsed.timeout_secs = parse_u64(next_ref(&mut args, arg)?, arg)?;
            }
            "--interval-ms" => {
                parsed.interval_ms = parse_u64(next_ref(&mut args, arg)?, arg)?;
            }
            value if value.starts_with("--save=") => {
                parsed.save = parse_bool(value.trim_start_matches("--save="), "--save")?;
            }
            value if value.starts_with("--timeout=") => {
                parsed.timeout_secs =
                    parse_u64(value.trim_start_matches("--timeout="), "--timeout")?;
            }
            value if value.starts_with("--timeout-secs=") => {
                parsed.timeout_secs = parse_u64(
                    value.trim_start_matches("--timeout-secs="),
                    "--timeout-secs",
                )?;
            }
            value if value.starts_with("--interval-ms=") => {
                parsed.interval_ms =
                    parse_u64(value.trim_start_matches("--interval-ms="), "--interval-ms")?;
            }
            value if value.starts_with('-') => {
                return Err(CliError::Usage(format!(
                    "unknown pair wait option: {value}"
                )));
            }
            value => {
                if !parsed.sid.is_empty() {
                    return Err(CliError::Usage("usage: va pair wait SID [--save]".into()));
                }
                parsed.sid = value.to_string();
            }
        }
    }
    if parsed.sid.is_empty() {
        return Err(CliError::Usage("usage: va pair wait SID [--save]".into()));
    }
    Ok(parsed)
}
