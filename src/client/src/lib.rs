//! Pure Rust client protocol helpers for VibeAround server consumers.
//!
//! This crate owns request construction, response decoding, and Rust-side
//! wire models for the `@va/server` HTTP contract. It deliberately does not
//! own network I/O, process spawning, native commands, Tauri IPC, or UI state.

pub mod error;
pub mod events;
pub mod http;
pub mod launcher;
pub mod profiles;
pub mod runtime;
pub mod service;
pub mod sessions;
pub mod settings;
pub mod workspaces;

pub use error::{ClientError, Result};
pub use http::{AuthRequirement, HttpMethod, RequestSpec, ResponseSpec};
