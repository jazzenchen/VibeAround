//! Standalone VibeAround server binary — starts the ServerDaemon from the command line.

fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let boot = match server::boot::ServerBootConfig::from_env_and_args(std::env::args().skip(1)) {
        Ok(boot) => boot,
        Err(message) => {
            eprintln!("{message}");
            if message.starts_with("Usage:") {
                return Ok(());
            }
            std::process::exit(2);
        }
    };
    boot.apply_process_env();
    common::logging::init();
    common::migration::run()?;
    let daemon = server::ServerDaemon::new(boot.port);

    let rt = tokio::runtime::Runtime::new()?;
    rt.block_on(async {
        if let Err(e) = daemon.start(boot.web_dist_path).await {
            tracing::info!("[VibeAround] Fatal: {}", e);
        }
    });

    std::process::exit(0);
}
