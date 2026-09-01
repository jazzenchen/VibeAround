mod args;
mod auth;
mod chat_store;
mod commands;
mod config;
mod error;
mod pair;
mod transport;

use std::env;

use args::{parse_args, usage};
use error::CliError;

pub(crate) use commands::print_json;

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
    commands::dispatch(&options, command).await
}
