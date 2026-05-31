//! Provider catalog — third-party endpoint metadata baked into the binary.
//!
//! Built-in providers have JSON under `src/resources/profile-catalog/`.
//! `profile-catalog/manifest.json` controls which provider files are loaded
//! and in what order.
//!
//! The catalog is still embedded in the desktop binary, but the Rust side no
//! longer hard-codes provider ids. The JSON manifest is the source of truth for
//! built-in profile catalog membership; adding fields requires only a serde
//! `#[serde(default)]` to stay forward-compatible.

use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::sync::LazyLock;

use include_dir::{include_dir, Dir};
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Embedded JSON sources
// ---------------------------------------------------------------------------
static PROFILE_CATALOG_DIR: Dir<'_> =
    include_dir!("$CARGO_MANIFEST_DIR/../resources/profile-catalog");
static PROFILE_CATALOG_MANIFEST_JSON: &str =
    include_str!("../../../resources/profile-catalog/manifest.json");

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ProviderCatalog {
    pub id: String,
    pub label: String,
    #[serde(default)]
    pub icon: Option<String>,
    #[serde(default)]
    pub homepage: Option<String>,
    /// Hide legacy/alias providers from the "new profile" picker while
    /// keeping them loadable for existing saved profiles.
    #[serde(default, skip_serializing_if = "is_false")]
    pub hidden_from_picker: bool,
    pub endpoints: Vec<EndpointDef>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct EndpointDef {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    pub api_type: String,
    pub default_base_url: String,
    /// Most OpenAI-compatible providers expose an API root at either the host
    /// root or `/v1`; when the root is provider-specific (for example
    /// `/v1beta/openai`) the catalog can opt out of appending `/v1`.
    #[serde(default = "default_true", skip_serializing_if = "is_true")]
    pub append_v1_path: bool,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub headers: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub auth_header: bool,
    #[serde(default)]
    pub models: Vec<ModelDef>,
    #[serde(default)]
    pub capabilities: EndpointCapabilities,
    pub auth_modes: Vec<AuthModeDef>,
    /// Optional caveat shown to users next to the launch button — e.g.
    /// "codex 0.X+ requires Responses API and this provider only serves
    /// chat-completions". `None` for endpoints with no known caveat.
    #[serde(default)]
    pub compatibility_warning: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct EndpointCapabilities {
    #[serde(default)]
    pub reasoning_effort: bool,
    #[serde(default, skip_serializing_if = "ContentCapabilities::is_empty")]
    pub content: ContentCapabilities,
}

fn is_false(value: &bool) -> bool {
    !*value
}

fn is_true(value: &bool) -> bool {
    *value
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq, Eq)]
pub struct ContentCapabilities {
    #[serde(default, skip_serializing_if = "is_false")]
    pub image_input: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub file_input: bool,
}

impl ContentCapabilities {
    pub fn is_empty(&self) -> bool {
        !self.image_input && !self.file_input
    }

    pub fn merge(&self, override_caps: &ContentCapabilities) -> ContentCapabilities {
        ContentCapabilities {
            image_input: self.image_input || override_caps.image_input,
            file_input: self.file_input || override_caps.file_input,
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ModelDef {
    pub id: String,
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub aliases: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_window: Option<u64>,
    #[serde(default, skip_serializing_if = "ContentCapabilities::is_empty")]
    pub capabilities: ContentCapabilities,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AuthModeDef {
    pub mode: String,
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default)]
    pub fields: Vec<FieldDef>,
    #[serde(default)]
    pub render: Option<RenderRules>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct FieldDef {
    pub name: String,
    pub label: String,
    #[serde(default)]
    pub secret: bool,
    #[serde(default)]
    pub required: bool,
    #[serde(default)]
    pub placeholder: Option<String>,
    #[serde(default)]
    pub validate: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct RenderRules {
    #[serde(default)]
    pub env: std::collections::BTreeMap<String, String>,
    /// Files to materialize alongside the env exports for CLIs that support
    /// an explicit config path. Codex profile launches intentionally avoid
    /// CODEX_HOME and translate these settings into `-c` CLI overrides so the
    /// user's own Codex home, skills, and plugins remain active.
    #[serde(default)]
    pub settings_files: Vec<SettingsFileTemplate>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SettingsFileTemplate {
    pub rel_path: String,
    pub template: String,
}

#[derive(Debug, Deserialize)]
struct ProfileCatalogManifest {
    providers: Vec<ProfileCatalogManifestEntry>,
}

#[derive(Debug, Deserialize)]
struct ProfileCatalogManifestEntry {
    id: String,
    file: String,
}

// ---------------------------------------------------------------------------
// Static loader
// ---------------------------------------------------------------------------

static CATALOG: LazyLock<Vec<ProviderCatalog>> = LazyLock::new(load_builtin_catalogs);

fn load_builtin_catalogs() -> Vec<ProviderCatalog> {
    let manifest = serde_json::from_str::<ProfileCatalogManifest>(PROFILE_CATALOG_MANIFEST_JSON)
        .unwrap_or_else(|e| panic!("profile-catalog: failed to parse manifest.json: {e}"));
    let mut seen = BTreeSet::new();
    let mut catalogs = Vec::with_capacity(manifest.providers.len());

    for entry in manifest.providers {
        validate_manifest_entry(&entry);
        if !seen.insert(entry.id.clone()) {
            panic!("profile-catalog: duplicate provider id '{}'", entry.id);
        }
        let file = PROFILE_CATALOG_DIR
            .get_file(entry.file.as_str())
            .unwrap_or_else(|| {
                panic!(
                    "profile-catalog: manifest entry '{}' references missing file '{}'",
                    entry.id, entry.file
                )
            });
        let body = file.contents_utf8().unwrap_or_else(|| {
            panic!(
                "profile-catalog: manifest entry '{}' file '{}' is not valid UTF-8",
                entry.id, entry.file
            )
        });
        let catalog = serde_json::from_str::<ProviderCatalog>(body).unwrap_or_else(|e| {
            panic!(
                "profile-catalog: failed to parse {} for provider '{}': {}",
                entry.file, entry.id, e
            )
        });
        if catalog.id != entry.id {
            panic!(
                "profile-catalog: manifest id '{}' does not match file '{}' id '{}'",
                entry.id, entry.file, catalog.id
            );
        }
        catalogs.push(catalog);
    }

    catalogs
}

fn validate_manifest_entry(entry: &ProfileCatalogManifestEntry) {
    if entry.id.trim().is_empty() {
        panic!("profile-catalog: manifest contains an empty provider id");
    }
    if entry.file.trim().is_empty()
        || entry.file.contains('/')
        || entry.file.contains('\\')
        || entry.file.contains("..")
        || !entry.file.ends_with(".json")
    {
        panic!(
            "profile-catalog: manifest entry '{}' has invalid file '{}'",
            entry.id, entry.file
        );
    }
}

pub fn all() -> &'static [ProviderCatalog] {
    &CATALOG
}

pub fn get(id: &str) -> Option<&'static ProviderCatalog> {
    if id == "custom" {
        return Some(custom());
    }
    CATALOG.iter().find(|c| c.id == id)
}

pub fn endpoint_id(endpoint: &EndpointDef) -> &str {
    endpoint.id.as_deref().unwrap_or(endpoint.api_type.as_str())
}

pub fn find_endpoint<'a>(
    provider: &'a ProviderCatalog,
    api_type: &str,
    selected_endpoint_id: Option<&str>,
) -> Option<&'a EndpointDef> {
    provider
        .endpoints
        .iter()
        .find(|endpoint| {
            endpoint.api_type == api_type
                && selected_endpoint_id
                    .map(|id| endpoint_id(endpoint) == id)
                    .unwrap_or(true)
        })
        .or_else(|| {
            if selected_endpoint_id.is_some() {
                return None;
            }
            provider
                .endpoints
                .iter()
                .find(|endpoint| endpoint.api_type == api_type)
        })
}

pub fn find_model<'a>(endpoint: &'a EndpointDef, model_id: &str) -> Option<&'a ModelDef> {
    let model_id = model_id.trim();
    if model_id.is_empty() {
        return None;
    }
    endpoint
        .models
        .iter()
        .find(|model| model_matches(model, model_id))
}

pub fn canonical_model_id(endpoint: &EndpointDef, model_id: &str) -> Option<String> {
    if let Some(base_model) = strip_bracket_suffix(model_id) {
        if let Some(model) = find_model(endpoint, base_model) {
            return Some(model.id.clone());
        }
    }
    find_model(endpoint, model_id).map(|model| model.id.clone())
}

pub fn strip_bracket_suffix(model_id: &str) -> Option<&str> {
    let model_id = model_id.trim();
    let (base, suffix) = model_id.rsplit_once('[')?;
    suffix
        .ends_with(']')
        .then(|| base.trim())
        .filter(|base| !base.is_empty())
}

pub fn model_matches(model: &ModelDef, model_id: &str) -> bool {
    let model_id = model_id.trim();
    if model_id.is_empty() {
        return false;
    }
    model.id == model_id || model.aliases.iter().any(|alias| alias.trim() == model_id)
}

// ---------------------------------------------------------------------------
// Synthetic "custom" provider — escape hatch for endpoints not in the
// baked-in catalog. Same render rules as baseline anthropic / openai-chat
// providers, just with no default base_url and no model suggestions; the
// user fills everything in.
// ---------------------------------------------------------------------------

// `compatibility_warning` field stays in EndpointDef for future
// per-provider caveats, but no v1 catalog entry fills it. Earlier we
// painted a blanket "codex requires Responses" warning on every
// openai-chat endpoint; user testing showed at least some third-party
// providers do serve /v1/responses, so the warning was over-cautious.
// Re-add per-provider when we have concrete evidence a specific
// endpoint breaks.

pub fn custom() -> &'static ProviderCatalog {
    static CUSTOM: LazyLock<ProviderCatalog> = LazyLock::new(|| {
        ProviderCatalog {
        id: "custom".to_string(),
        label: "Custom".to_string(),
        icon: Some("✨".to_string()),
        homepage: None,
        hidden_from_picker: false,
        endpoints: vec![
            EndpointDef {
                id: None,
                label: None,
                api_type: "anthropic".to_string(),
                default_base_url: String::new(),
                append_v1_path: true,
                headers: BTreeMap::new(),
                auth_header: false,
                models: Vec::new(),
                capabilities: EndpointCapabilities::default(),
                compatibility_warning: None,
                auth_modes: vec![AuthModeDef {
                    mode: "api_key".to_string(),
                    label: Some("Use API key".to_string()),
                    fields: vec![FieldDef {
                        name: "api_key".to_string(),
                        label: "API key".to_string(),
                        secret: true,
                        required: true,
                        placeholder: None,
                        validate: None,
                    }],
                    render: Some(RenderRules {
                        env: btree_kv(&[
                            ("ANTHROPIC_API_KEY", "{{api_key}}"),
                            ("ANTHROPIC_BASE_URL", "{{base_url}}"),
                            ("ANTHROPIC_MODEL", "{{model}}"),
                        ]),
                        settings_files: Vec::new(),
                    }),
                }],
            },
            EndpointDef {
                id: None,
                label: None,
                api_type: "openai-responses".to_string(),
                default_base_url: String::new(),
                append_v1_path: true,
                headers: BTreeMap::new(),
                auth_header: false,
                models: Vec::new(),
                capabilities: EndpointCapabilities {
                    reasoning_effort: true,
                    ..EndpointCapabilities::default()
                },
                compatibility_warning: None,
                auth_modes: vec![AuthModeDef {
                    mode: "api_key".to_string(),
                    label: Some("Use API key".to_string()),
                    fields: vec![FieldDef {
                        name: "api_key".to_string(),
                        label: "API key".to_string(),
                        secret: true,
                        required: true,
                        placeholder: None,
                        validate: None,
                    }],
                    render: Some(RenderRules {
                        env: btree_kv(&[("OPENAI_API_KEY", "{{api_key}}")]),
                        settings_files: vec![
                            SettingsFileTemplate {
                                rel_path: "config.toml".to_string(),
                                template: "model = \"{{model}}\"\nmodel_provider = \"custom\"\nmodel_reasoning_effort = \"{{reasoning_effort}}\"\ndisable_response_storage = true\n\n[model_providers.custom]\nname = \"Custom\"\nbase_url = \"{{base_url}}\"\nwire_api = \"responses\"\nrequires_openai_auth = true\n".to_string(),
                            },
                            SettingsFileTemplate {
                                rel_path: "auth.json".to_string(),
                                template: "{\n  \"OPENAI_API_KEY\": \"{{api_key|json}}\"\n}\n".to_string(),
                            },
                        ],
                    }),
                }],
            },
            EndpointDef {
                id: None,
                label: None,
                api_type: "openai-chat".to_string(),
                default_base_url: String::new(),
                append_v1_path: true,
                headers: BTreeMap::new(),
                auth_header: false,
                models: Vec::new(),
                capabilities: EndpointCapabilities::default(),
                compatibility_warning: None,
                auth_modes: vec![AuthModeDef {
                    mode: "api_key".to_string(),
                    label: Some("Use API key".to_string()),
                    fields: vec![FieldDef {
                        name: "api_key".to_string(),
                        label: "API key".to_string(),
                        secret: true,
                        required: true,
                        placeholder: None,
                        validate: None,
                    }],
                    render: Some(RenderRules {
                        env: btree_kv(&[("OPENAI_API_KEY", "{{api_key}}")]),
                        settings_files: vec![
                            SettingsFileTemplate {
                                rel_path: "config.toml".to_string(),
                                template: "model = \"{{model}}\"\nmodel_provider = \"custom\"\nmodel_reasoning_effort = \"high\"\ndisable_response_storage = true\n\n[model_providers.custom]\nname = \"Custom\"\nbase_url = \"{{base_url}}\"\nwire_api = \"chat\"\nrequires_openai_auth = true\n".to_string(),
                            },
                            SettingsFileTemplate {
                                rel_path: "auth.json".to_string(),
                                template: "{\n  \"OPENAI_API_KEY\": \"{{api_key|json}}\"\n}\n".to_string(),
                            },
                        ],
                    }),
                }],
            },
        ],
    }
    });
    &CUSTOM
}

fn btree_kv(pairs: &[(&str, &str)]) -> std::collections::BTreeMap<String, String> {
    pairs
        .iter()
        .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
        .collect()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn model<'a>(endpoint: &'a EndpointDef, id: &str) -> &'a ModelDef {
        find_model(endpoint, id).unwrap_or_else(|| panic!("model {id} must exist"))
    }

    #[test]
    fn baseline_catalog_parses() {
        // Touching `all()` triggers the LazyLock; the panic-on-parse-error
        // contract above means a successful call here proves all bundled
        // JSONs are well-formed.
        let entries = all();
        assert!(entries.len() >= 5);
        assert!(get("moonshot").is_some());
        assert!(get("kimi").is_some());
        assert!(get("dashscope").is_some());
        assert!(get("openrouter").is_some());
        assert!(get("minimax").is_some());
        assert!(get("mimo").is_some());
        assert!(get("deepseek").is_some());
        assert!(get("volcengine").is_some());
        assert!(get("zai").is_some());
        assert!(get("gemini").is_some());
        assert!(get("xai").is_some());
        assert!(get("nvidia").is_some());
        assert!(get("azure").is_some());
    }

    #[test]
    fn moonshot_supports_both_api_types() {
        let provider = get("moonshot").expect("moonshot must exist");
        let api_types: Vec<_> = provider
            .endpoints
            .iter()
            .map(|e| e.api_type.as_str())
            .collect();
        assert!(api_types.contains(&"anthropic"));
        assert!(api_types.contains(&"openai-chat"));
        let kimi_coding =
            find_endpoint(provider, "anthropic", Some("kimi-coding")).expect("kimi coding");
        assert_eq!(kimi_coding.default_base_url, "https://api.kimi.com/coding/");
        assert_eq!(
            kimi_coding.headers.get("User-Agent").map(String::as_str),
            Some("claude-code/0.1.0")
        );
    }

    #[test]
    fn kimi_coding_sets_coding_endpoint_headers() {
        let provider = get("kimi").expect("kimi must exist");
        assert!(provider.hidden_from_picker);
        let endpoint = provider
            .endpoints
            .iter()
            .find(|e| e.api_type == "anthropic")
            .expect("kimi anthropic endpoint");
        assert_eq!(endpoint.default_base_url, "https://api.kimi.com/coding/");
        assert_eq!(
            endpoint.headers.get("User-Agent").map(String::as_str),
            Some("claude-code/0.1.0")
        );
        assert_eq!(
            endpoint.models.first().map(|model| model.id.as_str()),
            Some("kimi-for-coding")
        );
        let coding_model = model(endpoint, "kimi-for-coding");
        assert_eq!(
            coding_model.label.as_deref(),
            Some("Kimi Code (powered by Kimi K2.6)")
        );
        assert_eq!(coding_model.context_window, Some(256_000));
        assert_eq!(
            model(endpoint, "k2p5").label.as_deref(),
            Some("Kimi Code (legacy K2.5)")
        );
    }

    #[test]
    fn moonshot_catalog_tracks_kimi_k26() {
        let provider = get("moonshot").expect("moonshot must exist");
        let anthropic =
            find_endpoint(provider, "anthropic", None).expect("moonshot anthropic endpoint");
        for id in ["kimi-k2.6", "kimi-k2-0905-preview", "kimi-k2-turbo-preview"] {
            assert_eq!(model(anthropic, id).context_window, Some(256_000));
        }

        let openai_chat =
            find_endpoint(provider, "openai-chat", None).expect("moonshot chat endpoint");
        assert_eq!(
            model(openai_chat, "kimi-k2.6").label.as_deref(),
            Some("Kimi K2.6")
        );
        assert_eq!(
            model(openai_chat, "kimi-k2.6").context_window,
            Some(256_000)
        );
        assert_eq!(
            model(openai_chat, "kimi-k2-0905-preview").context_window,
            Some(256_000)
        );
        assert_eq!(
            model(openai_chat, "moonshot-v1-32k").context_window,
            Some(32_768)
        );
        assert_eq!(
            model(openai_chat, "moonshot-v1-128k").context_window,
            Some(131_072)
        );
    }

    #[test]
    fn dashscope_exposes_protocol_specific_plan_endpoints() {
        let provider = get("dashscope").expect("dashscope must exist");
        let endpoints: Vec<_> = provider
            .endpoints
            .iter()
            .filter(|e| e.api_type == "openai-chat")
            .map(endpoint_id)
            .collect();
        assert_eq!(
            endpoints,
            vec![
                "coding-plan",
                "coding-plan-cn",
                "token-plan",
                "token-plan-cn"
            ]
        );
        let anthropic_endpoints: Vec<_> = provider
            .endpoints
            .iter()
            .filter(|e| e.api_type == "anthropic")
            .map(endpoint_id)
            .collect();
        assert_eq!(anthropic_endpoints, vec!["coding-plan", "coding-plan-cn"]);

        for &endpoint_id in &endpoints {
            let endpoint = find_endpoint(provider, "openai-chat", Some(endpoint_id))
                .unwrap_or_else(|| panic!("dashscope openai-chat endpoint {endpoint_id}"));
            assert_eq!(
                endpoint.headers.get("User-Agent").map(String::as_str),
                Some("codex-cli/0.80.0 (external, cli)")
            );
            assert_eq!(
                endpoint
                    .headers
                    .get("X-DashScope-UserAgent")
                    .map(String::as_str),
                Some("codex-cli/0.80.0 (external, cli)")
            );
            assert_eq!(
                endpoint
                    .headers
                    .get("X-DashScope-AuthType")
                    .map(String::as_str),
                Some("openai")
            );
        }
        for &endpoint_id in &anthropic_endpoints {
            let endpoint = find_endpoint(provider, "anthropic", Some(endpoint_id))
                .unwrap_or_else(|| panic!("dashscope anthropic endpoint {endpoint_id}"));
            assert_eq!(
                endpoint.headers.get("User-Agent").map(String::as_str),
                Some("claude-code/0.1.0")
            );
            assert!(endpoint.auth_header);
        }

        let token = find_endpoint(provider, "openai-chat", Some("token-plan"))
            .expect("token plan endpoint");
        assert_eq!(
            token.default_base_url,
            "https://dashscope-intl.aliyuncs.com/compatible-mode/v1"
        );

        let coding = find_endpoint(provider, "openai-chat", Some("coding-plan"))
            .expect("coding plan endpoint");
        assert_eq!(
            coding.default_base_url,
            "https://coding-intl.dashscope.aliyuncs.com/v1"
        );
        assert_eq!(
            coding
                .headers
                .get("X-DashScope-UserAgent")
                .map(String::as_str),
            Some("codex-cli/0.80.0 (external, cli)")
        );
        assert_eq!(
            coding
                .headers
                .get("X-DashScope-AuthType")
                .map(String::as_str),
            Some("openai")
        );
        assert!(coding.models.iter().any(|model| model.id == "qwen3.6-plus"));
        assert!(
            coding.models.iter().any(|model| model.id == "kimi-k2.5"),
            "Coding Plan still advertises Kimi K2.5 in the official docs"
        );
        assert!(
            coding.models.iter().all(|model| model.id != "kimi-k2.6"),
            "Do not list Kimi K2.6 on Coding Plan until the official docs do"
        );

        let anthropic = find_endpoint(provider, "anthropic", Some("coding-plan"))
            .expect("coding plan anthropic endpoint");
        assert_eq!(
            anthropic.default_base_url,
            "https://coding-intl.dashscope.aliyuncs.com/apps/anthropic"
        );
        assert!(anthropic.auth_header);
    }

    #[test]
    fn dashscope_token_plan_tracks_current_chinese_models() {
        let provider = get("dashscope").expect("dashscope must exist");
        for endpoint_id in ["token-plan", "token-plan-cn"] {
            let endpoint = find_endpoint(provider, "openai-chat", Some(endpoint_id))
                .unwrap_or_else(|| panic!("dashscope endpoint {endpoint_id}"));
            for (id, context_window) in [
                ("qwen3.6-max-preview", 256_000),
                ("qwen3.6-plus", 1_000_000),
                ("qwen3.6-flash", 1_000_000),
                ("deepseek-v4-pro", 1_000_000),
                ("deepseek-v4-flash", 1_000_000),
                ("glm-5.1", 198_000),
                ("kimi-k2.6", 256_000),
            ] {
                assert_eq!(model(endpoint, id).context_window, Some(context_window));
            }
            assert!(model(endpoint, "qwen3.6-plus").capabilities.image_input);
            assert!(model(endpoint, "qwen3.6-flash").capabilities.image_input);
            assert!(model(endpoint, "kimi-k2.6").capabilities.image_input);
        }
    }

    #[test]
    fn minimax_anthropic_endpoint_uses_auth_header() {
        let provider = get("minimax").expect("minimax must exist");
        let global = find_endpoint(provider, "anthropic", Some("global")).expect("global endpoint");
        let cn = find_endpoint(provider, "anthropic", Some("cn")).expect("cn endpoint");
        assert_eq!(global.default_base_url, "https://api.minimax.io/anthropic");
        assert_eq!(cn.default_base_url, "https://api.minimaxi.com/anthropic");
        assert!(global.auth_header);
        assert!(cn.auth_header);
        let auth = global
            .auth_modes
            .iter()
            .find(|auth| auth.mode == "api_key")
            .expect("api key auth");
        let render = auth.render.as_ref().expect("api key auth renders env");
        assert_eq!(
            render.env.get("ANTHROPIC_AUTH_TOKEN").map(String::as_str),
            Some("{{api_key}}")
        );
        assert!(render.env.get("ANTHROPIC_API_KEY").is_none());
    }

    #[test]
    fn mimo_catalog_exposes_token_plan_chat_endpoints() {
        let provider = get("mimo").expect("mimo must exist");
        let payg = find_endpoint(provider, "openai-chat", Some("pay-as-you-go"))
            .expect("mimo pay-as-you-go endpoint");

        assert!(!payg.append_v1_path);
        assert_eq!(payg.default_base_url, "https://api.xiaomimimo.com/v1");
        for (endpoint_id, base_url) in [
            ("token-plan-cn", "https://token-plan-cn.xiaomimimo.com/v1"),
            ("token-plan-sgp", "https://token-plan-sgp.xiaomimimo.com/v1"),
            ("token-plan-ams", "https://token-plan-ams.xiaomimimo.com/v1"),
        ] {
            let endpoint = find_endpoint(provider, "openai-chat", Some(endpoint_id))
                .unwrap_or_else(|| panic!("mimo token-plan endpoint {endpoint_id}"));
            assert_eq!(endpoint.default_base_url, base_url);
            assert!(!endpoint.append_v1_path);
        }
        assert_eq!(
            model(payg, "mimo-v2.5-pro").label.as_deref(),
            Some("MiMo V2.5 Pro")
        );
        for endpoint in &provider.endpoints {
            assert_eq!(
                model(endpoint, "mimo-v2.5-pro").context_window,
                Some(1_000_000)
            );
            assert_eq!(model(endpoint, "mimo-v2.5").context_window, Some(1_000_000));
            assert_eq!(
                model(endpoint, "mimo-v2-pro").context_window,
                Some(1_000_000)
            );
            assert_eq!(
                model(endpoint, "mimo-v2-omni").context_window,
                Some(256_000)
            );
            assert_eq!(
                model(endpoint, "mimo-v2-flash").context_window,
                Some(256_000)
            );
        }
    }

    #[test]
    fn xai_catalog_supports_grok_responses_and_chat() {
        let provider = get("xai").expect("xai must exist");
        for api_type in ["openai-responses", "openai-chat"] {
            let endpoint =
                find_endpoint(provider, api_type, None).expect("xai endpoint must exist");
            assert_eq!(endpoint.default_base_url, "https://api.x.ai/v1");
            assert!(!endpoint.append_v1_path);
            assert!(endpoint.capabilities.reasoning_effort);
            let grok = model(endpoint, "grok-latest");
            assert_eq!(grok.id, "grok-4.3");
            assert_eq!(grok.context_window, Some(1_000_000));
            assert!(grok.capabilities.image_input);
            let build = model(endpoint, "grok-code-fast");
            assert_eq!(build.id, "grok-build-0.1");
            assert_eq!(build.context_window, Some(256_000));
        }
    }

    #[test]
    fn volcengine_catalog_separates_products_and_protocols() {
        let provider = get("volcengine").expect("volcengine must exist");

        let anthropic_ids: Vec<_> = provider
            .endpoints
            .iter()
            .filter(|endpoint| endpoint.api_type == "anthropic")
            .map(endpoint_id)
            .collect();
        assert_eq!(anthropic_ids, vec!["ark-api", "coding-plan", "agent-plan"]);

        let chat_ids: Vec<_> = provider
            .endpoints
            .iter()
            .filter(|endpoint| endpoint.api_type == "openai-chat")
            .map(endpoint_id)
            .collect();
        assert_eq!(chat_ids, vec!["ark-api", "coding-plan", "agent-plan"]);

        let response_ids: Vec<_> = provider
            .endpoints
            .iter()
            .filter(|endpoint| endpoint.api_type == "openai-responses")
            .map(endpoint_id)
            .collect();
        assert_eq!(response_ids, vec!["ark-api"]);

        let ark_anthropic =
            find_endpoint(provider, "anthropic", Some("ark-api")).expect("ark anthropic");
        assert_eq!(
            ark_anthropic.default_base_url,
            "https://ark.cn-beijing.volces.com/api/compatible"
        );
        assert!(ark_anthropic.append_v1_path);
        assert!(ark_anthropic.auth_header);
        assert_eq!(
            model(ark_anthropic, "doubao-seed-code").id,
            "doubao-seed-code-preview-251028"
        );

        for api_type in ["openai-responses", "openai-chat"] {
            let endpoint =
                find_endpoint(provider, api_type, Some("ark-api")).expect("ark endpoint");
            assert_eq!(
                endpoint.default_base_url,
                "https://ark.cn-beijing.volces.com/api/v3"
            );
            assert!(!endpoint.append_v1_path);
            assert!(endpoint.capabilities.reasoning_effort);

            let code = model(endpoint, "doubao-seed-code");
            assert_eq!(code.id, "doubao-seed-code-preview-251028");
            assert_eq!(code.context_window, Some(256_000));
            assert!(code.capabilities.image_input);

            let deepseek = model(endpoint, "deepseek-v4-pro");
            assert_eq!(deepseek.id, "deepseek-v4-pro-260425");
            assert_eq!(deepseek.context_window, Some(1_024_000));
        }

        let coding_anthropic =
            find_endpoint(provider, "anthropic", Some("coding-plan")).expect("coding anthropic");
        assert_eq!(
            coding_anthropic.default_base_url,
            "https://ark.cn-beijing.volces.com/api/coding"
        );
        assert!(coding_anthropic.append_v1_path);
        assert!(coding_anthropic.auth_header);
        assert!(find_model(coding_anthropic, "ark-code-latest").is_some());
        assert!(find_model(coding_anthropic, "doubao-seed-2.0-code").is_some());
        assert!(find_model(coding_anthropic, "minimax-latest").is_some());
        assert!(find_model(coding_anthropic, "doubao-seed-code-preview-251028").is_none());

        let coding_chat =
            find_endpoint(provider, "openai-chat", Some("coding-plan")).expect("coding chat");
        assert_eq!(
            coding_chat.default_base_url,
            "https://ark.cn-beijing.volces.com/api/coding/v3"
        );
        assert!(!coding_chat.append_v1_path);
        assert!(find_model(coding_chat, "kimi-k2.6").is_some());

        let agent_anthropic =
            find_endpoint(provider, "anthropic", Some("agent-plan")).expect("agent anthropic");
        assert_eq!(
            agent_anthropic.default_base_url,
            "https://ark.cn-beijing.volces.com/api/plan"
        );
        assert!(agent_anthropic.append_v1_path);
        assert!(agent_anthropic.auth_header);
        assert!(find_model(agent_anthropic, "ark-code-latest").is_some());
        assert!(find_model(agent_anthropic, "doubao-seed-2.0-mini").is_some());
        assert!(find_model(agent_anthropic, "doubao-seedance-2.0").is_none());

        let agent_chat =
            find_endpoint(provider, "openai-chat", Some("agent-plan")).expect("agent chat");
        assert_eq!(
            agent_chat.default_base_url,
            "https://ark.cn-beijing.volces.com/api/plan/v3"
        );
        assert!(!agent_chat.append_v1_path);
        assert!(find_model(agent_chat, "minimax-m2.7").is_some());
    }

    #[test]
    fn nvidia_catalog_supports_nim_chat_completions() {
        let provider = get("nvidia").expect("nvidia must exist");
        let endpoint = find_endpoint(provider, "openai-chat", None).expect("nvidia chat endpoint");
        assert_eq!(
            endpoint.default_base_url,
            "https://integrate.api.nvidia.com/v1"
        );
        assert!(!endpoint.append_v1_path);
        assert_eq!(
            model(endpoint, "nvidia/nemotron-3-super-120b-a12b")
                .label
                .as_deref(),
            Some("Nemotron 3 Super 120B A12B")
        );
        for (id, context_window) in [
            ("nvidia/nemotron-3-super-120b-a12b", 1_000_000),
            ("nvidia/nemotron-3-nano-30b-a3b", 1_000_000),
            ("nvidia/nvidia-nemotron-nano-9b-v2", 128_000),
            ("qwen/qwen3-coder-480b-a35b-instruct", 262_144),
            ("openai/gpt-oss-120b", 128_000),
            ("moonshotai/kimi-k2.6", 256_000),
        ] {
            assert_eq!(model(endpoint, id).context_window, Some(context_window));
        }
        assert!(find_model(endpoint, "qwen/qwen3-coder-480b-a35b-instruct").is_some());
        assert!(find_model(endpoint, "openai/gpt-oss-120b").is_some());
    }

    #[test]
    fn zai_catalog_tracks_context_windows() {
        let provider = get("zai").expect("zai must exist");
        for api_type in ["anthropic", "openai-chat"] {
            let endpoint = find_endpoint(provider, api_type, None)
                .unwrap_or_else(|| panic!("zai {api_type} endpoint"));
            assert_eq!(model(endpoint, "glm-5.1").context_window, Some(200_000));
            assert_eq!(model(endpoint, "glm-5").context_window, Some(200_000));
            assert_eq!(model(endpoint, "glm-4.7").context_window, Some(200_000));
            assert_eq!(model(endpoint, "glm-4.5").context_window, Some(128_000));
        }
    }

    #[test]
    fn azure_supports_responses_only() {
        let provider = get("azure").expect("azure must exist");
        let api_types: Vec<_> = provider
            .endpoints
            .iter()
            .map(|e| e.api_type.as_str())
            .collect();
        assert_eq!(api_types, vec!["openai-responses"]);
    }

    #[test]
    fn gemini_supports_api_key_launch() {
        let provider = get("gemini").expect("gemini must exist");
        let endpoint = provider
            .endpoints
            .iter()
            .find(|e| e.api_type == "gemini")
            .expect("gemini endpoint must exist");
        let auth = endpoint
            .auth_modes
            .iter()
            .find(|a| a.mode == "api_key")
            .expect("gemini api_key auth must exist");
        let render = auth.render.as_ref().expect("api_key auth renders env");
        assert_eq!(
            render.env.get("GEMINI_API_KEY").map(String::as_str),
            Some("{{api_key}}")
        );
        assert_eq!(
            render.env.get("GOOGLE_API_KEY").map(String::as_str),
            Some("{{api_key}}")
        );
        assert_eq!(
            render
                .env
                .get("GEMINI_DEFAULT_AUTH_TYPE")
                .map(String::as_str),
            Some("gemini-api-key")
        );
        assert_eq!(
            render.env.get("GEMINI_MODEL").map(String::as_str),
            Some("{{model}}")
        );
        assert_eq!(
            render.env.get("GOOGLE_GEMINI_BASE_URL").map(String::as_str),
            Some("{{base_url}}")
        );
        assert!(render.settings_files.is_empty());
        assert_eq!(endpoint_id(endpoint), "gemini-api");
        let google_accounts = find_endpoint(provider, "gemini", Some("google-accounts"))
            .expect("gemini google accounts endpoint");
        assert_eq!(
            google_accounts.default_base_url,
            "https://cloudcode-pa.googleapis.com"
        );
        let oauth = google_accounts
            .auth_modes
            .iter()
            .find(|auth| auth.mode == "google_oauth")
            .expect("google accounts oauth auth");
        let oauth_render = oauth.render.as_ref().expect("oauth auth renders env");
        assert_eq!(
            oauth_render
                .env
                .get("GEMINI_DEFAULT_AUTH_TYPE")
                .map(String::as_str),
            Some("oauth-personal")
        );
        assert_eq!(
            oauth_render
                .env
                .get("GOOGLE_CLOUD_PROJECT")
                .map(String::as_str),
            Some("{{google_cloud_project}}")
        );
        let openai_chat = find_endpoint(provider, "openai-chat", Some("gemini-api"))
            .expect("gemini openai-compatible endpoint");
        assert_eq!(endpoint_id(openai_chat), "gemini-api");
        assert!(!openai_chat.append_v1_path);
        assert_eq!(
            openai_chat.default_base_url,
            "https://generativelanguage.googleapis.com/v1beta/openai"
        );
        let pro = find_model(openai_chat, "gemini-3.1-pro").expect("gemini pro alias");
        assert_eq!(pro.id, "gemini-3.1-pro-preview");
        assert_eq!(pro.context_window, Some(1_048_576));
        let vertex_chat = find_endpoint(provider, "openai-chat", Some("vertex-openai-compatible"))
            .expect("gemini vertex openai-compatible endpoint");
        assert_eq!(vertex_chat.default_base_url, "");
        assert!(!vertex_chat.append_v1_path);
        assert_eq!(
            vertex_chat.models.first().map(|model| model.id.as_str()),
            Some("google/gemini-2.5-flash")
        );
    }
}
