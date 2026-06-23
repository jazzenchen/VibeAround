use clap::{Args, Subcommand};

use super::Command;

#[derive(Debug, Clone, PartialEq, Eq, Args)]
pub(crate) struct PairStartArgs {
    #[arg(
        long,
        default_value_t = false,
        action = clap::ArgAction::Set,
        num_args = 0..=1,
        require_equals = true,
        default_missing_value = "true",
        value_parser = clap::builder::BoolishValueParser::new()
    )]
    pub(crate) wait: bool,
    #[arg(
        long,
        default_value_t = false,
        action = clap::ArgAction::Set,
        num_args = 0..=1,
        require_equals = true,
        default_missing_value = "true",
        value_parser = clap::builder::BoolishValueParser::new()
    )]
    pub(crate) save: bool,
    #[arg(long = "timeout", alias = "timeout-secs", default_value_t = 60, value_parser = super::parse_positive_u64)]
    pub(crate) timeout_secs: u64,
    #[arg(long, default_value_t = 2_000, value_parser = super::parse_positive_u64)]
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

#[derive(Debug, Clone, PartialEq, Eq, Args)]
pub(crate) struct PairStatusArgs {
    pub(crate) sid: String,
    #[arg(
        long,
        default_value_t = false,
        action = clap::ArgAction::Set,
        num_args = 0..=1,
        require_equals = true,
        default_missing_value = "true",
        value_parser = clap::builder::BoolishValueParser::new()
    )]
    pub(crate) save: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Args)]
pub(crate) struct PairWaitArgs {
    pub(crate) sid: String,
    #[arg(
        long,
        default_value_t = false,
        action = clap::ArgAction::Set,
        num_args = 0..=1,
        require_equals = true,
        default_missing_value = "true",
        value_parser = clap::builder::BoolishValueParser::new()
    )]
    pub(crate) save: bool,
    #[arg(long = "timeout", alias = "timeout-secs", default_value_t = 60, value_parser = super::parse_positive_u64)]
    pub(crate) timeout_secs: u64,
    #[arg(long, default_value_t = 2_000, value_parser = super::parse_positive_u64)]
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

#[derive(Debug, Clone, PartialEq, Eq, Subcommand)]
#[command(rename_all = "kebab-case")]
pub(crate) enum PairCommand {
    Start(PairStartArgs),
    Status(PairStatusArgs),
    Wait(PairWaitArgs),
}

impl PairCommand {
    pub(super) fn into_command(self) -> Command {
        match self {
            Self::Start(mut args) => {
                if args.save {
                    args.wait = true;
                }
                Command::PairStart(args)
            }
            Self::Status(args) => Command::PairStatus {
                sid: args.sid,
                save: args.save,
            },
            Self::Wait(args) => Command::PairWait(args),
        }
    }
}
