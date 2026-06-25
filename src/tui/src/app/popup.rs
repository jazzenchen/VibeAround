use std::time::Instant;

use va_client::launcher::LauncherPreferencesResponse;
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
                1 => self.agent_picker.profiles.len(),
                2 => self.agent_picker.workspaces.len(),
                3 => self.agent_picker.sessions.len(),
                _ => 0,
            },
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
        let Some((kind, level, cursor)) =
            self.popup.as_ref().map(|p| (p.kind, p.level, p.cursor))
        else {
            return;
        };
        match (kind, level) {
            (_, PopupLevel::Categories) => {
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
                self.popup = None;
            }
            (_, PopupLevel::Detail { .. }) => {}
        }
    }

    fn clamp_popup_cursor(&mut self) {
        let len = self.popup_list_len();
        if let Some(popup) = &mut self.popup {
            popup.clamp(len);
        }
    }

    fn apply_agent_popup_selection(&mut self, category: usize, item: usize) {
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
                if let Some(profile) = self.agent_picker.profiles.get(item) {
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
                if let Some(session) = self.agent_picker.sessions.get(item) {
                    self.selected_agent = Some(session.agent_id.clone());
                    self.selected_profile = None;
                    self.selected_workspace = Some(session.workspace.clone());
                    self.selected_session = Some(session.session_id.clone());
                    self.last_action = Some(format!("selected session {}", session.short_id));
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

    fn agent_popup_pref_operation(
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
                let Some(profile_id) = self.selected_profile.as_deref() else {
                    return Ok(None);
                };
                let Some(agent_id) = self.effective_agent() else {
                    return Err("select an agent before choosing a profile".into());
                };
                ops::launcher_set_agent_profile(agent_id, Some(profile_id))
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
