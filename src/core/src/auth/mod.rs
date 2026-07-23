//! Authentication subsystem: token management and browser pairing.
//!
//! - [`token`] — per-daemon auth token (generation, storage, comparison)
//! - [`pair`]  — browser pairing via 6-digit code confirmed through IM

pub mod pair;
pub mod token;

// Re-export the most commonly used items so existing `use common::auth::*`
// call sites keep working without changes.
pub use token::{
    local_api_token_file_path, read_local_api_token_file, read_token_file, set_owner_only,
    token_file_path, write_local_api_token_file, write_token_file, AuthFile, AuthToken,
};
