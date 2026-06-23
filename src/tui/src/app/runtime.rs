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
                self.status_selection.clamp(&self.snapshot);
                self.last_error = None;
                self.last_refresh = Some(Instant::now());
            }
            RuntimeSocketEvent::Error(error) => {
                self.last_error = Some(error);
            }
        }
    }
}
