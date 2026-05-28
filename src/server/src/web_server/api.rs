//! REST API handlers for the web server.
//!
//! Domain handlers live in small submodules. This facade keeps the route
//! declarations in `web_server::mod` stable while the implementation stays
//! grouped by feature area.

mod files;
mod previews;
mod profiles;
mod runtime;
mod sessions;
mod workspaces;

pub use files::{download_chat_file_handler, upload_chat_file_handler};
pub use previews::{delete_preview_handler, list_previews_handler};
pub use profiles::list_profiles_handler;
pub use runtime::{
    kill_agent_handler, kill_pty_handler, kill_tunnel_handler, list_agents_handler,
    list_agents_runtime_handler, list_channels_handler, list_tunnels_handler,
    reload_settings_handler, restart_channel_handler, start_channel_handler, stop_channel_handler,
    sync_channels_handler,
};
pub use sessions::{
    archive_launch_session_handler, create_session_handler, delete_session_handler,
    list_launch_sessions_batch_handler, list_launch_sessions_handler, list_sessions_handler,
    list_tmux_sessions_handler, unarchive_launch_session_delete_handler,
    unarchive_launch_session_handler,
};
pub use workspaces::{
    add_workspace_handler, create_workspace_handler, list_workspaces_handler,
    remove_workspace_handler,
};
