//! Ngrok: expose the web dashboard via the ngrok Rust SDK.
//! Token from global config; forwards to localhost:<DEFAULT_PORT>.

use ngrok::config::ForwarderBuilder;
use ngrok::tunnel::EndpointInfo;
use url::Url;

use crate::proc_log;
use crate::process::ProcessKind;

const PORT: u16 = crate::config::DEFAULT_PORT;

/// Start the ngrok tunnel using the Rust SDK. Returns the task that keeps
/// the session alive (abort it to stop the tunnel) and the public URL.
pub(crate) async fn start(
    config: &crate::config::Config,
) -> Result<(tokio::task::JoinHandle<()>, String), Box<dyn std::error::Error + Send + Sync>> {
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

    Ok((handle, url))
}
