use std::collections::HashMap;
use std::env;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use anyhow::anyhow;
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::sync::{mpsc, oneshot};
use tokio::time::timeout;

use crate::config::SearchToolConfig;
use crate::process::bridge::{
    BridgeExit, BridgeFactory, BridgeFuture, CancelSignal, ProcessBridge,
};
use crate::process::supervisor::{ProcessId, RestartPolicy, SpawnSpec, Supervisor};
use crate::process::ProcessKind;
use crate::process::StdioPipes;

const SEARCH_TOOL_ENV: &str = "VA_SEARCH_TOOL_STDIO";
const SEARCH_TOOL_LABEL: &str = "va-search-tool";
const SEARCH_TOOL_PLUGIN_ID: &str = "va-search-tool";
const SEARCH_REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
const SEARCH_READY_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WebSearchRequest {
    pub query: String,
    #[serde(default)]
    pub max_results: Option<usize>,
    #[serde(default)]
    pub include_domains: Vec<String>,
    #[serde(default)]
    pub exclude_domains: Vec<String>,
    #[serde(default)]
    pub search_context_size: Option<String>,
    #[serde(default)]
    pub providers: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WebSearchResponse {
    pub provider: String,
    pub query: String,
    pub results: Vec<WebSearchResult>,
    pub citations: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WebSearchResult {
    pub title: String,
    pub url: String,
    pub snippet: String,
    pub content: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub score: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub published_date: Option<String>,
    pub source: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchError {
    pub message: String,
}

impl SearchError {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl std::fmt::Display for SearchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for SearchError {}

#[derive(Clone)]
pub struct SearchToolRuntime {
    process_id: ProcessId,
    requests: mpsc::Sender<SearchCommand>,
}

impl SearchToolRuntime {
    pub async fn spawn_if_enabled(config: &SearchToolConfig) -> anyhow::Result<Option<Arc<Self>>> {
        if !config.has_enabled_sources() {
            tracing::info!("host web search fallback unavailable; no search sources enabled");
            return Ok(None);
        }

        let Some(launch) = search_tool_launch(config) else {
            tracing::warn!(
                "va-search-tool executable not found; host web search requests will fail"
            );
            return Ok(None);
        };

        let (request_tx, request_rx) = mpsc::channel::<SearchCommand>(64);
        let (ready_tx, ready_rx) = oneshot::channel::<()>();
        let bridge = SearchToolBridge {
            request_rx,
            ready_tx: Some(ready_tx),
        };
        let mut bridge = Some(bridge);
        let factory: BridgeFactory = Box::new(move || {
            let bridge = bridge.take().expect(
                "SearchToolBridge factory called more than once; RestartPolicy::Never is used",
            );
            Box::new(bridge) as Box<dyn ProcessBridge>
        });

        let mut spec = SpawnSpec::new(launch.program).args(launch.args);
        for (key, value) in search_tool_env(config) {
            spec = spec.env(key, value);
        }
        let supervisor = Supervisor::global();
        let process_id = supervisor
            .register(
                ProcessKind::SearchProvider,
                SEARCH_TOOL_LABEL,
                spec,
                RestartPolicy::Never,
                factory,
            )
            .await;

        match timeout(SEARCH_READY_TIMEOUT, ready_rx).await {
            Ok(Ok(())) => {
                tracing::info!(process_id = %process_id, "va-search-tool runtime ready");
            }
            Ok(Err(_)) | Err(_) => {
                let _ = supervisor.force_stop(process_id).await;
                tracing::warn!(
                    "va-search-tool did not become ready; host web search requests will fail"
                );
                return Ok(None);
            }
        }

        Ok(Some(Arc::new(Self {
            process_id,
            requests: request_tx,
        })))
    }

    pub async fn search(
        &self,
        request: WebSearchRequest,
    ) -> Result<WebSearchResponse, SearchError> {
        let (tx, rx) = oneshot::channel();
        self.requests
            .send(SearchCommand { request, tx })
            .await
            .map_err(|_| SearchError::new("search provider runtime is not running"))?;
        timeout(SEARCH_REQUEST_TIMEOUT, rx)
            .await
            .map_err(|_| SearchError::new("search provider request timed out"))?
            .map_err(|_| SearchError::new("search provider dropped the response"))?
    }

    pub async fn shutdown(&self) {
        if let Err(error) = Supervisor::global().force_stop(self.process_id).await {
            tracing::warn!(error = %error, "failed to stop va-search-tool runtime");
        }
    }
}

struct SearchCommand {
    request: WebSearchRequest,
    tx: oneshot::Sender<Result<WebSearchResponse, SearchError>>,
}

struct SearchToolBridge {
    request_rx: mpsc::Receiver<SearchCommand>,
    ready_tx: Option<oneshot::Sender<()>>,
}

impl ProcessBridge for SearchToolBridge {
    fn run(self: Box<Self>, pipes: StdioPipes, cancel: CancelSignal) -> BridgeFuture {
        Box::pin(async move { self.drive(pipes, cancel).await })
    }
}

impl SearchToolBridge {
    async fn drive(mut self: Box<Self>, pipes: StdioPipes, mut cancel: CancelSignal) -> BridgeExit {
        let mut stdin = pipes.stdin;
        let mut stdout = BufReader::new(pipes.stdout).lines();
        let mut pending: HashMap<String, oneshot::Sender<Result<WebSearchResponse, SearchError>>> =
            HashMap::new();
        let mut next_id = 1_u64;

        if let Some(ready_tx) = self.ready_tx.take() {
            let _ = ready_tx.send(());
        }

        loop {
            tokio::select! {
                changed = cancel.changed() => {
                    if changed.is_err() || *cancel.borrow() {
                        fail_pending(&mut pending, "search provider runtime stopped");
                        return BridgeExit::Cancelled;
                    }
                }
                command = self.request_rx.recv() => {
                    let Some(command) = command else {
                        fail_pending(&mut pending, "search provider request channel closed");
                        return BridgeExit::Clean;
                    };
                    let id = format!("search_{next_id}");
                    next_id = next_id.wrapping_add(1);
                    let payload = SearchToolRpcRequest {
                        jsonrpc: "2.0",
                        id: id.clone(),
                        method: "web_search".to_string(),
                        params: command.request,
                    };
                    let mut line = match serde_json::to_vec(&payload) {
                        Ok(line) => line,
                        Err(error) => {
                            let _ = command.tx.send(Err(SearchError::new(format!(
                                "failed to encode search request: {error}"
                            ))));
                            continue;
                        }
                    };
                    line.push(b'\n');
                    if let Err(error) = stdin.write_all(&line).await {
                        let _ = command.tx.send(Err(SearchError::new(format!(
                            "failed to write search request: {error}"
                        ))));
                        fail_pending(&mut pending, "search provider stdin closed");
                        return BridgeExit::ProtocolError(anyhow!(error).context("search provider stdin closed"));
                    }
                    pending.insert(id, command.tx);
                }
                line = stdout.next_line() => {
                    match line {
                        Ok(Some(line)) => handle_search_response_line(&line, &mut pending),
                        Ok(None) => {
                            fail_pending(&mut pending, "search provider stdout closed");
                            return BridgeExit::Clean;
                        }
                        Err(error) => {
                            fail_pending(&mut pending, "failed to read search provider stdout");
                            return BridgeExit::ProtocolError(anyhow!(error).context("failed to read search provider stdout"));
                        }
                    }
                }
            }
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct SearchToolRpcRequest {
    jsonrpc: &'static str,
    id: String,
    method: String,
    params: WebSearchRequest,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SearchToolRpcResponse {
    id: Option<String>,
    result: Option<WebSearchResponse>,
    error: Option<SearchToolRpcError>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SearchToolRpcError {
    message: String,
}

fn handle_search_response_line(
    line: &str,
    pending: &mut HashMap<String, oneshot::Sender<Result<WebSearchResponse, SearchError>>>,
) {
    let response = match serde_json::from_str::<SearchToolRpcResponse>(line) {
        Ok(response) => response,
        Err(error) => {
            tracing::warn!(error = %error, "failed to decode search provider response");
            return;
        }
    };
    let Some(id) = response.id else {
        tracing::warn!("search provider response missing id");
        return;
    };
    let Some(tx) = pending.remove(&id) else {
        tracing::warn!(%id, "search provider response had no pending request");
        return;
    };
    let result = match (response.result, response.error) {
        (Some(result), None) => Ok(result),
        (_, Some(error)) => Err(SearchError::new(error.message)),
        (None, None) => Err(SearchError::new("search provider response missing result")),
    };
    let _ = tx.send(result);
}

fn fail_pending(
    pending: &mut HashMap<String, oneshot::Sender<Result<WebSearchResponse, SearchError>>>,
    message: &str,
) {
    for (_, tx) in pending.drain() {
        let _ = tx.send(Err(SearchError::new(message)));
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SearchToolLaunch {
    program: String,
    args: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SearchToolLaunchSource {
    Configured,
    Env,
    InstalledPlugin,
    DevCheckout,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SearchToolLaunchInfo {
    source: SearchToolLaunchSource,
    path: PathBuf,
}

fn search_tool_launch(config: &SearchToolConfig) -> Option<SearchToolLaunch> {
    search_tool_launch_info(config).map(|info| launch_from_path(info.path))
}

fn search_tool_launch_info(config: &SearchToolConfig) -> Option<SearchToolLaunchInfo> {
    configured_search_tool_path(config)
        .map(|path| SearchToolLaunchInfo {
            source: SearchToolLaunchSource::Configured,
            path,
        })
        .or_else(env_search_tool_launch_info)
        .or_else(installed_search_tool_launch_info)
        .or_else(dev_search_tool_launch_info)
}

fn configured_search_tool_path(config: &SearchToolConfig) -> Option<PathBuf> {
    let path = config.stdio_path.as_ref()?;
    if path.exists() {
        Some(path.clone())
    } else {
        tracing::warn!(
            path = ?path,
            "configured va-search-tool executable does not exist; falling back to discovery"
        );
        None
    }
}

fn search_tool_env(config: &SearchToolConfig) -> Vec<(String, String)> {
    let enabled_sources = config.enabled_source_names();
    let mut vars = Vec::new();
    if let Some(max_results) = config.max_results {
        vars.push(("VA_SEARCH_MAX_RESULTS".to_string(), max_results.to_string()));
    }
    if let Some(search_context_size) = &config.search_context_size {
        vars.push((
            "VA_SEARCH_CONTEXT_SIZE".to_string(),
            search_context_size.clone(),
        ));
    }
    if !enabled_sources.is_empty() {
        vars.push(("VA_SEARCH_SOURCES".to_string(), enabled_sources.join(",")));
    }
    for (name, source) in &config.sources {
        let env_prefix = search_source_env_prefix(name);
        vars.push((
            format!("VA_SEARCH_{env_prefix}_ENABLED"),
            source.enabled.to_string(),
        ));
        if !source.enabled {
            continue;
        }
        if let Some(key) = source.api_key.clone().or_else(|| {
            source
                .api_key_env
                .as_deref()
                .and_then(|name| env::var(name).ok())
        }) {
            vars.push((format!("VA_SEARCH_{env_prefix}_API_KEY"), key.clone()));
            if let Some(alias) = well_known_api_key_env(name) {
                vars.push((alias.to_string(), key));
            }
        }
        if let Some(base_url) = &source.base_url {
            vars.push((format!("VA_SEARCH_{env_prefix}_BASE_URL"), base_url.clone()));
        }
    }
    vars
}

fn search_source_env_prefix(name: &str) -> String {
    name.chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() {
                ch.to_ascii_uppercase()
            } else {
                '_'
            }
        })
        .collect::<String>()
}

fn well_known_api_key_env(source: &str) -> Option<&'static str> {
    match source {
        "exa" => Some("EXA_API_KEY"),
        "tavily" => Some("TAVILY_API_KEY"),
        "grok" | "xai" => Some("XAI_API_KEY"),
        "brave" => Some("BRAVE_API_KEY"),
        _ => None,
    }
}

fn env_search_tool_launch_info() -> Option<SearchToolLaunchInfo> {
    env::var_os(SEARCH_TOOL_ENV)
        .map(PathBuf::from)
        .filter(|path| path.exists())
        .map(|path| SearchToolLaunchInfo {
            source: SearchToolLaunchSource::Env,
            path,
        })
}

fn installed_search_tool_launch_info() -> Option<SearchToolLaunchInfo> {
    let plugin = crate::plugins::find(SEARCH_TOOL_PLUGIN_ID)?;
    if plugin.manifest.kind != "search" {
        tracing::warn!(
            plugin_id = SEARCH_TOOL_PLUGIN_ID,
            kind = %plugin.manifest.kind,
            "search tool plugin has unexpected kind"
        );
        return None;
    }
    let entry = plugin.entry_path();
    if !entry.exists() {
        tracing::warn!(
            path = ?entry,
            "installed va-search-tool plugin entry does not exist"
        );
        return None;
    }
    Some(SearchToolLaunchInfo {
        source: SearchToolLaunchSource::InstalledPlugin,
        path: entry,
    })
}

fn dev_search_tool_launch_info() -> Option<SearchToolLaunchInfo> {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let script = manifest_dir.join("../../../va-search-tool/dist/main.js");
    script.exists().then_some(SearchToolLaunchInfo {
        source: SearchToolLaunchSource::DevCheckout,
        path: script,
    })
}

fn launch_from_path(path: PathBuf) -> SearchToolLaunch {
    if is_node_script(&path) {
        launch_node_script(path)
    } else {
        SearchToolLaunch {
            program: path.to_string_lossy().to_string(),
            args: vec!["stdio".to_string()],
        }
    }
}

fn launch_node_script(path: PathBuf) -> SearchToolLaunch {
    SearchToolLaunch {
        program: "node".to_string(),
        args: vec![path.to_string_lossy().to_string(), "stdio".to_string()],
    }
}

fn is_node_script(path: &std::path::Path) -> bool {
    matches!(
        path.extension().and_then(|value| value.to_str()),
        Some("js" | "mjs" | "cjs")
    )
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, HashMap};
    use std::fs;

    use crate::config::{SearchSourceConfig, SearchToolConfig};

    use super::*;

    #[test]
    fn search_tool_env_only_exports_keys_for_enabled_sources() {
        let config = SearchToolConfig {
            stdio_path: None,
            max_results: Some(7),
            search_context_size: Some("high".to_string()),
            sources: BTreeMap::from([
                (
                    "brave".to_string(),
                    SearchSourceConfig {
                        enabled: true,
                        api_key: Some("brave-key".to_string()),
                        api_key_env: None,
                        base_url: None,
                    },
                ),
                (
                    "exa".to_string(),
                    SearchSourceConfig {
                        enabled: true,
                        api_key: Some("exa-key".to_string()),
                        api_key_env: None,
                        base_url: Some("https://exa.example.test".to_string()),
                    },
                ),
                (
                    "tavily".to_string(),
                    SearchSourceConfig {
                        enabled: false,
                        api_key: Some("tavily-key".to_string()),
                        api_key_env: None,
                        base_url: None,
                    },
                ),
            ]),
        };

        let env = search_tool_env(&config)
            .into_iter()
            .collect::<HashMap<_, _>>();

        assert_eq!(
            env.get("VA_SEARCH_SOURCES").map(String::as_str),
            Some("exa,brave")
        );
        assert_eq!(
            env.get("VA_SEARCH_MAX_RESULTS").map(String::as_str),
            Some("7")
        );
        assert_eq!(
            env.get("VA_SEARCH_CONTEXT_SIZE").map(String::as_str),
            Some("high")
        );
        assert_eq!(
            env.get("VA_SEARCH_EXA_API_KEY").map(String::as_str),
            Some("exa-key")
        );
        assert_eq!(env.get("EXA_API_KEY").map(String::as_str), Some("exa-key"));
        assert_eq!(
            env.get("VA_SEARCH_EXA_BASE_URL").map(String::as_str),
            Some("https://exa.example.test")
        );
        assert_eq!(
            env.get("VA_SEARCH_TAVILY_ENABLED").map(String::as_str),
            Some("false")
        );
        assert!(!env.contains_key("VA_SEARCH_TAVILY_API_KEY"));
        assert!(!env.contains_key("TAVILY_API_KEY"));
        assert_eq!(
            env.get("VA_SEARCH_BRAVE_API_KEY").map(String::as_str),
            Some("brave-key")
        );
        assert_eq!(
            env.get("BRAVE_API_KEY").map(String::as_str),
            Some("brave-key")
        );
    }

    #[test]
    fn node_search_tool_scripts_spawn_through_node() {
        let launch = launch_from_path(PathBuf::from("/tmp/va-search-tool/dist/main.js"));

        assert_eq!(launch.program, "node");
        assert_eq!(
            launch.args,
            vec![
                "/tmp/va-search-tool/dist/main.js".to_string(),
                "stdio".to_string()
            ]
        );
    }

    #[test]
    fn configured_search_tool_path_reports_launch_info() {
        let path = std::env::temp_dir().join(format!("va-search-tool-test-{}", std::process::id()));
        fs::write(&path, "").expect("write fake search tool");
        let config = SearchToolConfig {
            stdio_path: Some(path.clone()),
            ..SearchToolConfig::default()
        };

        let info = search_tool_launch_info(&config).expect("search tool launch info");

        assert_eq!(info.source, SearchToolLaunchSource::Configured);
        assert_eq!(info.path, path);
        let _ = fs::remove_file(info.path);
    }
}
