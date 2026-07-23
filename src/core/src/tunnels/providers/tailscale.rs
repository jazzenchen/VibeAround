//! Tailscale Funnel: expose the web dashboard through the local Tailscale CLI.

use std::process::Stdio;

use tokio::io::{AsyncBufReadExt, BufReader};

use crate::proc_log;
use crate::process::registry::{ChildRegistry, ProcessKind};

const PORT: u16 = crate::config::DEFAULT_PORT;

fn parse_funnel_url(line: &str) -> Option<String> {
    let candidate = parse_https_url(line)?;
    let url = reqwest::Url::parse(&candidate).ok()?;
    let host = url.host_str()?;
    if url.scheme() != "https" || !host.ends_with(".ts.net") {
        return None;
    }
    Some(candidate)
}

fn parse_approval_url(line: &str) -> Option<String> {
    let candidate = parse_https_url(line)?;
    let url = reqwest::Url::parse(&candidate).ok()?;
    if url.host_str()? != "login.tailscale.com" || !url.path().starts_with("/f/funnel") {
        return None;
    }
    Some(candidate)
}

fn parse_https_url(line: &str) -> Option<String> {
    let start = line.find("https://")?;
    let candidate = line[start..]
        .split_whitespace()
        .next()?
        .trim_end_matches(['.', ',', ')']);
    Some(candidate.to_string())
}

pub async fn start(
    port: u16,
    approval_reporter: Option<crate::tunnels::TunnelApprovalReporter>,
) -> Result<(crate::tunnels::TunnelGuard, String), Box<dyn std::error::Error + Send + Sync>> {
    let tunnel_def =
        crate::resources::tunnel_by_id("tailscale").expect("tailscale not in tunnels.json");
    let program = tunnel_def.program.as_deref().unwrap_or("tailscale");
    let args = tunnel_def
        .args
        .as_ref()
        .cloned()
        .unwrap_or_else(|| vec!["funnel".to_string(), "--yes".to_string()]);

    let mut cmd = crate::process::env::command(program);
    cmd.args(&args)
        .arg(format!("http://127.0.0.1:{port}"))
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .kill_on_drop(true);

    let error_hint =
        crate::resources::tunnel_spawn_error_hint(tunnel_def).unwrap_or("is Tailscale installed?");
    let mut child = cmd
        .spawn()
        .map_err(|error| format!("Failed to spawn {program} ({error_hint}): {error}"))?;
    let pid = child.id();
    let stdout = child.stdout.take().ok_or("tailscale stdout not captured")?;
    let mut lines = BufReader::new(stdout).lines();

    let url = loop {
        let Some(line) = lines
            .next_line()
            .await
            .map_err(|error| format!("Reading tailscale stdout: {error}"))?
        else {
            let status = child.wait().await?;
            return Err(format!(
                "tailscale funnel exited before reporting its public URL ({status})"
            )
            .into());
        };
        if let Some(url) = parse_funnel_url(&line) {
            break url;
        }
        if let Some(approval_url) = parse_approval_url(&line) {
            if let Some(report) = &approval_reporter {
                report(approval_url);
            }
            continue;
        }
        if !line.trim().is_empty() {
            tracing::info!(provider = "tailscale", output = %line, "tailscale funnel setup");
        }
    };

    let registry_id = ChildRegistry::global().register(ProcessKind::Tunnel, "tailscale", child);
    proc_log!(
        info,
        kind = ProcessKind::Tunnel,
        label = "tailscale",
        pid = pid,
        event = "started",
        url = %url
    );

    Ok((crate::tunnels::TunnelGuard::Process { registry_id }, url))
}

pub async fn start_web_tunnel(
    _config: &crate::config::Config,
    approval_reporter: Option<crate::tunnels::TunnelApprovalReporter>,
) -> Result<(crate::tunnels::TunnelGuard, String), Box<dyn std::error::Error + Send + Sync>> {
    start(PORT, approval_reporter).await
}

pub struct TailscaleBackend;

#[async_trait::async_trait]
impl crate::tunnels::TunnelBackend for TailscaleBackend {
    fn name(&self) -> &'static str {
        "tailscale"
    }

    async fn start_web_tunnel(
        &self,
        config: &crate::config::Config,
        approval_reporter: Option<crate::tunnels::TunnelApprovalReporter>,
    ) -> Result<(crate::tunnels::TunnelGuard, String), Box<dyn std::error::Error + Send + Sync>>
    {
        start_web_tunnel(config, approval_reporter).await
    }
}

#[cfg(test)]
mod tests {
    use super::{parse_approval_url, parse_funnel_url};

    #[test]
    fn parses_public_funnel_url() {
        assert_eq!(
            parse_funnel_url("https://workstation.example-tailnet.ts.net"),
            Some("https://workstation.example-tailnet.ts.net".to_string())
        );
    }

    #[test]
    fn rejects_funnel_approval_url() {
        assert_eq!(
            parse_funnel_url("To enable, visit: https://login.tailscale.com/f/funnel?node=abc"),
            None
        );
    }

    #[test]
    fn parses_funnel_approval_url() {
        assert_eq!(
            parse_approval_url("https://login.tailscale.com/f/funnel?node=abc"),
            Some("https://login.tailscale.com/f/funnel?node=abc".to_string())
        );
    }

    #[test]
    fn parses_url_from_status_line() {
        assert_eq!(
            parse_funnel_url("Available on the internet: https://host.tailnet.ts.net/"),
            Some("https://host.tailnet.ts.net/".to_string())
        );
    }
}
