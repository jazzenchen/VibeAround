//! Tailscale Funnel: expose the web dashboard through the local Tailscale CLI.
//! The public URL — and the one-time Funnel approval link, when the tailnet
//! still needs it — are parsed from stdout by the tunnel bridge.

use crate::process::supervisor::SpawnSpec;
use crate::tunnels::bridge::UrlDiscovery;
use crate::tunnels::TunnelPlan;

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

/// Build the launch plan for `tailscale funnel` on the web dashboard port.
pub(crate) fn plan() -> Result<TunnelPlan, Box<dyn std::error::Error + Send + Sync>> {
    let tunnel_def =
        crate::resources::tunnel_by_id("tailscale").expect("tailscale not in tunnels.json");
    let program = tunnel_def.program.as_deref().unwrap_or("tailscale");
    let mut args = tunnel_def
        .args
        .as_ref()
        .cloned()
        .unwrap_or_else(|| vec!["funnel".to_string(), "--yes".to_string()]);
    args.push(format!("http://127.0.0.1:{PORT}"));

    Ok(TunnelPlan {
        spec: SpawnSpec::new(program).args(args),
        url: UrlDiscovery::FromStdout {
            parse_url: parse_funnel_url,
            parse_approval: Some(parse_approval_url),
        },
    })
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
