use std::time::Instant;

use super::{ErrorScope, TuiApp};
use crate::runtime_socket::{RuntimeSocketEvent, RuntimeStream};

impl TuiApp {
    pub(crate) fn apply_runtime_socket_event(&mut self, event: RuntimeSocketEvent) {
        match event {
            RuntimeSocketEvent::Channels(channels) => {
                self.snapshot.channels = channels;
                self.on_runtime_snapshot_updated(RuntimeStream::Channels);
            }
            RuntimeSocketEvent::Tunnels(tunnels) => {
                self.snapshot.tunnels = tunnels;
                self.on_runtime_snapshot_updated(RuntimeStream::Tunnels);
            }
            RuntimeSocketEvent::Agents(agents) => {
                self.snapshot.agents = agents;
                self.on_runtime_snapshot_updated(RuntimeStream::Agents);
            }
            RuntimeSocketEvent::Sessions(sessions) => {
                self.snapshot.sessions = sessions;
                self.on_runtime_snapshot_updated(RuntimeStream::Sessions);
            }
            RuntimeSocketEvent::Error { stream, message } => {
                self.set_error(ErrorScope::Runtime(stream), message);
            }
        }
    }

    fn on_runtime_snapshot_updated(&mut self, stream: RuntimeStream) {
        // Keep an open status popup pointing at a valid row as the runtime
        // streams update underneath it.
        self.clamp_popup_cursor();
        self.clear_error(ErrorScope::Runtime(stream));
        self.last_refresh = Some(Instant::now());
    }
}
