pub fn join_protocol_endpoint(base_url: &str, endpoint: &str, append_v1_path: bool) -> String {
    let base_url = base_url.trim_end_matches('/');
    let endpoint = endpoint.trim_start_matches('/');

    if !append_v1_path || base_url_ends_with_api_version(base_url) {
        format!("{base_url}/{endpoint}")
    } else {
        format!("{base_url}/v1/{endpoint}")
    }
}

fn base_url_ends_with_api_version(base_url: &str) -> bool {
    let last = base_url
        .trim_end_matches('/')
        .rsplit('/')
        .next()
        .unwrap_or_default();

    let Some(rest) = last.strip_prefix('v') else {
        return false;
    };

    let digit_count = rest
        .chars()
        .take_while(|c| c.is_ascii_digit())
        .count();

    if digit_count == 0 {
        return false;
    }

    let suffix = &rest[digit_count..];
    suffix.is_empty() || matches!(suffix, "alpha" | "beta")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_v1_for_host_root() {
        assert_eq!(
            join_protocol_endpoint("https://api.example.com", "chat/completions", true),
            "https://api.example.com/v1/chat/completions"
        );
    }

    #[test]
    fn does_not_double_v1() {
        assert_eq!(
            join_protocol_endpoint("https://api.example.com/v1", "chat/completions", true),
            "https://api.example.com/v1/chat/completions"
        );
    }

    #[test]
    fn versioned_base_v2() {
        assert_eq!(
            join_protocol_endpoint("https://api.example.com/v2", "chat/completions", true),
            "https://api.example.com/v2/chat/completions"
        );
    }

    #[test]
    fn versioned_base_v4_zai_global() {
        assert_eq!(
            join_protocol_endpoint("https://api.z.ai/api/paas/v4", "chat/completions", true),
            "https://api.z.ai/api/paas/v4/chat/completions"
        );
    }

    #[test]
    fn versioned_base_v4_trailing_slash() {
        assert_eq!(
            join_protocol_endpoint("https://open.bigmodel.cn/api/paas/v4/", "chat/completions", true),
            "https://open.bigmodel.cn/api/paas/v4/chat/completions"
        );
    }

    #[test]
    fn versioned_base_v1beta() {
        assert_eq!(
            join_protocol_endpoint("https://api.example.com/v1beta", "responses", true),
            "https://api.example.com/v1beta/responses"
        );
    }

    #[test]
    fn versioned_base_v1alpha() {
        assert_eq!(
            join_protocol_endpoint("https://api.example.com/v1alpha", "responses", true),
            "https://api.example.com/v1alpha/responses"
        );
    }

    #[test]
    fn append_v1_path_false() {
        assert_eq!(
            join_protocol_endpoint("https://api.example.com/custom", "chat/completions", false),
            "https://api.example.com/custom/chat/completions"
        );
    }

    #[test]
    fn non_version_v_prefix_still_appends_v1() {
        assert_eq!(
            join_protocol_endpoint("https://api.example.com/v1foo", "chat/completions", true),
            "https://api.example.com/v1foo/v1/chat/completions"
        );
    }

    #[test]
    fn base_url_ends_with_api_version_v3() {
        assert!(base_url_ends_with_api_version("https://example.com/api/v3"));
    }

    #[test]
    fn base_url_ends_with_api_version_v12() {
        assert!(base_url_ends_with_api_version("https://example.com/api/v12"));
    }

    #[test]
    fn base_url_not_versioned_plain() {
        assert!(!base_url_ends_with_api_version("https://example.com/api"));
    }

    #[test]
    fn base_url_not_versioned_v_without_digits() {
        assert!(!base_url_ends_with_api_version("https://example.com/api/vfoo"));
    }
}
