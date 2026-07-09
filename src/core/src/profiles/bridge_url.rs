//! Shared local API bridge URL and route shapes.

pub const LOCAL_API_PUBLIC_PREFIX: &str = "/va/local-api";
pub const LOCAL_API_PUBLIC_PATH_PREFIX: &str = "/va/local-api/";
pub const LOCAL_API_ROUTE_PATH_PREFIX: &str = "/local-api/";
pub const LOCAL_API_ROUTE_PREFIX: &str = "/local-api/{profile_id}/{scope}/{target_api_type}";
pub const LOCAL_API_RESPONSES_ROUTE: &str =
    "/local-api/{profile_id}/{scope}/{target_api_type}/v1/responses";
pub const LOCAL_API_CHAT_COMPLETIONS_ROUTE: &str =
    "/local-api/{profile_id}/{scope}/{target_api_type}/v1/chat/completions";
pub const LOCAL_API_MESSAGES_ROUTE: &str =
    "/local-api/{profile_id}/{scope}/{target_api_type}/v1/messages";
pub const LOCAL_API_MODELS_ROUTE: &str =
    "/local-api/{profile_id}/{scope}/{target_api_type}/v1/models";
pub const LOCAL_API_GEMINI_MODELS_ACTION_ROUTE: &str =
    "/local-api/{profile_id}/{scope}/{target_api_type}/{version}/models/{model_action}";

pub fn local_api_base(port: u16, profile_id: &str, scope: &str, target_api_type: &str) -> String {
    local_api_base_with_suffix(port, profile_id, scope, target_api_type, "")
}

pub fn local_api_v1_base(
    port: u16,
    profile_id: &str,
    scope: &str,
    target_api_type: &str,
) -> String {
    local_api_base_with_suffix(port, profile_id, scope, target_api_type, "/v1")
}

pub fn local_api_base_with_suffix(
    port: u16,
    profile_id: &str,
    scope: &str,
    target_api_type: &str,
    suffix: &str,
) -> String {
    format!(
        "http://127.0.0.1:{port}{LOCAL_API_PUBLIC_PREFIX}/{profile_id}/{scope}/{target_api_type}{suffix}"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_api_base_uses_dashboard_mount() {
        assert_eq!(
            local_api_base(12358, "dashscope", "codex-openai-responses", "openai-chat"),
            "http://127.0.0.1:12358/va/local-api/dashscope/codex-openai-responses/openai-chat"
        );
        assert_eq!(
            local_api_v1_base(12358, "gemini", "codex-openai-responses", "gemini"),
            "http://127.0.0.1:12358/va/local-api/gemini/codex-openai-responses/gemini/v1"
        );
    }

    #[test]
    fn local_api_route_constants_stay_under_local_api_prefix() {
        for route in [
            LOCAL_API_RESPONSES_ROUTE,
            LOCAL_API_CHAT_COMPLETIONS_ROUTE,
            LOCAL_API_MESSAGES_ROUTE,
            LOCAL_API_MODELS_ROUTE,
            LOCAL_API_GEMINI_MODELS_ACTION_ROUTE,
        ] {
            assert!(route.starts_with(LOCAL_API_ROUTE_PREFIX));
        }
    }
}
