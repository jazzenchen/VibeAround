use va_client::profiles::ModelProfileSummary;
use va_client::runtime::AgentInfo;
use va_client::sessions::SessionListItem;
use va_client::workspaces::WorkspaceItem;

use crate::data::{AgentPickerSnapshot, DashboardSnapshot};
use crate::detail::{agent_detail, channel_detail, session_detail, tunnel_detail, DetailContent};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RuntimePanel {
    Channels,
    Tunnels,
    Agents,
    Sessions,
}

impl RuntimePanel {
    fn left(self) -> Self {
        match self {
            Self::Agents => Self::Channels,
            Self::Sessions => Self::Tunnels,
            Self::Channels | Self::Tunnels => self,
        }
    }

    fn right(self) -> Self {
        match self {
            Self::Channels => Self::Agents,
            Self::Tunnels => Self::Sessions,
            Self::Agents | Self::Sessions => self,
        }
    }

    fn up(self) -> Self {
        match self {
            Self::Tunnels => Self::Channels,
            Self::Sessions => Self::Agents,
            Self::Channels | Self::Agents => self,
        }
    }

    fn down(self) -> Self {
        match self {
            Self::Channels => Self::Tunnels,
            Self::Agents => Self::Sessions,
            Self::Tunnels | Self::Sessions => self,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct StatusSelection {
    pub(crate) panel: RuntimePanel,
    channel_index: Option<usize>,
    tunnel_index: Option<usize>,
    agent_index: Option<usize>,
    session_index: Option<usize>,
}

impl Default for StatusSelection {
    fn default() -> Self {
        Self {
            panel: RuntimePanel::Channels,
            channel_index: None,
            tunnel_index: None,
            agent_index: None,
            session_index: None,
        }
    }
}

impl StatusSelection {
    pub(crate) fn index(&self, panel: RuntimePanel) -> Option<usize> {
        match panel {
            RuntimePanel::Channels => self.channel_index,
            RuntimePanel::Tunnels => self.tunnel_index,
            RuntimePanel::Agents => self.agent_index,
            RuntimePanel::Sessions => self.session_index,
        }
    }

    fn active_index(&self) -> Option<usize> {
        self.index(self.panel)
    }

    fn set_index(&mut self, panel: RuntimePanel, index: Option<usize>) {
        match panel {
            RuntimePanel::Channels => self.channel_index = index,
            RuntimePanel::Tunnels => self.tunnel_index = index,
            RuntimePanel::Agents => self.agent_index = index,
            RuntimePanel::Sessions => self.session_index = index,
        }
    }

    pub(crate) fn move_left(&mut self) {
        self.panel = self.panel.left();
    }

    pub(crate) fn move_right(&mut self) {
        self.panel = self.panel.right();
    }

    pub(crate) fn move_up(&mut self, snapshot: &DashboardSnapshot) {
        if self.panel.up() == self.panel {
            self.select_previous(snapshot);
        } else {
            self.panel = self.panel.up();
        }
    }

    pub(crate) fn move_down(&mut self, snapshot: &DashboardSnapshot) {
        if self.panel.down() == self.panel {
            self.select_next(snapshot);
        } else {
            self.panel = self.panel.down();
        }
    }

    pub(crate) fn select_next(&mut self, snapshot: &DashboardSnapshot) {
        let panel = self.panel;
        self.select_next_in_panel(panel, self.item_count(snapshot, panel));
    }

    pub(crate) fn select_previous(&mut self, snapshot: &DashboardSnapshot) {
        let panel = self.panel;
        self.select_previous_in_panel(panel, self.item_count(snapshot, panel));
    }

    fn select_next_in_panel(&mut self, panel: RuntimePanel, item_count: usize) {
        if item_count == 0 {
            self.set_index(panel, None);
            return;
        }
        let next = self
            .index(panel)
            .map(|index| (index + 1) % item_count)
            .unwrap_or(0);
        self.set_index(panel, Some(next));
    }

    fn select_previous_in_panel(&mut self, panel: RuntimePanel, item_count: usize) {
        if item_count == 0 {
            self.set_index(panel, None);
            return;
        }
        let last = item_count - 1;
        let previous = self
            .index(panel)
            .map(|index| if index == 0 { last } else { index - 1 })
            .unwrap_or(0);
        self.set_index(panel, Some(previous));
    }

    pub(crate) fn clamp(&mut self, snapshot: &DashboardSnapshot) {
        for panel in [
            RuntimePanel::Channels,
            RuntimePanel::Tunnels,
            RuntimePanel::Agents,
            RuntimePanel::Sessions,
        ] {
            self.clamp_panel(panel, self.item_count(snapshot, panel));
        }
    }

    fn clamp_panel(&mut self, panel: RuntimePanel, item_count: usize) {
        if item_count == 0 {
            self.set_index(panel, None);
            return;
        }
        let index = self.index(panel).unwrap_or(0).min(item_count - 1);
        self.set_index(panel, Some(index));
    }

    fn item_count(&self, snapshot: &DashboardSnapshot, panel: RuntimePanel) -> usize {
        match panel {
            RuntimePanel::Channels => snapshot.channels.len(),
            RuntimePanel::Tunnels => snapshot.tunnels.len(),
            RuntimePanel::Agents => snapshot.agents.len(),
            RuntimePanel::Sessions => snapshot.sessions.len(),
        }
    }

    pub(crate) fn detail(&self, snapshot: &DashboardSnapshot) -> Option<DetailContent> {
        match self.panel {
            RuntimePanel::Channels => self
                .active_index()
                .and_then(|index| snapshot.channels.get(index))
                .map(channel_detail),
            RuntimePanel::Tunnels => self
                .active_index()
                .and_then(|index| snapshot.tunnels.get(index))
                .map(tunnel_detail),
            RuntimePanel::Agents => self
                .active_index()
                .and_then(|index| snapshot.agents.get(index))
                .map(agent_detail),
            RuntimePanel::Sessions => self
                .active_index()
                .and_then(|index| snapshot.sessions.get(index))
                .map(session_detail),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AgentPanel {
    Agents,
    Profiles,
    Workspaces,
    Sessions,
}

impl AgentPanel {
    fn left(self) -> Self {
        match self {
            Self::Profiles => Self::Agents,
            Self::Sessions => Self::Workspaces,
            Self::Agents | Self::Workspaces => self,
        }
    }

    fn right(self) -> Self {
        match self {
            Self::Agents => Self::Profiles,
            Self::Workspaces => Self::Sessions,
            Self::Profiles | Self::Sessions => self,
        }
    }

    fn up(self) -> Self {
        match self {
            Self::Workspaces => Self::Agents,
            Self::Sessions => Self::Profiles,
            Self::Agents | Self::Profiles => self,
        }
    }

    fn down(self) -> Self {
        match self {
            Self::Agents => Self::Workspaces,
            Self::Profiles => Self::Sessions,
            Self::Workspaces | Self::Sessions => self,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AgentSelection {
    pub(crate) panel: AgentPanel,
    agent_index: Option<usize>,
    profile_index: Option<usize>,
    workspace_index: Option<usize>,
    session_index: Option<usize>,
}

impl Default for AgentSelection {
    fn default() -> Self {
        Self {
            panel: AgentPanel::Agents,
            agent_index: None,
            profile_index: None,
            workspace_index: None,
            session_index: None,
        }
    }
}

impl AgentSelection {
    pub(crate) fn index(&self, panel: AgentPanel) -> Option<usize> {
        match panel {
            AgentPanel::Agents => self.agent_index,
            AgentPanel::Profiles => self.profile_index,
            AgentPanel::Workspaces => self.workspace_index,
            AgentPanel::Sessions => self.session_index,
        }
    }

    fn set_index(&mut self, panel: AgentPanel, index: Option<usize>) {
        match panel {
            AgentPanel::Agents => self.agent_index = index,
            AgentPanel::Profiles => self.profile_index = index,
            AgentPanel::Workspaces => self.workspace_index = index,
            AgentPanel::Sessions => self.session_index = index,
        }
    }

    pub(crate) fn move_left(&mut self) {
        self.panel = self.panel.left();
    }

    pub(crate) fn move_right(&mut self) {
        self.panel = self.panel.right();
    }

    pub(crate) fn move_up(&mut self, snapshot: &AgentPickerSnapshot) {
        if self.panel.up() == self.panel {
            self.select_previous(snapshot);
        } else {
            self.panel = self.panel.up();
        }
    }

    pub(crate) fn move_down(&mut self, snapshot: &AgentPickerSnapshot) {
        if self.panel.down() == self.panel {
            self.select_next(snapshot);
        } else {
            self.panel = self.panel.down();
        }
    }

    fn select_next(&mut self, snapshot: &AgentPickerSnapshot) {
        let panel = self.panel;
        self.select_next_in_panel(panel, self.item_count(snapshot, panel));
    }

    fn select_previous(&mut self, snapshot: &AgentPickerSnapshot) {
        let panel = self.panel;
        self.select_previous_in_panel(panel, self.item_count(snapshot, panel));
    }

    fn select_next_in_panel(&mut self, panel: AgentPanel, item_count: usize) {
        if item_count == 0 {
            self.set_index(panel, None);
            return;
        }
        let next = self
            .index(panel)
            .map(|index| (index + 1) % item_count)
            .unwrap_or(0);
        self.set_index(panel, Some(next));
    }

    fn select_previous_in_panel(&mut self, panel: AgentPanel, item_count: usize) {
        if item_count == 0 {
            self.set_index(panel, None);
            return;
        }
        let last = item_count - 1;
        let previous = self
            .index(panel)
            .map(|index| if index == 0 { last } else { index - 1 })
            .unwrap_or(0);
        self.set_index(panel, Some(previous));
    }

    pub(crate) fn clamp(&mut self, snapshot: &AgentPickerSnapshot) {
        for panel in [
            AgentPanel::Agents,
            AgentPanel::Profiles,
            AgentPanel::Workspaces,
            AgentPanel::Sessions,
        ] {
            self.clamp_panel(panel, self.item_count(snapshot, panel));
        }
    }

    fn clamp_panel(&mut self, panel: AgentPanel, item_count: usize) {
        if item_count == 0 {
            self.set_index(panel, None);
            return;
        }
        let index = self.index(panel).unwrap_or(0).min(item_count - 1);
        self.set_index(panel, Some(index));
    }

    fn item_count(&self, snapshot: &AgentPickerSnapshot, panel: AgentPanel) -> usize {
        match panel {
            AgentPanel::Agents => snapshot.agents.len(),
            AgentPanel::Profiles => snapshot.profiles.len(),
            AgentPanel::Workspaces => snapshot.workspaces.len(),
            AgentPanel::Sessions => snapshot.sessions.len(),
        }
    }

    pub(crate) fn selected_agent<'a>(
        &self,
        snapshot: &'a AgentPickerSnapshot,
    ) -> Option<&'a AgentInfo> {
        self.agent_index
            .and_then(|index| snapshot.agents.get(index))
    }

    pub(crate) fn selected_profile<'a>(
        &self,
        snapshot: &'a AgentPickerSnapshot,
    ) -> Option<&'a ModelProfileSummary> {
        self.profile_index
            .and_then(|index| snapshot.profiles.get(index))
    }

    pub(crate) fn selected_workspace<'a>(
        &self,
        snapshot: &'a AgentPickerSnapshot,
    ) -> Option<&'a WorkspaceItem> {
        self.workspace_index
            .and_then(|index| snapshot.workspaces.get(index))
    }

    pub(crate) fn selected_session<'a>(
        &self,
        snapshot: &'a AgentPickerSnapshot,
    ) -> Option<&'a SessionListItem> {
        self.session_index
            .and_then(|index| snapshot.sessions.get(index))
    }
}
