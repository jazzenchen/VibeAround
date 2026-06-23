#[derive(Debug, thiserror::Error)]
pub(crate) enum CliError {
    #[error("{0}")]
    Usage(String),
    #[error("auth is required; pass --base-url and --token, or start VibeAround so auth.json exists at {0}")]
    MissingAuth(String),
    #[error("auth is required for this command; pass --token or set VIBEAROUND_TOKEN when using --base-url")]
    MissingToken,
    #[error("failed to read auth file {path}: {source}")]
    ReadAuth {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to reach {url}: {source}\ntry starting the server with `bun server:dev`, or pass --base-url")]
    Http {
        url: String,
        #[source]
        source: reqwest::Error,
    },
    #[error("websocket error for {url}: {source}")]
    WebSocket {
        url: String,
        #[source]
        source: tokio_tungstenite::tungstenite::Error,
    },
    #[error("I/O error while {action}: {source}")]
    Io {
        action: &'static str,
        #[source]
        source: std::io::Error,
    },
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("client error: {0}")]
    Client(#[from] va_client::ClientError),
}
