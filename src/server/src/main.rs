//! Standalone VibeAround server binary — starts the ServerDaemon from the command line.

use std::path::PathBuf;

fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let daemon = server::ServerDaemon::new(common::config::DEFAULT_PORT);
    let dist_path = PathBuf::from("web").join("dist");

    let rt = tokio::runtime::Runtime::new()?;
    rt.block_on(async {
        if let Err(e) = daemon.start(dist_path).await {
            eprintln!("[VibeAround] Fatal: {}", e);
        }
    });

    std::process::exit(0);
}
