use std::time::Instant;

use super::{AppView, TuiApp};
use crate::runtime_socket::RuntimeSocketEvent;

impl TuiApp {
    pub(crate) fn apply_runtime_socket_event(&mut self, event: RuntimeSocketEvent) {
        match event {
            RuntimeSocketEvent::Channels(channels) => {
                self.snapshot.channels = channels;
                self.status_selection.clamp(&self.snapshot);
                self.sync_status_detail();
                self.last_error = None;
                self.last_refresh = Some(Instant::now());
            }
            RuntimeSocketEvent::Tunnels(tunnels) => {
                self.snapshot.tunnels = tunnels;
                self.status_selection.clamp(&self.snapshot);
                self.sync_status_detail();
                self.last_error = None;
                self.last_refresh = Some(Instant::now());
            }
            RuntimeSocketEvent::Agents(agents) => {
                self.snapshot.agents = agents;
                self.status_selection.clamp(&self.snapshot);
                self.sync_status_detail();
                self.last_error = None;
                self.last_refresh = Some(Instant::now());
            }
            RuntimeSocketEvent::Sessions(sessions) => {
                self.snapshot.sessions = sessions;
                self.status_selection.clamp(&self.snapshot);
                self.sync_status_detail();
                self.last_error = None;
                self.last_refresh = Some(Instant::now());
            }
            RuntimeSocketEvent::Error(error) => {
                self.last_error = Some(error);
            }
        }
    }

    fn sync_status_detail(&mut self) {
        if self.view != AppView::StatusDetail {
            return;
        }

        self.detail = self.status_selection.detail(&self.snapshot);
        if self.detail.is_none() {
            self.view = AppView::Status;
        }
    }
}
