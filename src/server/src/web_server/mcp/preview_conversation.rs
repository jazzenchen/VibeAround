use std::path::Path;

use anyhow::anyhow;

use crate::web_server::AppState;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct PreviewParentRequest {
    pub(super) thread_id: Option<String>,
    pub(super) agent_kind: Option<String>,
    pub(super) session_id: Option<String>,
    pub(super) codex_metadata_session_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum PreviewParentIdentity {
    ManagedThread(common::workspace::threads::WorkspaceThreadId),
    ExternalSession {
        agent_id: String,
        session_id: String,
    },
}

impl PreviewParentRequest {
    fn identity(&self) -> Option<PreviewParentIdentity> {
        if let Some(thread_id) = self.thread_id.as_deref() {
            return Some(PreviewParentIdentity::ManagedThread(
                common::workspace::threads::WorkspaceThreadId::from(thread_id),
            ));
        }

        let agent_id = self.agent_kind.clone().or_else(|| {
            self.codex_metadata_session_id
                .as_ref()
                .map(|_| "codex".to_string())
        })?;
        let session_id = if agent_id.eq_ignore_ascii_case("codex") {
            self.codex_metadata_session_id
                .clone()
                .or_else(|| self.session_id.clone())?
        } else {
            self.session_id.clone()?
        };
        Some(PreviewParentIdentity::ExternalSession {
            agent_id,
            session_id,
        })
    }
}

pub(super) async fn resolve_preview_parent_thread(
    request: &PreviewParentRequest,
    cwd: &Path,
    state: &AppState,
) -> anyhow::Result<Option<common::workspace::threads::WorkspaceThreadId>> {
    let Some(identity) = request.identity() else {
        return Ok(None);
    };
    let manager = state.channel_hub.workspace_thread_manager();
    let runtime = match identity {
        PreviewParentIdentity::ManagedThread(thread_id) => {
            manager.runtime_for_thread_id(&thread_id).await?
        }
        PreviewParentIdentity::ExternalSession {
            agent_id,
            session_id,
        } => {
            let agent_id = common::resources::resolve_agent_id(&agent_id).map_err(|error| {
                anyhow!("invalid Preview parent agent '{}': {}", agent_id, error)
            })?;
            manager
                .attach_external_session_to_web_thread(
                    agent_id,
                    None,
                    session_id,
                    cwd.to_path_buf(),
                    common::workspace::manager::ExternalSessionAttachMode::ReuseOpenThread,
                )
                .await?
        }
    };
    let runtime_state = runtime.state().await;
    let expected_workspace = common::workspace::normalize_workspace_cwd(cwd.to_path_buf());
    if runtime_state.workspace != expected_workspace {
        return Err(anyhow!(
            "parent task workspace {} does not match Preview workspace {}",
            runtime_state.workspace.display(),
            expected_workspace.display()
        ));
    }
    Ok(Some(runtime_state.thread_id))
}

pub(super) async fn ensure_preview_child_thread(
    owner_slug: &str,
    parent_thread_id: Option<common::workspace::threads::WorkspaceThreadId>,
    state: &AppState,
) -> anyhow::Result<()> {
    let manager = state.channel_hub.workspace_thread_manager();
    if let Some(existing_child_id) = common::previews::owner_conversation_thread_id(owner_slug) {
        let existing_parent_id = manager
            .parent_thread_id_for_thread(&existing_child_id)
            .await?;
        return match parent_thread_id {
            None => Ok(()),
            Some(parent_thread_id) if existing_parent_id.as_ref() == Some(&parent_thread_id) => {
                Ok(())
            }
            Some(parent_thread_id) => Err(anyhow!(
                "Preview is already linked to parent task {}; delete it before linking parent task {}",
                existing_parent_id
                    .map(|thread_id| thread_id.to_string())
                    .unwrap_or_else(|| "<none>".to_string()),
                parent_thread_id
            )),
        };
    }

    let Some(parent_thread_id) = parent_thread_id else {
        return Ok(());
    };
    let child_runtime = manager.create_child_web_thread(&parent_thread_id).await?;
    let child_thread_id = child_runtime.state().await.thread_id;
    common::previews::bind_owner_conversation(owner_slug, child_thread_id).map_err(|error| {
        match error {
            common::previews::PreviewConversationBindError::NotFound => {
                anyhow!("Preview {} no longer exists", owner_slug)
            }
            common::previews::PreviewConversationBindError::Conflict { existing_thread_id } => {
                anyhow!(
                    "Preview was concurrently linked to child task {}",
                    existing_thread_id
                )
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn managed_thread_takes_priority_over_external_identity() {
        let request = PreviewParentRequest {
            thread_id: Some("wt_managed".to_string()),
            agent_kind: Some("claude".to_string()),
            session_id: Some("external-session".to_string()),
            codex_metadata_session_id: None,
        };

        assert_eq!(
            request.identity(),
            Some(PreviewParentIdentity::ManagedThread(
                common::workspace::threads::WorkspaceThreadId::from("wt_managed")
            ))
        );
    }

    #[test]
    fn exact_codex_metadata_identifies_an_external_session() {
        let request = PreviewParentRequest {
            agent_kind: Some("codex".to_string()),
            session_id: Some("stale-explicit-session".to_string()),
            codex_metadata_session_id: Some("codex-native-session".to_string()),
            ..PreviewParentRequest::default()
        };

        assert_eq!(
            request.identity(),
            Some(PreviewParentIdentity::ExternalSession {
                agent_id: "codex".to_string(),
                session_id: "codex-native-session".to_string()
            })
        );
    }

    #[test]
    fn an_external_session_requires_an_explicit_agent() {
        let ambiguous = PreviewParentRequest {
            session_id: Some("ambiguous-session".to_string()),
            ..PreviewParentRequest::default()
        };
        assert_eq!(ambiguous.identity(), None);

        let exact = PreviewParentRequest {
            agent_kind: Some("claude".to_string()),
            session_id: Some("claude-native-session".to_string()),
            ..PreviewParentRequest::default()
        };
        assert_eq!(
            exact.identity(),
            Some(PreviewParentIdentity::ExternalSession {
                agent_id: "claude".to_string(),
                session_id: "claude-native-session".to_string()
            })
        );
    }
}
