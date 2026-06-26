use std::time::Instant;

use va_client::launcher::LauncherPreferencesResponse;
use va_client::sessions::LaunchSessionInfo;
use va_client::{ops, Operation};

use super::{ErrorScope, TuiApp};
use crate::popup::{Popup, PopupKind, PopupLevel};
use crate::transport::HttpTransport;

impl TuiApp {
    pub(crate) async fn open_status_popup(&mut self, transport: &HttpTransport) {
        self.popup = Some(Popup::new(PopupKind::Status));
        self.refresh_status(transport).await;
        self.clamp_popup_cursor();
    }

    pub(crate) async fn open_agent_popup(&mut self, transport: &HttpTransport) {
        self.popup = Some(Popup::new(PopupKind::Agent));
        self.refresh_agent_picker(transport).await;
        self.clamp_popup_cursor();
    }

    pub(crate) fn popup_is_open(&self) -> bool {
        self.popup.is_some()
    }

    /// Rows in the popup's current level — categories, a category's items, or
    /// zero while showing a leaf detail.
    pub(crate) fn popup_list_len(&self) -> usize {
        let Some(popup) = &self.popup else {
            return 0;
        };
        match popup.level {
            PopupLevel::Categories => popup.kind.categories().len(),
            PopupLevel::Items { category } => self.popup_item_count(popup.kind, category),
            PopupLevel::Detail { .. } => 0,
        }
    }

    pub(crate) fn popup_item_count(&self, kind: PopupKind, category: usize) -> usize {
        match kind {
            PopupKind::Status => match category {
                0 => self.snapshot.channels.len(),
                1 => self.snapshot.tunnels.len(),
                2 => self.snapshot.agents.len(),
                3 => self.snapshot.sessions.len(),
                _ => 0,
            },
            PopupKind::Agent => match category {
                0 => self.agent_picker.agents.len(),
                // +1 for the synthetic "direct" (no managed profile) entry.
                1 => self.agent_picker.profiles.len() + 1,
                2 => self.agent_picker.workspaces.len(),
                // +1 for the synthetic "new session" entry at the top.
                3 => self.agent_session_items().len() + 1,
                _ => 0,
            },
        }
    }

    /// Sessions for the agent popup, filtered to the agent currently in
    /// context so the list isn't polluted by other agents' sessions.
    pub(crate) fn agent_session_items(&self) -> Vec<&LaunchSessionInfo> {
        let agent = self.effective_agent();
        self.agent_picker
            .sessions
            .iter()
            .filter(|session| agent.is_none_or(|id| session.agent_id == id))
            .collect()
    }

    /// The current selection shown next to each agent category, so the
    /// categories list doubles as a config summary.
    pub(crate) fn agent_category_value(&self, category: usize) -> String {
        let short = |id: &str| id.chars().take(8).collect::<String>();
        match category {
            0 => self.effective_agent().unwrap_or("—").to_string(),
            // No managed profile reads as "direct", not unset.
            1 => self.effective_profile().unwrap_or("direct").to_string(),
            2 => self.effective_workspace().unwrap_or("—").to_string(),
            3 => self
                .effective_session()
                .map(short)
                .unwrap_or_else(|| "new".to_string()),
            _ => String::new(),
        }
    }

    /// Whether the runtime agent at `index` is the host running the current
    /// chat (matched by session id), so the status list flags "you are here".
    pub(crate) fn status_agent_is_current(&self, index: usize) -> bool {
        let Some(current) = self
            .effective_session()
            .or(self.chat_state.session_id.as_deref())
        else {
            return false;
        };
        self.snapshot
            .agents
            .get(index)
            .and_then(|agent| agent.session_id.as_deref())
            == Some(current)
    }

    /// Whether the item at `index` in an agent category is the one currently in
    /// context (gets the `●` marker). Index 0 of sessions is the "new" entry.
    pub(crate) fn agent_item_is_effective(&self, category: usize, index: usize) -> bool {
        match category {
            0 => self
                .agent_picker
                .agents
                .get(index)
                .is_some_and(|agent| self.effective_agent() == Some(agent.id.as_str())),
            1 => {
                if index == 0 {
                    self.effective_profile().is_none()
                } else {
                    self.agent_picker
                        .profiles
                        .get(index - 1)
                        .is_some_and(|profile| {
                            self.effective_profile() == Some(profile.id.as_str())
                        })
                }
            }
            2 => self
                .agent_picker
                .workspaces
                .get(index)
                .is_some_and(|workspace| {
                    self.effective_workspace() == Some(workspace.path.as_str())
                }),
            3 => {
                if index == 0 {
                    self.effective_session().is_none()
                } else {
                    self.agent_session_items()
                        .get(index - 1)
                        .is_some_and(|session| {
                            self.effective_session() == Some(session.session_id.as_str())
                        })
                }
            }
            _ => false,
        }
    }

    pub(crate) fn popup_move_up(&mut self) {
        let len = self.popup_list_len();
        if let Some(popup) = &mut self.popup {
            popup.move_up(len);
        }
    }

    pub(crate) fn popup_move_down(&mut self) {
        let len = self.popup_list_len();
        if let Some(popup) = &mut self.popup {
            popup.move_down(len);
        }
    }

    pub(crate) fn popup_back(&mut self) {
        if let Some(popup) = &mut self.popup {
            if popup.back() {
                self.popup = None;
            } else {
                self.clamp_popup_cursor();
            }
        }
    }

    /// Enter drills one level deeper; on an agent item it sets the chat context
    /// and closes, on a status item it opens the read-only detail.
    pub(crate) async fn popup_enter(&mut self, transport: &HttpTransport) {
        let Some((kind, level, cursor)) = self.popup.as_ref().map(|p| (p.kind, p.level, p.cursor))
        else {
            return;
        };
        match (kind, level) {
            (_, PopupLevel::Categories) => {
                if kind.is_close_category(cursor) {
                    self.popup = None;
                    return;
                }
                if let Some(popup) = &mut self.popup {
                    popup.open_category();
                }
                self.clamp_popup_cursor();
            }
            (PopupKind::Status, PopupLevel::Items { .. }) => {
                if self.popup_list_len() > 0 {
                    if let Some(popup) = &mut self.popup {
                        popup.open_detail();
                    }
                }
            }
            (PopupKind::Agent, PopupLevel::Items { category }) => {
                self.apply_agent_popup_selection(category, cursor);
                self.sync_agent_popup_selection(category, transport).await;
                if let Some(popup) = &mut self.popup {
                    popup.level = PopupLevel::Categories;
                    popup.cursor = category;
                }
                self.clamp_popup_cursor();
            }
            (_, PopupLevel::Detail { .. }) => {}
        }
    }

    pub(crate) fn clamp_popup_cursor(&mut self) {
        let len = self.popup_list_len();
        if let Some(popup) = &mut self.popup {
            popup.clamp(len);
        }
    }

    pub(crate) fn apply_agent_popup_selection(&mut self, category: usize, item: usize) {
        match category {
            0 => {
                if let Some(agent) = self.agent_picker.agents.get(item) {
                    if self.selected_agent.as_deref() != Some(agent.id.as_str()) {
                        self.selected_profile = None;
                        self.selected_workspace = None;
                        self.selected_session = None;
                    }
                    self.selected_agent = Some(agent.id.clone());
                    self.last_action = Some(format!("selected agent {}", agent.id));
                }
            }
            1 => {
                if item == 0 {
                    // "direct": no managed profile.
                    self.selected_profile = None;
                    self.selected_session = None;
                    self.last_action = Some("direct profile".to_string());
                } else if let Some(profile) = self.agent_picker.profiles.get(item - 1) {
                    self.selected_profile = Some(profile.id.clone());
                    self.selected_session = None;
                    self.last_action = Some(format!("selected profile {}", profile.label));
                }
            }
            2 => {
                if let Some(workspace) = self.agent_picker.workspaces.get(item) {
                    self.selected_workspace = Some(workspace.path.clone());
                    self.selected_session = None;
                    self.last_action = Some(format!("selected workspace {}", workspace.path));
                }
            }
            3 => {
                if item == 0 {
                    // "new": keep the current agent/profile/workspace, drop the
                    // bound session so the next message starts fresh.
                    self.selected_session = None;
                    self.force_new_session = true;
                    self.last_action = Some("new session".to_string());
                } else if let Some(session) =
                    self.agent_session_items().get(item - 1).map(|session| {
                        (
                            session.agent_id.clone(),
                            session.session_id.clone(),
                            session.workspace.clone(),
                            session.short_id.clone(),
                        )
                    })
                {
                    let (agent_id, session_id, workspace, short_id) = session;
                    self.selected_agent = Some(agent_id);
                    self.selected_profile = None;
                    self.selected_workspace = Some(workspace);
                    self.selected_session = Some(session_id);
                    self.force_new_session = false;
                    self.last_action = Some(format!("selected session {short_id}"));
                }
            }
            _ => {}
        }
    }

    async fn sync_agent_popup_selection(&mut self, category: usize, transport: &HttpTransport) {
        let operation = match self.agent_popup_pref_operation(category) {
            Ok(Some(operation)) => operation,
            Ok(None) => return,
            Err(error) => {
                self.set_error(ErrorScope::Agent, error);
                return;
            }
        };
        match transport.execute(operation).await {
            Ok(preferences) => {
                self.agent_picker.preferences = Some(preferences);
                self.clear_error(ErrorScope::Agent);
                self.last_refresh = Some(Instant::now());
            }
            Err(error) => self.set_error(ErrorScope::Agent, error.to_string()),
        }
    }

    pub(crate) fn agent_popup_pref_operation(
        &self,
        category: usize,
    ) -> Result<Option<Operation<LauncherPreferencesResponse>>, String> {
        match category {
            0 => {
                let Some(agent_id) = self.selected_agent.as_deref() else {
                    return Ok(None);
                };
                ops::launcher_set_selected_agent(agent_id)
                    .map(Some)
                    .map_err(|error| error.to_string())
            }
            1 => {
                let Some(agent_id) = self.effective_agent() else {
                    return Err("select an agent before choosing a profile".into());
                };
                // `None` persists "direct" — clearing the managed profile.
                ops::launcher_set_agent_profile(agent_id, self.selected_profile.as_deref())
                    .map(Some)
                    .map_err(|error| error.to_string())
            }
            2 => {
                let Some(workspace) = self.selected_workspace.as_deref() else {
                    return Ok(None);
                };
                let Some(agent_id) = self.effective_agent() else {
                    return Err("select an agent before choosing a workspace".into());
                };
                ops::launcher_set_agent_workspace(agent_id, workspace)
                    .map(Some)
                    .map_err(|error| error.to_string())
            }
            _ => Ok(None),
        }
    }
}
