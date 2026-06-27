use std::env;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context};

pub fn data_dir() -> anyhow::Result<PathBuf> {
    if let Some(path) = non_empty_env("VIBEAROUND_DATA_DIR") {
        return Ok(expand_home(&path));
    }
    let home = non_empty_env("HOME")
        .or_else(|| non_empty_env("USERPROFILE"))
        .context("HOME is not set; set VIBEAROUND_DATA_DIR or pass --profile-path")?;
    Ok(Path::new(&home).join(".vibearound"))
}

pub fn launch_profile_path(name: &str) -> anyhow::Result<PathBuf> {
    validate_launch_name(name, "launch profile")?;
    Ok(data_dir()?
        .join("launch")
        .join("profiles")
        .join(format!("{name}.json")))
}

pub fn settings_path() -> anyhow::Result<PathBuf> {
    Ok(data_dir()?.join("settings.json"))
}

pub fn validate_launch_name(name: &str, label: &str) -> anyhow::Result<()> {
    if name.trim().is_empty()
        || name.contains('/')
        || name.contains('\\')
        || name == "."
        || name == ".."
        || name.contains("..")
    {
        bail!("invalid {label} name '{}'", name);
    }
    Ok(())
}

fn expand_home(path: &str) -> PathBuf {
    if let Some(rest) = path.strip_prefix("~/") {
        if let Some(home) = non_empty_env("HOME").or_else(|| non_empty_env("USERPROFILE")) {
            return Path::new(&home).join(rest);
        }
    }
    PathBuf::from(path)
}

fn non_empty_env(key: &str) -> Option<String> {
    env::var(key).ok().filter(|value| !value.trim().is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_path_like_launch_names() {
        assert!(validate_launch_name("../secret", "launch profile").is_err());
        assert!(validate_launch_name("nested/name", "launch profile").is_err());
        assert!(validate_launch_name("codex", "launch profile").is_ok());
    }
}
