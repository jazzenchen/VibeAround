//! Localtunnel: expose the web dashboard (and xterm) over the internet via a public URL.
//! Spawns `npx localtunnel --port 5182` (or bunx), parses the public URL from stdout, keeps process alive.
//! Tunnel password: loca.lt uses the tunnel initiator's public IP as the "password" (anti-abuse).
//! There is no SDK to get it; the only way is to GET https://loca.lt/mytunnelpassword from the same
//! machine running the tunnel — we do that and parse the IP from the response.

use std::process::Stdio;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;

const PORT: u16 = 5182;

/// Try to extract public URL from a line of localtunnel stdout (e.g. "your url is: https://xxx.loca.lt").
fn parse_url_from_line(line: &str) -> Option<String> {
    let line = line.trim();
    // Common patterns: "your url is: https://..." or "https://...loca.lt"
    if let Some(idx) = line.find("https://") {
        let rest = &line[idx..];
        let end = rest
            .find(|c: char| c.is_whitespace() || c == '\r' || c == '\n')
            .unwrap_or(rest.len());
        let url = rest[..end].trim_end_matches(|c| c == '.' || c == ',');
        if url.starts_with("https://") && (url.contains("loca.lt") || url.contains("localtunnel")) {
            return Some(url.to_string());
        }
    }
    if let Some(idx) = line.find("http://") {
        let rest = &line[idx..];
        let end = rest
            .find(|c: char| c.is_whitespace() || c == '\r' || c == '\n')
            .unwrap_or(rest.len());
        let url = rest[..end].trim_end_matches(|c| c == '.' || c == ',');
        if url.contains("loca.lt") || url.contains("localtunnel") {
            return Some(url.to_string());
        }
    }
    None
}

/// Start localtunnel for the given port. Returns (guard, public URL) once the URL is printed.
/// Caller must keep the guard and await `guard.wait()` to keep the tunnel alive.
pub async fn start(port: u16) -> Result<(super::TunnelGuard, String), Box<dyn std::error::Error + Send + Sync>> {
    let mut cmd = Command::new("npx");
    cmd.args(["localtunnel", "--port", port.to_string().as_str()])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let mut child = cmd.spawn().map_err(|e| {
        format!(
            "Failed to spawn localtunnel (is Node/npx installed?): {}",
            e
        )
    })?;

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

    Ok((super::TunnelGuard::Process(child), url))
}

/// Start tunnel for the default web dashboard port.
pub async fn start_web_tunnel() -> Result<(super::TunnelGuard, String), Box<dyn std::error::Error + Send + Sync>> {
    start(PORT).await
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
        _config: &crate::config::Config,
    ) -> Result<(super::TunnelGuard, String), Box<dyn std::error::Error + Send + Sync>> {
        start_web_tunnel().await
    }
}

/// URL to obtain tunnel password when running on this machine (loca.lt returns your public IP as the password).
pub const TUNNEL_PASSWORD_URL: &str = "https://loca.lt/mytunnelpassword";

/// Request header to bypass loca.lt tunnel reminder page (any value).
pub const BYPASS_TUNNEL_REMINDER_HEADER: &str = "bypass-tunnel-reminder";
pub const BYPASS_TUNNEL_REMINDER_VALUE: &str = "1";
/// Non-standard User-Agent so requests bypass the reminder (alternative to the header).
pub const TUNNEL_BYPASS_USER_AGENT: &str = "VibeAround/1.0";

/// Fetch the tunnel password by requesting loca.lt from the same machine (same IP as the tunnel).
pub async fn fetch_tunnel_password() -> Result<Option<String>, Box<dyn std::error::Error + Send + Sync>> {
    let client = reqwest::Client::builder()
        .build()
        .map_err(|e| format!("reqwest client: {}", e))?;
    let resp = client
        .get(TUNNEL_PASSWORD_URL)
        .send()
        .await
        .map_err(|e| format!("GET {}: {}", TUNNEL_PASSWORD_URL, e))?;
    let text = resp.text().await.map_err(|e| format!("response body: {}", e))?;
    let re = regex::Regex::new(r"\b(\d{1,3}\.\d{1,3}\.\d{1,3}\.\d{1,3})\b").unwrap();
    Ok(re.find(&text).map(|m| m.as_str().to_string()))
}

/// Ping the tunnel URL with bypass header so loca.lt does not show the reminder.
pub async fn ping_tunnel_with_bypass(tunnel_url: &str) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let url = tunnel_url.trim_end_matches('/');
    let client = reqwest::Client::builder()
        .user_agent(TUNNEL_BYPASS_USER_AGENT)
        .build()
        .map_err(|e| format!("reqwest client: {}", e))?;
    let _ = client
        .get(url)
        .header(BYPASS_TUNNEL_REMINDER_HEADER, BYPASS_TUNNEL_REMINDER_VALUE)
        .send()
        .await
        .map_err(|e| format!("GET {}: {}", url, e))?;
    Ok(())
}
