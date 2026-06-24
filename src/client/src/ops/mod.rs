//! Ready-to-send operation catalog.
//!
//! These helpers pair each request builder with its matching decoder. They
//! still do not send anything; the host owns transport.

mod auth;
mod launcher;
mod previews;
mod profiles;
mod runtime;
mod service;
mod sessions;
mod settings;
mod workspaces;

pub use auth::*;
pub use launcher::*;
pub use previews::*;
pub use profiles::*;
pub use runtime::*;
pub use service::*;
pub use sessions::*;
pub use settings::*;
pub use workspaces::*;

use crate::{ResponseSpec, Result};

pub fn decode_success(response: ResponseSpec) -> Result<()> {
    response.ensure_success()
}
