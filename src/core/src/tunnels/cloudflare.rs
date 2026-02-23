//! Cloudflare tunnel provider (stub). To be implemented later.

/// Cloudflare backend. Implements TunnelBackend for unified dispatch.
pub struct CloudflareBackend;

#[async_trait::async_trait]
impl crate::tunnels::TunnelBackend for CloudflareBackend {
    fn name(&self) -> &'static str {
        "cloudflare"
    }

    async fn start_web_tunnel(
        &self,
        _config: &crate::config::Config,
    ) -> Result<(super::TunnelGuard, String), Box<dyn std::error::Error + Send + Sync>> {
        Err("cloudflare provider is not implemented yet".into())
    }
}
