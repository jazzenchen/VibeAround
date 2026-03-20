//! IM channel spec: unified kind enum for channel identification.
//! All channels now run as external plugins; no in-process transport construction.

/// Channel kind identifier. Used to register and identify IM bots.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ImChannelKind {
    Feishu,
    Telegram,
}

impl ImChannelKind {
    /// Unique string id for config and logging.
    pub fn kind_id(&self) -> &'static str {
        match self {
            ImChannelKind::Feishu => "feishu",
            ImChannelKind::Telegram => "telegram",
        }
    }

    /// Parse from string (e.g. from settings.json channel name).
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "feishu" | "lark" => Some(ImChannelKind::Feishu),
            "telegram" => Some(ImChannelKind::Telegram),
            _ => None,
        }
    }

    /// All known channel kinds.
    pub fn all() -> &'static [ImChannelKind] {
        &[ImChannelKind::Feishu, ImChannelKind::Telegram]
    }
}
