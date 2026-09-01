//! Cloudflare Tunnel: expose the web dashboard via `cloudflared tunnel run`,
//! passing the token through the `TUNNEL_TOKEN` environment variable so it
//! never appears in the process argv (visible to every local process).
//! The public URL comes from `tunnel.cloudflare.hostname`.

use std::path::PathBuf;

use crate::process::supervisor::SpawnSpec;
use crate::tunnels::bridge::UrlDiscovery;
use crate::tunnels::TunnelPlan;

/// Build the launch plan for the Cloudflare tunnel. Named Tunnels have a
/// fixed hostname, so the public URL is known before the child starts.
pub(crate) fn plan(
    config: &crate::config::Config,
) -> Result<TunnelPlan, Box<dyn std::error::Error + Send + Sync>> {
    let token = config.cloudflare_tunnel_token.as_deref().ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "cloudflare token not set: set tunnel.cloudflare.tunnel_token in settings.json",
        )
    })?;

    let hostname = config.cloudflare_hostname.as_deref().ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "cloudflare hostname not set: set tunnel.cloudflare.hostname in settings.json (e.g. vibe.yourdomain.com)",
        )
    })?;

    let tunnel_def = crate::resources::tunnel_by_id("cloudflare")
        .expect("cloudflare tunnel not in tunnels.json");
    let program = tunnel_def.program.as_deref().unwrap_or("cloudflared");
    let resolved_program = resolve_cloudflared_program(tunnel_def, program, config)?;
    let args: Vec<String> = tunnel_def
        .args
        .clone()
        .unwrap_or_else(|| vec!["tunnel".to_string(), "run".to_string()]);

    let url = format!(
        "https://{}",
        hostname
            .trim_start_matches("https://")
            .trim_start_matches("http://")
    );

    Ok(TunnelPlan {
        spec: SpawnSpec::new(resolved_program.to_string_lossy())
            .args(args)
            .env("TUNNEL_TOKEN", token),
        url: UrlDiscovery::Known(url),
    })
}

fn resolve_cloudflared_program(
    tunnel_def: &crate::resources::TunnelDef,
    program: &str,
    config: &crate::config::Config,
) -> Result<PathBuf, Box<dyn std::error::Error + Send + Sync>> {
    if config.toolchain_mode.is_managed() {
        let dependency_id = tunnel_def
            .dependency_id
            .as_deref()
            .ok_or("cloudflare managed dependency is not configured")?;
        return Ok(crate::plugins::user_plugin_dependency_bin_path(
            dependency_id,
            program,
        ));
    }
    Ok(PathBuf::from(program))
}
