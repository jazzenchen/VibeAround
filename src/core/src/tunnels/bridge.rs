//! `ProcessBridge` adapter for process-based tunnel providers.
//!
//! The bridge's only protocol job is URL discovery: watch the child's
//! stdout until the provider prints its public URL (or an interactive
//! approval link), push what it finds into the [`TunnelManager`], and keep
//! draining output until the pipe closes. Exit handling is deliberately NOT
//! here — the launch path subscribes to supervisor events and marks the
//! tunnel failed on `Stopped`, which covers spawn failures (where the bridge
//! never runs) with the same mechanism as crashes.

use std::sync::Arc;

use tokio::io::AsyncBufReadExt;

use crate::proc_log;
use crate::process::bridge::{BridgeExit, BridgeFuture, CancelSignal, ProcessBridge, StdioPipes};
use crate::process::ProcessKind;

use super::manager::TunnelManager;

/// How the provider's public URL becomes known.
pub(crate) enum UrlDiscovery {
    /// Known before spawn (Cloudflare named tunnels take it from config).
    Known(String),
    /// Parse stdout lines until `parse_url` yields the public URL.
    FromStdout {
        parse_url: fn(&str) -> Option<String>,
        /// Optional second parser for an interactive-approval link
        /// (Tailscale Funnel's one-time enable step).
        parse_approval: Option<fn(&str) -> Option<String>>,
    },
}

impl std::fmt::Debug for UrlDiscovery {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            UrlDiscovery::Known(url) => f.debug_tuple("Known").field(url).finish(),
            UrlDiscovery::FromStdout { .. } => f.write_str("FromStdout"),
        }
    }
}

pub(super) struct TunnelBridge {
    pub(super) provider_key: &'static str,
    pub(super) manager: Arc<TunnelManager>,
    pub(super) url: UrlDiscovery,
}

impl ProcessBridge for TunnelBridge {
    fn run(self: Box<Self>, pipes: StdioPipes, mut cancel: CancelSignal) -> BridgeFuture {
        Box::pin(async move {
            let TunnelBridge {
                provider_key,
                manager,
                url,
            } = *self;
            // The providers never read stdin; keep it open anyway so a
            // stdin-sensitive binary doesn't see EOF mid-run.
            let _stdin = pipes.stdin;

            let mut parsers = match url {
                UrlDiscovery::Known(url) => {
                    proc_log!(
                        info,
                        kind = ProcessKind::Tunnel,
                        label = provider_key,
                        event = "url",
                        url = %url
                    );
                    manager.set_url(provider_key, &url);
                    None
                }
                UrlDiscovery::FromStdout {
                    parse_url,
                    parse_approval,
                } => Some((parse_url, parse_approval)),
            };

            let mut lines = tokio::io::BufReader::new(pipes.stdout).lines();
            loop {
                let line = tokio::select! {
                    changed = cancel.changed() => {
                        if changed.is_err() || *cancel.borrow() {
                            return BridgeExit::Cancelled;
                        }
                        continue;
                    }
                    line = lines.next_line() => line,
                };
                match line {
                    Ok(Some(line)) => {
                        if let Some((parse_url, parse_approval)) = parsers {
                            if let Some(url) = parse_url(&line) {
                                proc_log!(
                                    info,
                                    kind = ProcessKind::Tunnel,
                                    label = provider_key,
                                    event = "url",
                                    url = %url
                                );
                                manager.set_url(provider_key, &url);
                                parsers = None;
                                continue;
                            }
                            if let Some(approval_url) =
                                parse_approval.and_then(|parse| parse(&line))
                            {
                                proc_log!(
                                    info,
                                    kind = ProcessKind::Tunnel,
                                    label = provider_key,
                                    event = "awaiting_approval",
                                    url = %approval_url
                                );
                                manager.set_awaiting_approval(provider_key, approval_url);
                                continue;
                            }
                        }
                        if !line.trim().is_empty() {
                            proc_log!(
                                info,
                                kind = ProcessKind::Tunnel,
                                label = provider_key,
                                event = "stdout",
                                line = %line
                            );
                        }
                    }
                    Ok(None) | Err(_) => return BridgeExit::Clean,
                }
            }
        })
    }
}
