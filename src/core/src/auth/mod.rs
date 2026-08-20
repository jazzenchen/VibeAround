//! Authentication subsystem: token management and browser pairing.
//!
//! - [`token`] — per-daemon auth token (generation, storage, comparison)
//! - [`pair`]  — browser pairing via 6-digit code confirmed through IM

pub mod pair;
pub mod token;

pub use token::{
    load_or_create_local_agent_api_token, local_agent_api_token_file_path,
    local_api_token_file_path, mcp_token_file_path, read_local_agent_api_token_file,
    read_local_api_token_file, read_mcp_token_file, read_token_file, set_owner_only,
    token_file_path, write_local_agent_api_token_file, write_local_api_token_file,
    write_mcp_token_file, write_token_file, AuthFile, AuthToken, SharedAuthToken,
};
