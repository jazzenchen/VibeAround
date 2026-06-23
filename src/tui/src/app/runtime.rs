use std::time::Instant;

use super::TuiApp;
use crate::runtime_socket::RuntimeSocketEvent;

impl TuiApp {
    pub(crate) fn apply_runtime_socket_event(&mut self, event: RuntimeSocketEvent) {
        match event {
            RuntimeSocketEvent::Channels(channels) => {
                self.snapshot.channels = channels;
                self.status_selection.clamp(&self.snapshot);
                self.last_error = None;
                self.last_refresh = Some(Instant::now());
            }
            RuntimeSocketEvent::Tunnels(tunnels) => {
                self.snapshot.tunnels = tunnels;
                self.status_selection.clamp(&self.snapshot);
                self.last_error = None;
                self.last_refresh = Some(Instant::now());
            }
            RuntimeSocketEvent::Agents(agents) => {
                self.snapshot.agents = agents;
                self.status_selection.clamp(&self.snapshot);
                self.last_error = None;
                self.last_refresh = Some(Instant::now());
            }
            RuntimeSocketEvent::Sessions(sessions) => {
                self.snapshot.sessions = sessions;
                self.agent_picker.sessions = self.snapshot.sessions.clone();
                if self.selected_session.as_deref().is_some_and(|session_id| {
                    !self
                        .snapshot
                        .sessions
                        .iter()
                        .any(|session| session.session_id == session_id)
                }) {
                    self.selected_session = None;
                }
                self.status_selection.clamp(&self.snapshot);
                self.agent_selection.clamp(&self.agent_picker);
                self.last_error = None;
                self.last_refresh = Some(Instant::now());
            }
            RuntimeSocketEvent::Error(error) => {
                self.last_error = Some(error);
            }
        }
    }
}
