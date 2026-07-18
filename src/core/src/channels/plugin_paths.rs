//! Instance-scoped writable directories for channel plugins.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::config;

pub const PLUGIN_STATE_DIR_ENV: &str = "VIBEAROUND_PLUGIN_STATE_DIR";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginRuntimeDirs {
    pub cache: PathBuf,
    pub state: PathBuf,
}

pub fn plugin_runtime_dirs(instance_id: &str) -> PluginRuntimeDirs {
    let instance_dir = format!(
        "instance-{}",
        url::form_urlencoded::byte_serialize(instance_id.as_bytes()).collect::<String>()
    );
    PluginRuntimeDirs {
        cache: config::data_dir()
            .join(".cache")
            .join("channels")
            .join(&instance_dir),
        state: config::state_dir().join("channels").join(instance_dir),
    }
}

pub fn prepare_plugin_runtime_dirs(instance_id: &str) -> io::Result<PluginRuntimeDirs> {
    let dirs = plugin_runtime_dirs(instance_id);
    create_private_dir(&dirs.cache)?;
    create_private_dir(&dirs.state)?;
    Ok(dirs)
}

fn create_private_dir(path: &Path) -> io::Result<()> {
    fs::create_dir_all(path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn instance_encoding_cannot_escape_or_collide() {
        let escaped = plugin_runtime_dirs("../../shared");
        let literal = plugin_runtime_dirs("..%2F..%2Fshared");

        assert!(escaped
            .cache
            .starts_with(config::data_dir().join(".cache/channels")));
        assert!(escaped
            .state
            .starts_with(config::state_dir().join("channels")));
        assert_ne!(escaped.cache, literal.cache);
        assert_eq!(
            escaped.cache.file_name().and_then(|name| name.to_str()),
            Some("instance-..%2F..%2Fshared")
        );
    }
}
