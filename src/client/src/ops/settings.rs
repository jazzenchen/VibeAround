use serde_json::Value;

use crate::operation::Operation;
use crate::settings::SettingsWriteResponse;
use crate::Result;

pub fn settings_get() -> Operation<Value> {
    Operation::new(crate::settings::get(), crate::settings::decode_get)
}

pub fn settings_put(settings: Value) -> Result<Operation<SettingsWriteResponse>> {
    Ok(Operation::new(
        crate::settings::put(settings)?,
        crate::settings::decode_put,
    ))
}
