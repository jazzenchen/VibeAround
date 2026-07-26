//! Ngrok: expose the web dashboard via the ngrok Rust SDK.
//! Token from global config; forwards to localhost:<DEFAULT_PORT>.

use ngrok::config::ForwarderBuilder;
use ngrok::tunnel::EndpointInfo;
use url::Url;

use crate::proc_log;
use crate::process::registry::ProcessKind;

const PORT: u16 = crate::config::DEFAULT_PORT;

/// Start ngrok tunnel using the Rust SDK. Returns (guard, public URL).
/// Uses the given config for auth token and optional static domain.
pub async fn start_web_tunnel(
    config: &crate::config::Config,
) -> Result<(crate::tunnels::TunnelGuard, String), Box<dyn std::error::Error + Send + Sync>> {
    let token = config.ngrok_auth_token.as_deref().ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "ngrok token not set: set tunnel.ngrok.auth_token in settings.json",
        )
    })?;
    let session = ngrok::Session::builder()
        .authtoken(token)
        .connect()
        .await
        .map_err(|e| format!("ngrok session connect: {}", e))?;

    let forward_url = Url::parse(&format!("http://localhost:{}", PORT))
        .map_err(|e| format!("forward URL: {}", e))?;
    let forwarder = match config
        .ngrok_domain
        .as_deref()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
    {
        Some(domain) => {
            proc_log!(
                info,
                kind = ProcessKind::Tunnel,
                label = "ngrok",
                event = "static_domain",
                domain = %domain
            );
            let f = session
                .http_endpoint()
                .domain(domain)
                .listen_and_forward(forward_url.clone())
                .await
                .map_err(|e| format!("ngrok domain {:?} failed: {} (use your reserved/static domain from ngrok dashboard)", domain, e))?;
            f
        }
        None => session
            .http_endpoint()
            .listen_and_forward(forward_url)
            .await
            .map_err(|e| format!("ngrok listen_and_forward: {}", e))?,
    };

    let url = forwarder.url().to_string();
    proc_log!(
        info,
        kind = ProcessKind::Tunnel,
        label = "ngrok",
        event = "started",
        url = %url
    );

    // Keep both Session and forwarder alive; dropping Session closes the ngrok connection and makes the endpoint go offline (ERR_NGROK_3200).
    let handle = tokio::spawn(async move {
        let _session = session;
        let _forwarder = forwarder;
        std::future::pending::<()>().await
    });

    Ok((crate::tunnels::TunnelGuard::Sdk(handle), url))
}

/// Ngrok backend. Implements TunnelBackend for unified dispatch.
pub struct NgrokBackend;

#[async_trait::async_trait]
impl crate::tunnels::TunnelBackend for NgrokBackend {
    fn name(&self) -> &'static str {
        "ngrok"
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
