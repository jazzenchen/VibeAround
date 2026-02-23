//! Cloudflare tunnel provider (stub). To be implemented later.

type TunnelResult = Result<(super::TunnelGuard, String), Box<dyn std::error::Error + Send + Sync>>;

pub async fn start_web_tunnel() -> TunnelResult {
    Err("cloudflare provider is not implemented yet".into())
}
