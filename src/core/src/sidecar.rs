//! Locating binaries that ship next to VibeAround itself (va-launch,
//! va-tui). One lookup serves the desktop app (Tauri external-bin layout),
//! the npm CLI (platform binaries side by side), and development checkouts
//! (`target/{debug,release}`).

use std::path::PathBuf;

use anyhow::bail;

/// Find a bundled binary: an explicit env override, then the directories next
/// to the running executable, then the dev target dirs. Errors when the
/// override points at nothing or no candidate exists.
pub fn find(binary: &str, env_override: &str) -> anyhow::Result<PathBuf> {
    if let Some(path) = std::env::var_os(env_override) {
        let path = PathBuf::from(path);
        if path.is_file() {
            return Ok(path);
        }
        bail!("{env_override} is not a file: {}", path.display());
    }

    let candidates = candidate_paths(binary);
    if let Some(path) = candidates.iter().find(|path| path.is_file()) {
        return Ok(path.clone());
    }

    let searched = candidates
        .iter()
        .map(|path| path.display().to_string())
        .collect::<Vec<_>>()
        .join(", ");
    bail!("{binary} binary not found; searched: {searched}; build it or set {env_override}")
}

/// The command to run for a bundled binary: its resolved path, or the bare
/// name for a PATH lookup when it is not bundled (npm / dev setups).
pub fn command(binary: &str, env_override: &str) -> String {
    find(binary, env_override)
        .map(|path| path.to_string_lossy().into_owned())
        .unwrap_or_else(|_| binary.to_string())
}

fn candidate_paths(binary: &str) -> Vec<PathBuf> {
    let names = binary_names(binary);
    let mut roots = Vec::new();

    if let Ok(current_exe) = std::env::current_exe() {
        if let Some(exe_dir) = current_exe.parent() {
            push_unique(&mut roots, exe_dir.to_path_buf());
            push_unique(&mut roots, exe_dir.join("resources"));
            push_unique(&mut roots, exe_dir.join("_up_").join("resources"));
            push_unique(&mut roots, exe_dir.join("..").join("Resources"));
            push_unique(
                &mut roots,
                exe_dir
                    .join("..")
                    .join("Resources")
                    .join("_up_")
                    .join("resources"),
            );
        }
    }

    // Development checkouts: the workspace target dir and the desktop's
    // prepared sidecar dir.
    let workspace = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..");
    for profile in ["debug", "release"] {
        push_unique(&mut roots, workspace.join("target").join(profile));
    }
    push_unique(&mut roots, workspace.join("desktop").join("binaries"));

    roots
        .into_iter()
        .flat_map(|root| names.iter().map(move |name| root.join(name)))
        .collect()
}

fn push_unique(paths: &mut Vec<PathBuf>, path: PathBuf) {
    if !paths.iter().any(|existing| existing == &path) {
        paths.push(path);
    }
}

/// Plain name first, then the Tauri external-bin name with the target triple.
fn binary_names(binary: &str) -> Vec<String> {
    let plain = format!("{binary}{}", executable_extension());
    let mut names = vec![plain.clone()];
    if let Some(triple) = current_target_triple() {
        let sidecar = format!("{binary}-{triple}{}", executable_extension());
        if sidecar != plain {
            names.push(sidecar);
        }
    }
    names
}

pub fn current_target_triple() -> Option<&'static str> {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("windows", "x86_64") => Some("x86_64-pc-windows-msvc"),
        ("windows", "aarch64") => Some("aarch64-pc-windows-msvc"),
        ("macos", "x86_64") => Some("x86_64-apple-darwin"),
        ("macos", "aarch64") => Some("aarch64-apple-darwin"),
        ("linux", "x86_64") => Some("x86_64-unknown-linux-gnu"),
        ("linux", "aarch64") => Some("aarch64-unknown-linux-gnu"),
        _ => None,
    }
}

fn executable_extension() -> &'static str {
    if cfg!(target_os = "windows") {
        ".exe"
    } else {
        ""
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn env_override_must_point_at_a_file() {
        let var = "VIBEAROUND_SIDECAR_TEST_MISSING";
        std::env::set_var(var, "/definitely/not/here");
        let error = find("va-nothing", var).unwrap_err().to_string();
        std::env::remove_var(var);
        assert!(error.contains("is not a file"));
    }

    #[test]
    fn unbundled_binaries_fall_back_to_a_path_lookup() {
        assert_eq!(
            command("va-nothing", "VIBEAROUND_SIDECAR_TEST_UNSET"),
            "va-nothing"
        );
    }

    #[test]
    fn sidecar_name_follows_the_tauri_external_bin_layout() {
        let Some(triple) = current_target_triple() else {
            return;
        };
        let names = binary_names("va-tui");
        assert_eq!(names[0], format!("va-tui{}", executable_extension()));
        assert_eq!(
            names[1],
            format!("va-tui-{triple}{}", executable_extension())
        );
    }
}
