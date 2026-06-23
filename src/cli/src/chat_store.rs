use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::args::{ChatForgetArgs, ChatSendArgs, Options};
use crate::config::{chat_sessions_path, set_owner_only};
use crate::error::CliError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ChatSessionScope {
    pub(crate) workspace: String,
    pub(crate) agent: Option<String>,
    pub(crate) profile_id: Option<String>,
}

impl ChatSessionScope {
    pub(crate) fn display(&self) -> String {
        let agent = self.agent.as_deref().unwrap_or("default-agent");
        let profile = self.profile_id.as_deref().unwrap_or("default-profile");
        format!(
            "workspace={}, agent={}, profile={}",
            self.workspace, agent, profile
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct ChatSessionEntry {
    workspace: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    agent: Option<String>,
    #[serde(rename = "profileId", skip_serializing_if = "Option::is_none")]
    profile_id: Option<String>,
    #[serde(rename = "sessionId")]
    session_id: String,
    #[serde(rename = "updatedAtMs")]
    updated_at_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct StoredChatSession {
    pub(crate) workspace: String,
    pub(crate) agent: Option<String>,
    pub(crate) profile_id: Option<String>,
    pub(crate) session_id: String,
    pub(crate) updated_at_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
struct ChatSessionStore {
    version: u8,
    entries: Vec<ChatSessionEntry>,
}

impl Default for ChatSessionStore {
    fn default() -> Self {
        Self {
            version: 1,
            entries: Vec::new(),
        }
    }
}

pub(crate) fn scope_for_args(args: &ChatSendArgs) -> Result<ChatSessionScope, CliError> {
    scope_for_parts(
        args.workspace_path.as_deref(),
        args.agent.as_deref(),
        args.profile_id.as_deref(),
    )
}

pub(crate) fn scope_for_forget_args(args: &ChatForgetArgs) -> Result<ChatSessionScope, CliError> {
    scope_for_parts(
        args.workspace_path.as_deref(),
        args.agent.as_deref(),
        args.profile_id.as_deref(),
    )
}

fn scope_for_parts(
    workspace_path: Option<&str>,
    agent: Option<&str>,
    profile_id: Option<&str>,
) -> Result<ChatSessionScope, CliError> {
    Ok(ChatSessionScope {
        workspace: workspace_key(workspace_path)?,
        agent: trimmed(agent),
        profile_id: trimmed(profile_id),
    })
}

pub(crate) fn list_sessions(options: &Options) -> Result<Vec<StoredChatSession>, CliError> {
    let mut sessions = ChatSessionStore::load(&chat_sessions_path(options))?
        .entries
        .into_iter()
        .map(StoredChatSession::from)
        .collect::<Vec<_>>();
    sessions.sort_by(|left, right| right.updated_at_ms.cmp(&left.updated_at_ms));
    Ok(sessions)
}

pub(crate) fn saved_session_for(
    options: &Options,
    scope: &ChatSessionScope,
) -> Result<Option<String>, CliError> {
    let store = ChatSessionStore::load(&chat_sessions_path(options))?;
    Ok(store.find(scope).map(|entry| entry.session_id.clone()))
}

pub(crate) fn save_session_for(
    options: &Options,
    scope: &ChatSessionScope,
    session_id: &str,
) -> Result<(), CliError> {
    let path = chat_sessions_path(options);
    let mut store = ChatSessionStore::load(&path)?;
    store.upsert(scope, session_id);
    store.save(&path)
}

pub(crate) fn forget_session_for(
    options: &Options,
    scope: &ChatSessionScope,
) -> Result<bool, CliError> {
    let path = chat_sessions_path(options);
    let mut store = ChatSessionStore::load(&path)?;
    let removed = store.remove(scope);
    if removed {
        store.save(&path)?;
    }
    Ok(removed)
}

pub(crate) fn clear_sessions(options: &Options) -> Result<usize, CliError> {
    let path = chat_sessions_path(options);
    let store = ChatSessionStore::load(&path)?;
    match fs::remove_file(&path) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(source) => {
            return Err(CliError::Io {
                action: "removing chat session store",
                source,
            });
        }
    }
    Ok(store.entries.len())
}

fn workspace_key(workspace: Option<&str>) -> Result<String, CliError> {
    let path = match trimmed(workspace) {
        Some(path) => PathBuf::from(path),
        None => std::env::current_dir().map_err(|source| CliError::Io {
            action: "resolving current workspace",
            source,
        })?,
    };
    let absolute = if path.is_absolute() {
        path
    } else {
        std::env::current_dir()
            .map_err(|source| CliError::Io {
                action: "resolving current workspace",
                source,
            })?
            .join(path)
    };
    Ok(absolute.to_string_lossy().into_owned())
}

fn trimmed(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

impl ChatSessionStore {
    fn load(path: &Path) -> Result<Self, CliError> {
        let body = match fs::read_to_string(path) {
            Ok(body) => body,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Self::default());
            }
            Err(source) => {
                return Err(CliError::Io {
                    action: "reading chat session store",
                    source,
                });
            }
        };
        if body.trim().is_empty() {
            return Ok(Self::default());
        }
        serde_json::from_str(&body).map_err(CliError::from)
    }

    fn save(&self, path: &Path) -> Result<(), CliError> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|source| CliError::Io {
                action: "creating chat session store directory",
                source,
            })?;
        }
        let body = serde_json::to_string_pretty(self)?;
        fs::write(path, body).map_err(|source| CliError::Io {
            action: "writing chat session store",
            source,
        })?;
        set_owner_only(path).map_err(|source| CliError::Io {
            action: "securing chat session store",
            source,
        })?;
        Ok(())
    }

    fn find(&self, scope: &ChatSessionScope) -> Option<&ChatSessionEntry> {
        self.entries.iter().find(|entry| entry.matches(scope))
    }

    fn upsert(&mut self, scope: &ChatSessionScope, session_id: &str) {
        let updated_at_ms = current_time_ms();
        if let Some(entry) = self.entries.iter_mut().find(|entry| entry.matches(scope)) {
            entry.session_id = session_id.to_string();
            entry.updated_at_ms = updated_at_ms;
            return;
        }
        self.entries.push(ChatSessionEntry {
            workspace: scope.workspace.clone(),
            agent: scope.agent.clone(),
            profile_id: scope.profile_id.clone(),
            session_id: session_id.to_string(),
            updated_at_ms,
        });
    }

    fn remove(&mut self, scope: &ChatSessionScope) -> bool {
        let before = self.entries.len();
        self.entries.retain(|entry| !entry.matches(scope));
        self.entries.len() != before
    }
}

impl ChatSessionEntry {
    fn matches(&self, scope: &ChatSessionScope) -> bool {
        self.workspace == scope.workspace
            && self.agent == scope.agent
            && self.profile_id == scope.profile_id
    }
}

impl From<ChatSessionEntry> for StoredChatSession {
    fn from(entry: ChatSessionEntry) -> Self {
        Self {
            workspace: entry.workspace,
            agent: entry.agent,
            profile_id: entry.profile_id,
            session_id: entry.session_id,
            updated_at_ms: entry.updated_at_ms,
        }
    }
}

fn current_time_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis().min(u128::from(u64::MAX)) as u64)
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn store_round_trips_scoped_session() {
        let path = std::env::temp_dir().join(format!(
            "va-cli-chat-store-test-{}-{}.json",
            std::process::id(),
            line!()
        ));
        let _ = fs::remove_file(&path);
        let scope = ChatSessionScope {
            workspace: "/tmp/project".into(),
            agent: Some("codex".into()),
            profile_id: Some("default".into()),
        };

        let mut store = ChatSessionStore::default();
        store.upsert(&scope, "session-1");
        store.save(&path).expect("save");

        let loaded = ChatSessionStore::load(&path).expect("load");
        assert_eq!(
            loaded.find(&scope).map(|entry| entry.session_id.as_str()),
            Some("session-1")
        );

        let _ = fs::remove_file(&path);
    }

    #[test]
    fn store_updates_existing_scope() {
        let scope = ChatSessionScope {
            workspace: "/tmp/project".into(),
            agent: None,
            profile_id: None,
        };
        let mut store = ChatSessionStore::default();

        store.upsert(&scope, "session-1");
        store.upsert(&scope, "session-2");

        assert_eq!(store.entries.len(), 1);
        assert_eq!(store.entries[0].session_id, "session-2");
    }

    #[test]
    fn store_removes_matching_scope_only() {
        let scope = ChatSessionScope {
            workspace: "/tmp/project".into(),
            agent: None,
            profile_id: None,
        };
        let other = ChatSessionScope {
            workspace: "/tmp/project".into(),
            agent: Some("codex".into()),
            profile_id: None,
        };
        let mut store = ChatSessionStore::default();
        store.upsert(&scope, "session-1");
        store.upsert(&other, "session-2");

        assert!(store.remove(&scope));
        assert_eq!(store.entries.len(), 1);
        assert_eq!(store.entries[0].session_id, "session-2");
        assert!(!store.remove(&scope));
    }

    #[test]
    fn list_and_clear_sessions_use_options_path() {
        let root = std::env::temp_dir().join(format!(
            "va-cli-chat-store-options-test-{}-{}",
            std::process::id(),
            line!()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("dir");
        let options = Options {
            auth_file: Some(root.join("auth.json")),
            ..Default::default()
        };
        let scope = ChatSessionScope {
            workspace: "/tmp/project".into(),
            agent: None,
            profile_id: None,
        };

        save_session_for(&options, &scope, "session-1").expect("save");
        assert_eq!(list_sessions(&options).expect("list").len(), 1);
        assert_eq!(clear_sessions(&options).expect("clear"), 1);
        assert!(list_sessions(&options).expect("list").is_empty());

        let _ = fs::remove_dir_all(&root);
    }
}
