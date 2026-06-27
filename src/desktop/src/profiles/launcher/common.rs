use std::path::PathBuf;

#[derive(Debug, Clone)]
pub(super) struct LaunchPlan {
    pub env: Vec<(String, String)>,
    pub command: String,
    pub args: Vec<String>,
    pub cleanup_paths: Vec<PathBuf>,
    pub window_label: String,
    pub workspace: PathBuf,
    pub macos_app_probe: Option<String>,
    #[cfg_attr(not(target_os = "windows"), allow(dead_code))]
    pub windows_process_probe: Option<String>,
    #[cfg_attr(not(target_os = "windows"), allow(dead_code))]
    pub windows_executable_path: Option<PathBuf>,
}
