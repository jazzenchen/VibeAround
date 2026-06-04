//! Channel plugins — IM integrations (Telegram, Feishu, web, etc.).
//!
//! Filters the generic [`discover_plugins`] output by
//! `kind == "channel"` and provides channel-scoped lookup helpers.
//! The underlying manifest schema lives in the parent module.
//!
//! [`discover_plugins`]: super::discover_plugins

use std::collections::HashMap;

use super::{DiscoveredPlugin, DiscoveredPluginSummary};

const CHANNEL_PLUGIN_KIND: &str = "channel";

/// All channel-kind plugins keyed by id.
pub fn discover() -> HashMap<String, DiscoveredPlugin> {
    super::discover_plugins()
        .into_iter()
        .filter(|(_, plugin)| plugin.manifest.kind == CHANNEL_PLUGIN_KIND)
        .collect()
}

/// All user-installed channel plugins keyed by id.
pub fn discover_user() -> HashMap<String, DiscoveredPlugin> {
    super::discover_user_plugins()
        .into_iter()
        .filter(|(_, plugin)| plugin.manifest.kind == CHANNEL_PLUGIN_KIND)
        .collect()
}

/// Sorted summary list for UI display.
pub fn list_summaries() -> Vec<DiscoveredPluginSummary> {
    let mut plugins = discover()
        .values()
        .map(DiscoveredPluginSummary::from)
        .collect::<Vec<_>>();
    plugins.sort_by(|left, right| left.id.cmp(&right.id));
    plugins
}

/// Look up a single channel plugin by id.
pub fn find(plugin_id: &str) -> Option<DiscoveredPlugin> {
    discover().remove(plugin_id)
}

/// Look up a user-installed channel plugin by id.
pub fn find_user(plugin_id: &str) -> Option<DiscoveredPlugin> {
    discover_user().remove(plugin_id)
}
