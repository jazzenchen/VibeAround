//! Concrete tunnel provider backends. The process-based providers
//! (cloudflare / localtunnel / tailscale) build a [`super::TunnelPlan`]
//! that the parent module registers with the process supervisor; ngrok is
//! SDK-based and returns the task that keeps its session alive.

pub(super) mod cloudflare;
pub(super) mod localtunnel;
pub(super) mod ngrok;
pub(super) mod tailscale;
