//! Workspace utilities: directory checks and HTML detection.

use std::path::Path;

/// True if the directory exists and has no entries (or only . and ..).
pub fn is_dir_empty(path: &Path) -> bool {
    let Ok(entries) = std::fs::read_dir(path) else {
        return true;
    };
    entries.filter_map(|e| e.ok()).next().is_none()
}

/// True if the directory contains any file whose name ends with .html (case-insensitive).
pub fn dir_has_html(path: &Path) -> bool {
    let Ok(entries) = std::fs::read_dir(path) else {
        return false;
    };
    entries
        .filter_map(|e| e.ok())
        .any(|e| {
            e.path().file_name().and_then(|n| n.to_str()).map_or(false, |n| {
                n.to_lowercase().ends_with(".html")
            })
        })
}
