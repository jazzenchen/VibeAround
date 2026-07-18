//! Plugin session management — spawn, handshake, and JSON-RPC communication
//! with onboarding auth scripts and main plugin entry points.

use std::path::Path;

use anyhow::{anyhow, Context};
use serde::Deserialize;
use serde_json::Value;
use tauri::async_runtime;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, ChildStdout};

use common::{channels::plugin_paths, plugins};

pub struct PluginSession {
    pub(super) child: Child,
    pub(super) stdin: ChildStdin,
    pub(super) stdout: BufReader<ChildStdout>,
    pub(super) next_request_id: u64,
}

/// Spawn a plugin's auth-standalone script (for QR/pairing flows during onboarding).
pub(super) async fn spawn_auth_session(
    name: &str,
    config_value: Value,
) -> anyhow::Result<PluginSession> {
    let plugin = plugins::channel::find(name)
        .ok_or_else(|| anyhow!("plugin '{}' not found or not built", name))?;
    let auth_entry = plugin.dir.join("dist").join("auth-standalone.js");
    if !auth_entry.exists() {
        return Err(anyhow!(
            "auth script not found for plugin '{}' at {:?}",
            name,
            auth_entry
        ));
    }
    let _ = config_value; // reserved for future per-plugin auth config
    spawn_node_session(name, &auth_entry, &plugin.dir).await
}

/// Spawn a plugin's main entry point and perform the ACP initialize handshake.
/// Kept for future runtime plugin management; currently only auth sessions are used during onboarding.
#[allow(dead_code)]
pub(super) async fn spawn_plugin_session(
    name: &str,
    config_value: Value,
) -> anyhow::Result<PluginSession> {
    let plugin = plugins::channel::find(name)
        .ok_or_else(|| anyhow!("plugin '{}' not found or not built", name))?;
    let entry_point = plugin.entry_path();
    let plugin_dir = plugin.dir.clone();
    let mut session = spawn_node_session(name, &entry_point, &plugin_dir).await?;

    // ACP handshake: read the client's initialize request, respond with server config
    let client_init_id: Value;
    loop {
        let mut line = String::new();
        let bytes = session
            .stdout
            .read_line(&mut line)
            .await
            .context("reading plugin initialize request")?;
        if bytes == 0 {
            return Err(anyhow!(
                "plugin '{}' exited before sending initialize",
                name
            ));
        }
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let msg: Value = serde_json::from_str(trimmed).context("parsing plugin message")?;
        if msg.get("method").and_then(|v| v.as_str()) == Some("initialize") {
            client_init_id = msg.get("id").cloned().unwrap_or(Value::Null);
            break;
        }
    }

    let runtime_dirs = plugin_paths::plugin_runtime_dirs(name);
    let init_response = serde_json::json!({
        "jsonrpc": "2.0",
        "id": client_init_id,
        "result": {
            "protocolVersion": "2025-03-26",
            "agentInfo": { "name": "vibearound-onboarding", "version": env!("CARGO_PKG_VERSION") },
            "_meta": {
                "config": config_value,
                "cacheDir": runtime_dirs.cache.to_string_lossy(),
                "channelKind": name,
            }
        }
    });
    let line = serde_json::to_string(&init_response).context("serializing init response")? + "\n";
    session
        .stdin
        .write_all(line.as_bytes())
        .await
        .context("writing init response")?;
    session
        .stdin
        .flush()
        .await
        .context("flushing init response")?;

    Ok(session)
}

/// Spawn a Node.js script, wire stderr logging, and perform a raw JSON-RPC initialize handshake.
async fn spawn_node_session(
    name: &str,
    entry_point: &Path,
    plugin_dir: &Path,
) -> anyhow::Result<PluginSession> {
    let runtime_dirs = plugin_paths::prepare_plugin_runtime_dirs(name)
        .with_context(|| format!("preparing runtime directories for plugin '{}'", name))?;
    let mut child = common::process::env::command("node")
        .arg(entry_point)
        .current_dir(plugin_dir)
        .env(plugin_paths::PLUGIN_STATE_DIR_ENV, &runtime_dirs.state)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .with_context(|| format!("failed to spawn node for plugin '{}'", name))?;

    let mut stdin = child.stdin.take().context("stdin unavailable")?;
    let stdout = child.stdout.take().context("stdout unavailable")?;
    if let Some(stderr) = child.stderr.take() {
        let name = name.to_string();
        async_runtime::spawn(async move {
            let reader = BufReader::new(stderr);
            let mut lines = reader.lines();
            while let Ok(Some(line)) = lines.next_line().await {
                tracing::info!("[onboarding:{}] {}", name, line);
            }
        });
    }

    // Send raw JSON-RPC initialize and wait for the matching response
    let init_req = serde_json::json!({
        "jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {}
    });
    let line = serde_json::to_string(&init_req).context("serializing initialize")? + "\n";
    stdin
        .write_all(line.as_bytes())
        .await
        .context("writing initialize")?;
    stdin.flush().await.context("flushing initialize")?;

    let mut stdout = BufReader::new(stdout);
    loop {
        let mut line = String::new();
        let bytes = stdout
            .read_line(&mut line)
            .await
            .context("reading initialize response")?;
        if bytes == 0 {
            return Err(anyhow!("'{}' exited before initialize completed", name));
        }
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let msg: Value = serde_json::from_str(trimmed).context("parsing initialize response")?;
        if msg.get("id").and_then(|v| v.as_u64()) == Some(1) {
            if let Some(error) = msg.get("error") {
                let msg = error
                    .get("message")
                    .and_then(|v| v.as_str())
                    .unwrap_or("init error");
                return Err(anyhow!("{}", msg));
            }
            break;
        }
    }

    Ok(PluginSession {
        child,
        stdin,
        stdout,
        next_request_id: 2,
    })
}

/// Send a JSON-RPC request and wait for the matching response ID.
pub(super) async fn plugin_request<T: for<'de> Deserialize<'de>>(
    session: &mut PluginSession,
    method: &str,
    params: Value,
) -> anyhow::Result<T> {
    let request_id = session.next_request_id;
    session.next_request_id += 1;

    let req = serde_json::json!({
        "jsonrpc": "2.0",
        "id": request_id,
        "method": method,
        "params": params,
    });
    let line = serde_json::to_string(&req).context("serializing request")? + "\n";
    session
        .stdin
        .write_all(line.as_bytes())
        .await
        .with_context(|| format!("writing request '{}'", method))?;
    session.stdin.flush().await.context("flushing request")?;

    loop {
        let mut line = String::new();
        let bytes = session
            .stdout
            .read_line(&mut line)
            .await
            .context("reading response")?;
        if bytes == 0 {
            return Err(anyhow!(
                "plugin request '{}' ended without a response",
                method
            ));
        }
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let msg: Value = serde_json::from_str(trimmed).context("parsing response")?;
        if msg.get("id").and_then(|v| v.as_u64()) != Some(request_id) {
            continue;
        }
        if let Some(error) = msg.get("error") {
            let message = error
                .get("message")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown plugin error");
            return Err(anyhow!("{}", message));
        }
        let result = msg.get("result").cloned().unwrap_or(Value::Null);
        return serde_json::from_value::<T>(result).context("deserializing response");
    }
}

/// Send a shutdown request and kill the process.
pub(super) async fn shutdown_plugin_session(session: &mut PluginSession) {
    let request_id = session.next_request_id;
    session.next_request_id += 1;
    let req = serde_json::json!({
        "jsonrpc": "2.0", "id": request_id, "method": "shutdown", "params": {}
    });
    if let Ok(line) = serde_json::to_string(&req) {
        let _ = session.stdin.write_all((line + "\n").as_bytes()).await;
        let _ = session.stdin.flush().await;
    }
    let _ = session.child.kill().await;
}
