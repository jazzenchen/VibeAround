use std::path::{Path, PathBuf};

use axum::Json;
use serde_json::Value;

use crate::web_server::AppState;

use super::jsonrpc::{jsonrpc_err, mcp_error_text, mcp_text};
use super::ports::is_denied_port;
use super::preview_conversation::{
    ensure_preview_conversation_thread, resolve_preview_parent_thread, PreviewParentRequest,
};
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

pub(super) async fn mcp_preview_start(
    id: Option<Value>,
    arguments: &Value,
    metadata: Option<&Value>,
    state: &AppState,
) -> Json<Value> {
    let port = match arguments.get("port").and_then(Value::as_u64) {
        Some(port) if port > 0 && port <= u16::MAX as u64 => port as u16,
        _ => {
            return jsonrpc_err(
                id,
                -32602,
                "Missing or invalid required argument: port (1-65535)",
            );
        }
    };

    if is_denied_port(port) {
        return mcp_error_text(
            id,
            &format!(
                "Port {} is a well-known service port and cannot be previewed for security reasons. \
                 Use a typical dev server port (e.g. 3000, 5173, 8080).",
                port
            ),
        );
    }

    let cwd = match arguments.get("cwd").and_then(Value::as_str) {
        Some(cwd) => cwd,
        None => return jsonrpc_err(id, -32602, "Missing required argument: cwd"),
    };
    let cwd_path = PathBuf::from(cwd);
    if let Err(response) = validate_workspace(&cwd_path, id.clone()) {
        return response;
    }

    let parent_request = preview_parent_request(arguments, metadata);
    let parent_thread_id =
        match resolve_preview_parent_thread(&parent_request, &cwd_path, state).await {
            Ok(parent_thread_id) => parent_thread_id,
            Err(error) => {
                return mcp_error_text(
                    id,
                    &format!("Failed to resolve Preview parent task: {error:#}"),
                );
            }
        };

    let title = derive_title(arguments, &cwd_path);
    let owner_session = parent_request.owner_session_id();
    let (owner_slug, share) = common::previews::ensure_server(port, cwd_path, title, owner_session);
    if let Err(error) =
        ensure_preview_conversation_thread(&owner_slug, parent_thread_id, state).await
    {
        return mcp_error_text(
            id,
            &format!("Failed to initialize Preview conversation: {error:#}"),
        );
    }
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
    mcp_text(
        id,
        &server_preview_message(
            &local_owner_url,
            tunnel_url.as_deref(),
            &owner_slug,
            &share,
            port,
            &review_bridge_url,
        ),
    )
}

fn server_preview_message(
    local_owner_url: &str,
    tunnel_base: Option<&str>,
    owner_slug: &str,
    share: &common::previews::PreviewShare,
    port: u16,
    review_bridge_url: &str,
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
        "Preview ready.\n\n\
         Local owner: `{local_owner_url}`\n\
         {remote}\n\
         Port: {port}\n\
         Text and element review bridge (optional, dev-only):\n\
         `<script src=\"{review_bridge_url}\"></script>`"
    )
}

pub(super) async fn mcp_md_preview(
    id: Option<Value>,
    arguments: &Value,
    metadata: Option<&Value>,
    state: &AppState,
) -> Json<Value> {
    let file = match arguments.get("file").and_then(Value::as_str) {
        Some(file) => file,
        None => return jsonrpc_err(id, -32602, "Missing required argument: file"),
    };
    let cwd = match arguments.get("cwd").and_then(Value::as_str) {
        Some(cwd) => cwd,
        None => return jsonrpc_err(id, -32602, "Missing required argument: cwd"),
    };

    let cwd_path = PathBuf::from(cwd);
    if let Err(response) = validate_workspace(&cwd_path, id.clone()) {
        return response;
    }

    let file_path = match resolve_md_preview_file(&cwd_path, file) {
        Ok(file_path) => file_path,
        Err(error) => return mcp_error_text(id, &error),
    };

    let title = arguments
        .get("title")
        .and_then(Value::as_str)
        .filter(|title| !title.is_empty())
        .map(String::from)
        .unwrap_or_else(|| {
            file_path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("Preview")
                .to_string()
        });
    let parent_request = preview_parent_request(arguments, metadata);
    let parent_thread_id =
        match resolve_preview_parent_thread(&parent_request, &cwd_path, state).await {
            Ok(parent_thread_id) => parent_thread_id,
            Err(error) => {
                return mcp_error_text(
                    id,
                    &format!("Failed to resolve Preview parent task: {error:#}"),
                );
            }
        };

    let (owner_slug, share) = common::previews::ensure_file(file_path, cwd_path, title);
    if let Err(error) =
        ensure_preview_conversation_thread(&owner_slug, parent_thread_id, state).await
    {
        return mcp_error_text(
            id,
            &format!("Failed to initialize Preview conversation: {error:#}"),
        );
    }
    let tunnel_url = state.tunnels.first_url();
    let owner_base = tunnel_url
        .clone()
        .unwrap_or_else(|| format!("http://127.0.0.1:{}", state.port));
    let owner_url = build_preview_url(&owner_base, "preview/u", &owner_slug);
    let message = match tunnel_url {
        Some(base) => {
            let share_url = build_preview_url(&base, "preview/s", &share.id);
            let remaining = share
                .expires_at
                .saturating_duration_since(std::time::Instant::now());
            format!(
                "Markdown preview ready.\n\n\
                 Owner: `{owner_url}`\n\
                 Share: `{share_url}`\n\
                 Access code: `{}`\n\
                 Link and code expire together in {}:{:02}.",
                share.code,
                remaining.as_secs() / 60,
                remaining.as_secs() % 60
            )
        }
        None => format!(
            "Markdown preview ready.\n\n\
             Owner: `{owner_url}`\n\
             Public sharing is unavailable until a tunnel is running."
        ),
    };

    mcp_text(id, &message)
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

fn resolve_md_preview_file(workspace: &Path, file: &str) -> Result<PathBuf, String> {
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

    use super::{resolve_md_preview_file, server_preview_message};

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
            resolve_md_preview_file(&workspace, outside.to_str().unwrap()).unwrap(),
            outside.canonicalize().unwrap()
        );
        assert_eq!(
            resolve_md_preview_file(&workspace, "../outside.md").unwrap(),
            outside.canonicalize().unwrap()
        );
        assert!(
            resolve_md_preview_file(&missing_workspace, outside.to_str().unwrap())
                .unwrap_err()
                .contains("Failed to resolve workspace")
        );
        assert!(resolve_md_preview_file(&workspace, "missing.md")
            .unwrap_err()
            .contains("File not found"));

        std::fs::remove_dir_all(root).unwrap();
    }
}
