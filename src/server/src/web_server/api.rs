//! REST API handlers for the web server.
//!
//! Domain handlers live in small submodules. This facade keeps the route
//! declarations in `web_server::mod` stable while the implementation stays
//! grouped by feature area.

mod files;
mod launcher;
mod previews;
mod profiles;
mod runtime;
mod service;
mod sessions;
mod settings;
mod workspaces;

pub use files::{download_chat_file_handler, upload_chat_file_handler};
pub use launcher::{
    get_launcher_preferences_handler, launcher_plan_handler, set_agent_launch_args_handler,
    set_agent_profile_handler, set_agent_workspace_handler, set_default_launch_handler,
    set_local_agent_api_handler, set_profile_connection_handler, set_selected_agent_handler,
};
pub use previews::{delete_preview_handler, list_previews_handler};
pub use profiles::{
    create_model_profile_handler, delete_model_profile_handler, get_model_profile_handler,
    list_model_profiles_handler, list_profiles_handler, reorder_model_profiles_handler,
    update_model_profile_handler,
};
pub use runtime::{
    kill_agent_handler, kill_pty_handler, kill_tunnel_handler, list_agents_handler,
    list_agents_runtime_handler, list_channels_handler, list_tunnels_handler,
    reload_settings_handler, restart_channel_handler, start_channel_handler, stop_channel_handler,
    sync_channels_handler,
};
pub use service::{health_handler, info_handler};
pub use sessions::{
    archive_launch_session_handler, create_session_handler, delete_session_handler,
    init_workspace_thread_handler, list_launch_sessions_batch_handler,
    list_launch_sessions_handler, list_sessions_handler, list_tmux_sessions_handler,
    unarchive_launch_session_delete_handler, unarchive_launch_session_handler,
};
pub use settings::{get_settings_handler, patch_settings_handler, put_settings_handler};
pub use workspaces::{
    add_workspace_handler, create_workspace_handler, list_workspaces_handler,
    remove_workspace_handler, reorder_workspaces_handler, set_default_workspace_handler,
};
