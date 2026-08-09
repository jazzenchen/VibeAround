use serde_json::Value;

pub(super) fn codex_session_id_from_mcp_metadata(metadata: Option<&Value>) -> Option<String> {
    let metadata = metadata?;
    codex_turn_metadata_value(metadata)
        .as_ref()
        .and_then(|turn| string_field(turn, "thread_id"))
        .or_else(|| string_field(metadata, "threadId"))
        .or_else(|| string_field(metadata, "thread_id"))
        .or_else(|| {
            codex_turn_metadata_value(metadata)
                .as_ref()
                .and_then(|turn| string_field(turn, "session_id"))
        })
}

fn codex_turn_metadata_value(metadata: &Value) -> Option<Value> {
    let value = metadata.get("x-codex-turn-metadata")?;
    if value.is_object() {
        return Some(value.clone());
    }
    let text = value.as_str()?;
    serde_json::from_str(text).ok()
}

pub(super) fn string_field(value: &Value, field: &str) -> Option<String> {
    value
        .get(field)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
}

pub(super) fn argument_string(arguments: &Value, field: &str) -> Option<String> {
    string_field(arguments, field)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::codex_session_id_from_mcp_metadata;

    #[test]
    fn codex_metadata_prefers_turn_thread_id() {
        let metadata = json!({
            "threadId": "top-level-thread",
            "x-codex-turn-metadata": {
                "session_id": "turn-session",
                "thread_id": "turn-thread",
                "turn_id": "turn"
            }
        });

        assert_eq!(
            codex_session_id_from_mcp_metadata(Some(&metadata)).as_deref(),
            Some("turn-thread")
        );
    }

    #[test]
    fn codex_metadata_accepts_json_encoded_turn_metadata() {
        let metadata = json!({
            "x-codex-turn-metadata": "{\"thread_id\":\"encoded-thread\"}"
        });

        assert_eq!(
            codex_session_id_from_mcp_metadata(Some(&metadata)).as_deref(),
            Some("encoded-thread")
        );
    }
}
