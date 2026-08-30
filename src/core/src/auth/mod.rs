//! Authentication subsystem: token management and browser pairing.
//!
//! - [`token`] — per-daemon auth token (generation, storage, comparison)
//! - [`pair`]  — browser pairing via 6-digit code confirmed through IM

pub mod pair;
pub mod token;

pub use token::{
    load_or_create_local_agent_api_token, read_local_agent_api_token_file,
    read_local_api_token_file, read_mcp_token_file, read_token_file, set_owner_only,
    token_file_path, write_auth_file, AuthFile, AuthToken, DaemonTokens, SharedAuthToken,
};
