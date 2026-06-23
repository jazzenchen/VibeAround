use std::time::Instant;

use va_client::launcher::LauncherPreferencesResponse;
use va_client::{ops, Operation};

use super::TuiApp;
use crate::selection::AgentPanel;
use crate::transport::HttpTransport;

impl TuiApp {
    pub(crate) async fn sync_agent_picker_selection(&mut self, transport: &HttpTransport) {
        let operation = match self.launcher_preference_operation_for_selection() {
            Ok(Some(operation)) => operation,
            Ok(None) => return,
            Err(error) => {
                self.last_error = Some(error);
                return;
            }
        };

        match transport.execute(operation).await {
            Ok(preferences) => {
                self.agent_picker.preferences = Some(preferences);
                self.last_error = None;
                self.last_refresh = Some(Instant::now());
            }
            Err(error) => {
                self.last_error = Some(error.to_string());
            }
        }
    }

    pub(crate) fn launcher_preference_operation_for_selection(
        &self,
    ) -> Result<Option<Operation<LauncherPreferencesResponse>>, String> {
        match self.agent_selection.panel {
            AgentPanel::Agents => {
                let Some(agent_id) = self.selected_agent.as_deref() else {
                    return Ok(None);
                };
                ops::launcher_set_selected_agent(agent_id)
                    .map(Some)
                    .map_err(|error| error.to_string())
            }
            AgentPanel::Profiles => {
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
            AgentPanel::Workspaces => {
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
            AgentPanel::Sessions => Ok(None),
        }
    }

    pub(super) fn select_agent_picker_item(&mut self) {
        match self.agent_selection.panel {
            AgentPanel::Agents => {
                if let Some(agent) = self.agent_selection.selected_agent(&self.agent_picker) {
                    if self.selected_agent.as_deref() != Some(agent.id.as_str()) {
                        self.selected_profile = None;
                        self.selected_workspace = None;
                        self.selected_session = None;
                    }
                    self.selected_agent = Some(agent.id.clone());
                    self.last_action = Some(format!("selected agent {}", agent.id));
                }
            }
            AgentPanel::Profiles => {
                if let Some(profile) = self.agent_selection.selected_profile(&self.agent_picker) {
                    self.selected_profile = Some(profile.id.clone());
                    self.selected_session = None;
                    self.last_action = Some(format!("selected profile {}", profile.label));
                }
            }
            AgentPanel::Workspaces => {
                if let Some(workspace) = self.agent_selection.selected_workspace(&self.agent_picker)
                {
                    self.selected_workspace = Some(workspace.path.clone());
                    self.selected_session = None;
                    self.last_action = Some(format!("selected workspace {}", workspace.path));
                }
            }
            AgentPanel::Sessions => {
                if let Some(session) = self.agent_selection.selected_session(&self.agent_picker) {
                    self.selected_agent = Some(session.agent_id.clone());
                    self.selected_profile = None;
                    self.selected_workspace = Some(session.workspace.clone());
                    self.selected_session = Some(session.session_id.clone());
                    self.last_action = Some(format!("selected session {}", session.short_id));
                }
            }
        }
    }
}
