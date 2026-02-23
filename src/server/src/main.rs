//! Standalone VibeAround server binary. Run with --port and --dist, or use defaults.

use std::path::PathBuf;

use common::config;

const DEFAULT_PORT: u16 = 5182;

fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let _ = config::ensure_loaded();
    let mut port = DEFAULT_PORT;
    let mut dist: Option<PathBuf> = None;

    let args: Vec<String> = std::env::args().collect();
    let mut i = 1;
    while i < args.len() {
        if args[i] == "--port" && i + 1 < args.len() {
            port = args[i + 1].parse().unwrap_or(DEFAULT_PORT);
            i += 2;
            continue;
        }
        if args[i] == "--dist" && i + 1 < args.len() {
            dist = Some(PathBuf::from(&args[i + 1]));
            i += 2;
            continue;
        }
        i += 1;
    }

    let dist_path = dist.unwrap_or_else(|| {
        // Default: web/dist when run from workspace root (src/), so server serves the web dashboard SPA
        PathBuf::from("web").join("dist")
    });

    let rt = tokio::runtime::Runtime::new()?;
    rt.block_on(server::run_web_server(port, dist_path, None))
}
