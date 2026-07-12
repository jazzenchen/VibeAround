use std::path::PathBuf;

use crate::config;
use crate::plugins::{DiscoveredPlugin, TopicConversationScope};
use crate::routing::{ActorId, ChannelInstanceId, ChannelKind};

#[derive(Debug, Clone)]
pub struct ChannelPluginManifest {
    pub channel_kind: ChannelKind,
    pub instance_id: ChannelInstanceId,
    pub actor_id: ActorId,
    pub version: String,
    pub runtime: String,
    pub plugin_dir: PathBuf,
    pub entry_path: PathBuf,
    pub raw_config: serde_json::Value,
    pub topic_scope: TopicConversationScope,
}

impl ChannelPluginManifest {
    pub fn from_discovered(
        channel_kind: impl Into<ChannelKind>,
        plugin: &DiscoveredPlugin,
    ) -> Option<Self> {
        let channel_kind = channel_kind.into();
        let raw_config = config::ensure_loaded().channel_raw_config(&channel_kind)?;
        // Legacy settings expose one configured process per channel kind.
        // Lifecycle/runtime code still receives an explicit instance ID so
        // multi-instance config can be added without another ownership rewrite.
        let instance_id = channel_kind.clone();
        let actor_id = instance_id.clone();
        Some(Self {
            channel_kind,
            instance_id,
            actor_id,
            version: plugin.installed_version(),
            runtime: plugin.manifest.runtime.clone(),
            plugin_dir: plugin.dir.clone(),
            entry_path: plugin.entry_path(),
            raw_config,
            topic_scope: plugin.manifest.capabilities.topic_scope,
        })
    }
}
