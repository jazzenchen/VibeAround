//! Localtunnel: expose the web dashboard over the internet via a public URL.
//! In system mode, spawns `npx localtunnel --port <DEFAULT_PORT>`.
//! In VibeAround-managed mode, runs the managed `lt` npm entry with system Node.
//! The public URL is parsed from stdout by the tunnel bridge.
//! loca.lt uses the tunnel initiator's public IP as its anti-abuse password,
//! retrieved from `https://loca.lt/mytunnelpassword`.

use crate::process::supervisor::SpawnSpec;
use crate::tunnels::bridge::UrlDiscovery;
use crate::tunnels::TunnelPlan;

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

/// Build the launch plan for localtunnel on the web dashboard port.
pub(crate) fn plan(
    config: &crate::config::Config,
) -> Result<TunnelPlan, Box<dyn std::error::Error + Send + Sync>> {
    let tunnel_def =
        crate::resources::tunnel_by_id("localtunnel").expect("localtunnel not in tunnels.json");
    let (program, mut args) = localtunnel_command(tunnel_def, config)?;
    args.push(PORT.to_string());

    Ok(TunnelPlan {
        spec: SpawnSpec::new(program).args(args),
        url: UrlDiscovery::FromStdout {
            parse_url: parse_url_from_line,
            parse_approval: None,
        },
    })
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

#[cfg(test)]
mod tests {
    use super::parse_url_from_line;

    #[test]
    fn parses_standard_url_line() {
        assert_eq!(
            parse_url_from_line("your url is: https://brave-cat-42.loca.lt"),
            Some("https://brave-cat-42.loca.lt".to_string())
        );
    }

    #[test]
    fn ignores_unrelated_lines() {
        assert_eq!(parse_url_from_line("tunnel starting on port 12358"), None);
    }
}
