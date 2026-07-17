use serde_json::Value;

use crate::operation::Operation;
use crate::settings::{SettingsSnapshot, SettingsWriteResponse};
use crate::Result;

pub fn settings_get() -> Operation<Value> {
    Operation::new(crate::settings::get(), crate::settings::decode_get)
}

pub fn settings_snapshot() -> Operation<SettingsSnapshot> {
    Operation::new(crate::settings::get(), crate::settings::decode_snapshot)
}

pub fn settings_replace(
    settings: Value,
    revision: &str,
) -> Result<Operation<SettingsWriteResponse>> {
    Ok(Operation::new(
        crate::settings::replace(settings, revision)?,
        crate::settings::decode_write,
    ))
}

pub fn settings_patch(patch: Value) -> Result<Operation<SettingsWriteResponse>> {
    Ok(Operation::new(
        crate::settings::patch(patch)?,
        crate::settings::decode_write,
    ))
}
