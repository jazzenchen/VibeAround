use std::path::PathBuf;

use clap::Args;

#[derive(Debug, Clone, PartialEq, Eq, Args)]
pub(crate) struct ServeArgs {
    #[arg(long)]
    pub(crate) port: Option<u16>,
    #[arg(long = "data-dir")]
    pub(crate) data_dir: Option<PathBuf>,
    #[arg(long = "web-dist")]
    pub(crate) web_dist: Option<PathBuf>,
    #[arg(long = "auth-mode")]
    pub(crate) auth_mode: Option<String>,
    #[arg(long = "server-bin")]
    pub(crate) server_bin: Option<PathBuf>,
}
