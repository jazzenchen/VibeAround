//! Ngrok: expose the web dashboard via the ngrok agent (`ngrok http`).
//! Token from global config through the `NGROK_AUTHTOKEN` environment
//! variable so it never appears in argv; optional static domain from
//! config. The public URL is parsed from the agent's JSON logs on stdout
//! by the tunnel bridge.

use crate::process::supervisor::SpawnSpec;
use crate::tunnels::bridge::UrlDiscovery;
use crate::tunnels::TunnelPlan;

const PORT: u16 = crate::config::DEFAULT_PORT;

/// Extract the public URL from one ngrok JSON log line
/// (`{"msg":"started tunnel","url":"https://…", …}`).
fn parse_url_from_line(line: &str) -> Option<String> {
    let value: serde_json::Value = serde_json::from_str(line.trim()).ok()?;
    if value.get("msg")?.as_str()? != "started tunnel" {
        return None;
    }
    let url = value.get("url")?.as_str()?;
    url.starts_with("https://").then(|| url.to_string())
}

/// Build the launch plan for the ngrok agent on the web dashboard port.
pub(crate) fn plan(
    config: &crate::config::Config,
) -> Result<TunnelPlan, Box<dyn std::error::Error + Send + Sync>> {
    let token = config.ngrok_auth_token.as_deref().ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "ngrok token not set: set tunnel.ngrok.auth_token in settings.json",
        )
    })?;

    let tunnel_def = crate::resources::tunnel_by_id("ngrok").expect("ngrok not in tunnels.json");
    let program = tunnel_def.program.as_deref().unwrap_or("ngrok");

    let mut args = vec![
        "http".to_string(),
        PORT.to_string(),
        "--log".to_string(),
        "stdout".to_string(),
        "--log-format".to_string(),
        "json".to_string(),
    ];
    if let Some(domain) = config
        .ngrok_domain
        .as_deref()
        .map(str::trim)
        .filter(|domain| !domain.is_empty())
    {
        args.push("--domain".to_string());
        args.push(domain.to_string());
    }

    // The retired ngrok Rust SDK never consulted proxy environment
    // variables, and the agent refuses to run behind an HTTP proxy on the
    // free plan — strip them so the migration stays behavior-neutral.
    let mut spec = SpawnSpec::new(program)
        .args(args)
        .env("NGROK_AUTHTOKEN", token);
    for key in [
        "http_proxy",
        "https_proxy",
        "all_proxy",
        "HTTP_PROXY",
        "HTTPS_PROXY",
        "ALL_PROXY",
    ] {
        spec = spec.env_remove(key);
    }

    Ok(TunnelPlan {
        spec,
        url: UrlDiscovery::FromStdout {
            parse_url: parse_url_from_line,
            parse_approval: None,
        },
    })
}

#[cfg(test)]
mod tests {
    use super::parse_url_from_line;

    #[test]
    fn parses_started_tunnel_line() {
        let line = r#"{"addr":"http://localhost:12358","lvl":"info","msg":"started tunnel","name":"command_line","obj":"tunnels","t":"2026-09-02T10:00:00+0800","url":"https://assured-desired-penguin.ngrok-free.app"}"#;
        assert_eq!(
            parse_url_from_line(line),
            Some("https://assured-desired-penguin.ngrok-free.app".to_string())
        );
    }

    #[test]
    fn ignores_other_log_lines() {
        assert_eq!(
            parse_url_from_line(r#"{"lvl":"info","msg":"client session established"}"#),
            None
        );
        assert_eq!(parse_url_from_line("plain text output"), None);
    }
}
