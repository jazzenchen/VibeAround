//! Cloudflare Tunnel: expose the web dashboard via `cloudflared tunnel run --token <TOKEN>`.
//! The public URL is configured in Cloudflare Dashboard (Public Hostname), so we read it from
//! settings.json `tunnel.cloudflare.hostname` instead of parsing stdout.

use std::path::PathBuf;
use std::process::Stdio;

use crate::proc_log;
use crate::process::registry::{ChildRegistry, ProcessKind};

/// Start Cloudflare tunnel. Returns (guard, public URL).
/// Token from config; hostname (public URL) from config since Cloudflare Named Tunnels have a fixed URL.
pub async fn start_web_tunnel(
    config: &crate::config::Config,
) -> Result<(crate::tunnels::TunnelGuard, String), Box<dyn std::error::Error + Send + Sync>> {
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
    let base_args: Vec<&str> = tunnel_def
        .args
        .as_ref()
        .map(|a| a.iter().map(|s| s.as_str()).collect())
        .unwrap_or_else(|| vec!["tunnel", "run", "--token"]);

    let resolved_program_string = resolved_program.to_string_lossy().to_string();
    let mut cmd = crate::process::env::command(&resolved_program_string);
    cmd.args(&base_args)
        .arg(token)
        .stdout(Stdio::null())
        .stderr(Stdio::inherit());

    cmd.kill_on_drop(true);
    let error_hint =
        crate::resources::tunnel_spawn_error_hint(tunnel_def).unwrap_or("is it installed?");
    let child = cmd
        .spawn()
        .map_err(|e| format!("Failed to spawn {} ({}): {}", program, error_hint, e))?;
    let pid = child.id();

    // Transfer Child ownership to the registry so daemon shutdown's
    // kill_all() reaches it even if the outer task never gets a chance
    // to reach guard.wait(). See TunnelGuard::wait for the happy-path
    // reaper.
    let registry_id = ChildRegistry::global().register(ProcessKind::Tunnel, "cloudflare", child);

    let url = format!(
        "https://{}",
        hostname
            .trim_start_matches("https://")
            .trim_start_matches("http://")
    );
    proc_log!(
        info,
        kind = ProcessKind::Tunnel,
        label = "cloudflare",
        pid = pid,
        event = "started",
        url = %url
    );

    Ok((crate::tunnels::TunnelGuard::Process { registry_id }, url))
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

/// Cloudflare backend. Implements TunnelBackend for unified dispatch.
pub struct CloudflareBackend;

#[async_trait::async_trait]
impl crate::tunnels::TunnelBackend for CloudflareBackend {
    fn name(&self) -> &'static str {
        "cloudflare"
    }

    async fn start_web_tunnel(
        &self,
        config: &crate::config::Config,
        _approval_reporter: Option<crate::tunnels::TunnelApprovalReporter>,
    ) -> Result<(crate::tunnels::TunnelGuard, String), Box<dyn std::error::Error + Send + Sync>>
    {
        start_web_tunnel(config).await
    }
}
