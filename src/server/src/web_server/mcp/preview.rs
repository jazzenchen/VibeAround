use std::path::{Path, PathBuf};

use axum::Json;
use serde_json::Value;

use crate::web_server::AppState;

use super::jsonrpc::{jsonrpc_err, mcp_error_text, mcp_text};
use super::preview_conversation::{resolve_preview_parent_thread, PreviewParentRequest};
use super::session_identity::{argument_string, codex_session_id_from_mcp_metadata};
use super::tools::validate_workspace;

fn preview_parent_request(arguments: &Value, metadata: Option<&Value>) -> PreviewParentRequest {
    PreviewParentRequest {
        thread_id: argument_string(arguments, "thread_id"),
        agent_kind: argument_string(arguments, "agent_kind")
            .or_else(|| argument_string(arguments, "agent_type")),
        session_id: argument_string(arguments, "session_id"),
        codex_metadata_session_id: codex_session_id_from_mcp_metadata(metadata),
    }
}

#[derive(Debug, PartialEq)]
enum PreviewSource<'a> {
    Server(u16),
    File(&'a str),
}

fn preview_source(arguments: &Value) -> Result<PreviewSource<'_>, &'static str> {
    match (arguments.get("port"), arguments.get("file")) {
        (Some(_), Some(_)) | (None, None) => Err("Pass exactly one Preview source: port or file."),
        (Some(port), None) => port
            .as_u64()
            .filter(|port| *port > 0 && *port <= u16::MAX as u64)
            .map(|port| PreviewSource::Server(port as u16))
            .ok_or("Invalid Preview port: expected an integer from 1 to 65535."),
        (None, Some(file)) => file
            .as_str()
            .filter(|file| !file.is_empty())
            .map(PreviewSource::File)
            .ok_or("Invalid Preview file: expected a non-empty path."),
    }
}

pub(super) async fn mcp_preview(
    id: Option<Value>,
    arguments: &Value,
    metadata: Option<&Value>,
    state: &AppState,
) -> Json<Value> {
    let source = match preview_source(arguments) {
        Ok(source) => source,
        Err(message) => return jsonrpc_err(id, -32602, message),
    };
    let cwd = match arguments.get("cwd").and_then(Value::as_str) {
        Some(cwd) => cwd,
        None => return jsonrpc_err(id, -32602, "Missing required argument: cwd"),
    };
    let cwd_path = PathBuf::from(cwd);
    if let Err(response) = validate_workspace(&cwd_path, id.clone()) {
        return response;
    }

    match source {
        PreviewSource::Server(port) => {
            mcp_server_preview(id, arguments, metadata, state, cwd_path, port).await
        }
        PreviewSource::File(file) => {
            mcp_file_preview(id, arguments, metadata, state, cwd_path, file).await
        }
    }
}

async fn mcp_server_preview(
    id: Option<Value>,
    arguments: &Value,
    metadata: Option<&Value>,
    state: &AppState,
    cwd_path: PathBuf,
    port: u16,
) -> Json<Value> {
    let title = derive_title(arguments, &cwd_path);
    let (owner_slug, share) = common::previews::ensure_server(port, cwd_path.clone(), title);
    let warnings =
        initialize_preview_conversation(arguments, metadata, &cwd_path, &owner_slug, state).await;
    let _ = state.preview_refresh_tx.send(owner_slug.clone());
    let local_owner_url = format!(
        "http://127.0.0.1:{}/va/preview/u/{}",
        state.port, owner_slug
    );
    let tunnel_url = state.tunnels.first_url();
    let review_bridge_url = format!(
        "http://127.0.0.1:{}{}",
        state.port,
        crate::web_server::preview::review_bridge_script_href()
    );
    let message = server_preview_message(
        &local_owner_url,
        tunnel_url.as_deref(),
        &owner_slug,
        &share,
        port,
        &review_bridge_url,
    );
    mcp_text(id, &append_preview_warnings(message, warnings))
}

fn server_preview_message(
    local_owner_url: &str,
    tunnel_base: Option<&str>,
    owner_slug: &str,
    share: &common::previews::PreviewShare,
    port: u16,
    review_bridge_url: &str,
) -> String {
    let access = preview_access_message(local_owner_url, tunnel_base, owner_slug, share);
    format!(
        "Preview ready.\n\n\
         {access}\n\
         Port: {port}\n\
         Text and element review bridge (optional, dev-only):\n\
         `<script src=\"{review_bridge_url}\"></script>`"
    )
}

fn preview_access_message(
    local_owner_url: &str,
    tunnel_base: Option<&str>,
    owner_slug: &str,
    share: &common::previews::PreviewShare,
) -> String {
    let remote = match tunnel_base {
        Some(base) => {
            let tunnel_owner = build_preview_url(base, "preview/u", owner_slug);
            let tunnel_share = build_preview_url(base, "preview/s", &share.id);
            let remaining = share
                .expires_at
                .saturating_duration_since(std::time::Instant::now());
            format!(
                "Tunnel owner: `{tunnel_owner}`\n\
                 Tunnel Share: `{tunnel_share}`\n\
                 Access code: `{}`\n\
                 Link and code expire together in {}:{:02}.",
                share.code,
                remaining.as_secs() / 60,
                remaining.as_secs() % 60
            )
        }
        None => "Tunnel owner: unavailable (no tunnel is running)\n\
                 Public sharing is unavailable until a tunnel is running."
            .to_string(),
    };
    format!(
        "Local owner: `{local_owner_url}`\n\
         {remote}"
    )
}

async fn mcp_file_preview(
    id: Option<Value>,
    arguments: &Value,
    metadata: Option<&Value>,
    state: &AppState,
    cwd_path: PathBuf,
    file: &str,
) -> Json<Value> {
    let file_path = match resolve_preview_file(&cwd_path, file) {
        Ok(file_path) => file_path,
        Err(error) => return mcp_error_text(id, &error),
    };

    let title = derive_title(arguments, &file_path);
    let (owner_slug, share) = common::previews::ensure_file(file_path, cwd_path.clone(), title);
    let warnings =
        initialize_preview_conversation(arguments, metadata, &cwd_path, &owner_slug, state).await;
    let _ = state.preview_refresh_tx.send(owner_slug.clone());
    let tunnel_url = state.tunnels.first_url();
    let local_owner_url = format!(
        "http://127.0.0.1:{}/va/preview/u/{}",
        state.port, owner_slug
    );
    let access =
        preview_access_message(&local_owner_url, tunnel_url.as_deref(), &owner_slug, &share);
    let message = format!("Markdown preview ready.\n\n{access}");

    mcp_text(id, &append_preview_warnings(message, warnings))
}

async fn initialize_preview_conversation(
    arguments: &Value,
    metadata: Option<&Value>,
    cwd: &Path,
    owner_slug: &str,
    state: &AppState,
) -> [Option<String>; 2] {
    let request = preview_parent_request(arguments, metadata);
    let (parent_thread_id, parent_warning) =
        match resolve_preview_parent_thread(&request, cwd, state).await {
            Ok(parent_thread_id) => (parent_thread_id, None),
            Err(error) => {
                tracing::warn!(error = %error, "Preview parent task link was skipped");
                (
                    None,
                    Some(format!(
                        "Parent task link was skipped; Preview continues standalone: {error:#}"
                    )),
                )
            }
        };
    let conversation_warning = match state
        .channel_hub
        .workspace_thread_manager()
        .ensure_preview_web_thread(parent_thread_id.as_ref(), owner_slug)
        .await
    {
        Ok(_) => None,
        Err(error) => {
            tracing::warn!(preview_slug = owner_slug, error = %error, "Preview conversation initialization failed");
            Some(format!(
                "Preview conversation is temporarily unavailable: {error:#}"
            ))
        }
    };
    [parent_warning, conversation_warning]
}

fn append_preview_warnings(
    message: String,
    warnings: impl IntoIterator<Item = Option<String>>,
) -> String {
    let warnings = warnings.into_iter().flatten().collect::<Vec<_>>();
    if warnings.is_empty() {
        message
    } else {
        format!("{message}\n\nWarning: {}", warnings.join("\nWarning: "))
    }
}

fn derive_title(arguments: &Value, cwd: &Path) -> String {
    arguments
        .get("title")
        .and_then(Value::as_str)
        .filter(|title| !title.is_empty())
        .map(String::from)
        .unwrap_or_else(|| {
            cwd.file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("Preview")
                .to_string()
        })
}

fn build_preview_url(base: &str, route: &str, slug: &str) -> String {
    format!("{}/va/{}/{}", base.trim_end_matches('/'), route, slug)
}

fn resolve_preview_file(workspace: &Path, file: &str) -> Result<PathBuf, String> {
    let workspace = workspace.canonicalize().map_err(|error| {
        format!(
            "Failed to resolve workspace {}: {error}",
            workspace.display()
        )
    })?;
    let requested = PathBuf::from(file);
    let requested = if requested.is_relative() {
        workspace.join(requested)
    } else {
        requested
    };
    let resolved = requested
        .canonicalize()
        .map_err(|error| format!("File not found: {} ({error})", requested.display()))?;
    if !resolved.is_file() {
        return Err(format!("Path is not a file: {}", resolved.display()));
    }
    Ok(resolved)
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use common::previews::PreviewShare;
    use serde_json::json;

    use super::{
        append_preview_warnings, preview_access_message, preview_source, resolve_preview_file,
        server_preview_message, PreviewSource,
    };

    fn share() -> PreviewShare {
        PreviewShare {
            id: "share-id".to_string(),
            code: "123456".to_string(),
            expires_at: Instant::now() + Duration::from_secs(600),
        }
    }

    #[test]
    fn server_preview_message_includes_tunnel_share_details() {
        let message = server_preview_message(
            "http://127.0.0.1:12358/va/preview/u/owner-slug",
            Some("https://preview.example.com/"),
            "owner-slug",
            &share(),
            5173,
            "http://127.0.0.1:12358/va/preview/assets/review.js",
        );

        assert!(
            message.contains("Tunnel owner: `https://preview.example.com/va/preview/u/owner-slug`")
        );
        assert!(
            message.contains("Tunnel Share: `https://preview.example.com/va/preview/s/share-id`")
        );
        assert!(message.contains("Access code: `123456`"));
        assert!(
            message.contains("Link and code expire together in 9:59")
                || message.contains("Link and code expire together in 10:00")
        );
        assert!(!message.contains("Public sharing is unavailable"));
    }

    #[test]
    fn server_preview_message_explains_when_tunnel_is_unavailable() {
        let message = server_preview_message(
            "http://127.0.0.1:12358/va/preview/u/owner-slug",
            None,
            "owner-slug",
            &share(),
            5173,
            "http://127.0.0.1:12358/va/preview/assets/review.js",
        );

        assert!(message.contains("Tunnel owner: unavailable (no tunnel is running)"));
        assert!(message.contains("Public sharing is unavailable until a tunnel is running."));
        assert!(!message.contains("Tunnel Share:"));
        assert!(!message.contains("Access code:"));
    }

    #[test]
    fn preview_access_contract_always_keeps_the_local_owner() {
        let message = preview_access_message(
            "http://127.0.0.1:12358/va/preview/u/owner-slug",
            Some("https://preview.example.com/"),
            "owner-slug",
            &share(),
        );

        assert!(message.contains("Local owner: `http://127.0.0.1:12358/va/preview/u/owner-slug`"));
        assert!(
            message.contains("Tunnel owner: `https://preview.example.com/va/preview/u/owner-slug`")
        );
        assert!(
            message.contains("Tunnel Share: `https://preview.example.com/va/preview/s/share-id`")
        );
    }

    #[test]
    fn preview_warnings_do_not_replace_success_output() {
        assert_eq!(
            append_preview_warnings("Preview ready.".into(), [None]),
            "Preview ready."
        );
        assert_eq!(
            append_preview_warnings(
                "Preview ready.".into(),
                [
                    Some("Parent link skipped.".into()),
                    Some("Chat unavailable.".into())
                ],
            ),
            "Preview ready.\n\nWarning: Parent link skipped.\nWarning: Chat unavailable."
        );
    }

    #[test]
    fn markdown_preview_allows_external_files_but_requires_resolvable_paths() {
        let root = std::env::temp_dir().join(format!(
            "vibearound-md-preview-paths-{}",
            uuid::Uuid::new_v4()
        ));
        let workspace = root.join("workspace");
        let missing_workspace = root.join("missing-workspace");
        let outside = root.join("outside.md");
        std::fs::create_dir_all(&workspace).unwrap();
        std::fs::write(&outside, "preview").unwrap();

        assert_eq!(
            resolve_preview_file(&workspace, outside.to_str().unwrap()).unwrap(),
            outside.canonicalize().unwrap()
        );
        assert_eq!(
            resolve_preview_file(&workspace, "../outside.md").unwrap(),
            outside.canonicalize().unwrap()
        );
        assert!(
            resolve_preview_file(&missing_workspace, outside.to_str().unwrap())
                .unwrap_err()
                .contains("Failed to resolve workspace")
        );
        assert!(resolve_preview_file(&workspace, "missing.md")
            .unwrap_err()
            .contains("File not found"));

        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn preview_accepts_exactly_one_source() {
        assert_eq!(
            preview_source(&json!({ "port": 5173 })).unwrap(),
            PreviewSource::Server(5173)
        );
        assert_eq!(
            preview_source(&json!({ "file": "README.md" })).unwrap(),
            PreviewSource::File("README.md")
        );
        assert!(preview_source(&json!({})).is_err());
        assert!(preview_source(&json!({ "port": 5173, "file": "README.md" })).is_err());
        assert!(preview_source(&json!({ "port": 0 })).is_err());
        assert!(preview_source(&json!({ "file": "" })).is_err());
    }
}
