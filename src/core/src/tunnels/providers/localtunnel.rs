//! Localtunnel: expose the web dashboard over the internet via a public URL.
//! In system mode, spawns `npx localtunnel --port <DEFAULT_PORT>`.
//! In VibeAround-managed mode, runs the managed `lt` npm entry with system Node.
//! Parses the public URL from stdout and keeps the process alive.
//! loca.lt uses the tunnel initiator's public IP as its anti-abuse password,
//! retrieved from `https://loca.lt/mytunnelpassword`.

use std::process::Stdio;
use tokio::io::{AsyncBufReadExt, BufReader};

use crate::proc_log;
use crate::process::registry::{ChildRegistry, ProcessKind};

const PORT: u16 = crate::config::DEFAULT_PORT;

/// Try to extract public URL from a line of localtunnel stdout (e.g. "your url is: https://xxx.loca.lt").
fn parse_url_from_line(line: &str) -> Option<String> {
    let line = line.trim();
    // Common patterns: "your url is: https://..." or "https://...loca.lt"
    if let Some(idx) = line.find("https://") {
        let rest = &line[idx..];
        let end = rest
            .find(|c: char| c.is_whitespace() || c == '\r' || c == '\n')
            .unwrap_or(rest.len());
        let url = rest[..end].trim_end_matches(['.', ',']);
        if url.starts_with("https://") && (url.contains("loca.lt") || url.contains("localtunnel")) {
            return Some(url.to_string());
        }
    }
    if let Some(idx) = line.find("http://") {
        let rest = &line[idx..];
        let end = rest
            .find(|c: char| c.is_whitespace() || c == '\r' || c == '\n')
            .unwrap_or(rest.len());
        let url = rest[..end].trim_end_matches(['.', ',']);
        if url.contains("loca.lt") || url.contains("localtunnel") {
            return Some(url.to_string());
        }
    }
    None
}

/// Start localtunnel for the given port. Returns (guard, public URL) once the URL is printed.
/// Caller must keep the guard and await `guard.wait()` to keep the tunnel alive.
pub async fn start(
    port: u16,
    config: &crate::config::Config,
) -> Result<(crate::tunnels::TunnelGuard, String), Box<dyn std::error::Error + Send + Sync>> {
    let tunnel_def =
        crate::resources::tunnel_by_id("localtunnel").expect("localtunnel not in tunnels.json");
    let (program, mut args) = localtunnel_command(tunnel_def, config)?;
    args.push(port.to_string());

    let mut cmd = crate::process::env::command(&program);
    cmd.args(&args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);

    let error_hint =
        crate::resources::tunnel_spawn_error_hint(tunnel_def).unwrap_or("is Node/npx installed?");
    let mut child = cmd
        .spawn()
        .map_err(|e| format!("Failed to spawn {} ({}): {}", program, error_hint, e))?;
    let pid = child.id();

    let stdout = child
        .stdout
        .take()
        .ok_or("localtunnel stdout not captured")?;

    let reader = BufReader::new(stdout);
    let mut lines = reader.lines();
    let url = loop {
        let line = lines
            .next_line()
            .await
            .map_err(|e| format!("Reading localtunnel stdout: {}", e))?
            .ok_or("localtunnel closed stdout before printing URL")?;
        if let Some(u) = parse_url_from_line(&line) {
            break u;
        }
    };

    // Register after stdout yields the public URL.
    let registry_id = ChildRegistry::global().register(ProcessKind::Tunnel, "localtunnel", child);

    proc_log!(
        info,
        kind = ProcessKind::Tunnel,
        label = "localtunnel",
        pid = pid,
        event = "started",
        url = %url
    );

    Ok((crate::tunnels::TunnelGuard::Process { registry_id }, url))
}

/// Start tunnel for the default web dashboard port.
pub async fn start_web_tunnel(
    config: &crate::config::Config,
) -> Result<(crate::tunnels::TunnelGuard, String), Box<dyn std::error::Error + Send + Sync>> {
    start(PORT, config).await
}

fn localtunnel_command(
    tunnel_def: &crate::resources::TunnelDef,
    config: &crate::config::Config,
) -> Result<(String, Vec<String>), Box<dyn std::error::Error + Send + Sync>> {
    if config.toolchain_mode.is_managed() {
        let dependency_id = tunnel_def
            .dependency_id
            .as_deref()
            .ok_or("localtunnel managed dependency is not configured")?;
        let install_dir = crate::plugins::user_plugin_dependency_dir(dependency_id);
        let entry = crate::process::env::resolve_npm_bin_in_dir(&install_dir, "lt")?;
        return Ok((
            "node".to_string(),
            vec![entry.to_string_lossy().to_string(), "--port".to_string()],
        ));
    }

    let program = tunnel_def.program.as_deref().unwrap_or("npx").to_string();
    let args = tunnel_def
        .args
        .as_ref()
        .cloned()
        .unwrap_or_else(|| vec!["localtunnel".to_string(), "--port".to_string()]);
    Ok((program, args))
}

/// Localtunnel backend. Implements TunnelBackend for unified dispatch.
pub struct LocaltunnelBackend;

#[async_trait::async_trait]
impl crate::tunnels::TunnelBackend for LocaltunnelBackend {
    fn name(&self) -> &'static str {
        "localtunnel"
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
