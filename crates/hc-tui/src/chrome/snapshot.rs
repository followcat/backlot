// Derived from herdr src/protocol/wire.rs, src/client/shell/endpoints.rs, and
// src/client/shell/config.rs (Apache-2.0).

//! The neutral data the ported renderers consume. Field-for-field mirrors of
//! the wire snapshot types, minus endpoint features hc-tui does not project.

use std::collections::HashMap;

use ratatui::layout::Rect;

use super::status::AgentStatus;
use crate::config::{SidebarCollapsedModeConfig, TabBarPositionConfig};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClientShellSnapshot {
    /// Changes whenever the endpoint process restarts.
    pub boot_id: String,
    /// Monotonic replacement revision within one endpoint boot.
    pub revision: u64,
    pub focused_workspace_id: Option<String>,
    pub focused_tab_id: Option<String>,
    pub focused_pane_id: Option<String>,
    pub tab_bar_right: Vec<ClientShellTabStatusSegment>,
    pub tab_bar_right_separator: String,
    pub agent_order: Vec<String>,
    /// Active saved agent-view label; `None` in hc-tui's M2 projection.
    pub agent_view_label: Option<String>,
    pub workspaces: Vec<ClientShellWorkspace>,
    pub tabs: Vec<ClientShellTab>,
    pub panes: Vec<ClientShellPane>,
    pub agents: Vec<ClientShellAgent>,
    pub commands: Vec<ClientShellCommand>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClientShellCommand {
    pub command_id: String,
    pub binding_label: String,
    pub binding_labels: Vec<String>,
    pub action: ClientShellCommandAction,
    pub description: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClientShellCommandAction {
    Shell,
    Pane,
    Popup,
    PluginAction,
    /// A future endpoint action kind that this client cannot execute.
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClientShellTabStatusSegment {
    pub text: String,
    pub accent: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClientShellWorkspace {
    pub workspace_id: String,
    pub active_tab_id: String,
    pub new_workspace_cwd: String,
    pub number: usize,
    pub label: String,
    pub custom_label: bool,
    pub branch: Option<String>,
    pub git_ahead_behind: Option<(usize, usize)>,
    pub tokens: Vec<(String, String)>,
    /// Git checkout identity does not imply Herdr-managed group membership.
    pub checkout: Option<ClientShellCheckout>,
    pub worktree: Option<ClientShellWorktree>,
    pub focused: bool,
    pub agent_status: AgentStatus,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClientShellWorktree {
    pub key: String,
    pub label: String,
    pub is_linked_worktree: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClientShellCheckout {
    pub repository_key: String,
    pub checkout_key: String,
    pub repo_name: String,
    pub checkout_name: String,
    pub repo_root: String,
    pub is_linked_worktree: bool,
    pub is_bare: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClientShellTab {
    pub tab_id: String,
    pub workspace_id: String,
    pub number: usize,
    pub label: String,
    pub custom_label: bool,
    pub zoomed: bool,
    pub focused: bool,
    pub agent_status: AgentStatus,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClientShellPane {
    pub pane_id: String,
    pub workspace_id: String,
    pub tab_id: String,
    pub label: Option<String>,
    pub cwd: Option<String>,
    pub foreground_cwd: Option<String>,
    pub focused: bool,
    pub right_click_passthrough: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClientShellAgent {
    pub pane_id: String,
    pub workspace_id: String,
    pub tab_id: String,
    pub name: Option<String>,
    pub display_agent: Option<String>,
    pub agent: Option<String>,
    pub title: Option<String>,
    pub terminal_title: Option<String>,
    pub terminal_title_stripped: Option<String>,
    pub agent_status: AgentStatus,
    pub state_change_seq: u64,
    pub state_labels: Vec<(String, String)>,
    pub tokens: Vec<(String, String)>,
    pub focused: bool,
}

/// Endpoint connection state shown by the sidebar machine rows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClientEndpointStatus {
    Connecting,
    Online,
    Reconnecting,
    Attention,
    Disabled,
}

/// Opaque endpoint identity. Local renders as the machine row's first entry.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum ClientEndpointId {
    Local,
    Remote(String),
}

impl ClientEndpointId {
    pub fn is_local(&self) -> bool {
        matches!(self, Self::Local)
    }
}

#[derive(Clone, Debug)]
pub struct ClientShellEndpoint {
    pub endpoint_id: ClientEndpointId,
    pub label: String,
    pub status: ClientEndpointStatus,
    pub snapshot: Option<Box<ClientShellSnapshot>>,
    /// Connection generation that produced `snapshot`.
    pub snapshot_generation: Option<u64>,
    pub agent_recency: HashMap<String, u64>,
}

/// Geometry produced by `ClientShellConfig::layout`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClientShellLayout {
    pub sidebar: Rect,
    pub tab_bar: Rect,
    pub mobile_header: Rect,
    pub pane_surface: Rect,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClientSurfaceSize {
    pub cols: u16,
    pub rows: u16,
}

/// Display-relevant slice of `ClientShellConfig`.
#[derive(Debug, Clone)]
pub struct ClientShellLayoutConfig {
    pub sidebar_min_width: u16,
    pub sidebar_max_width: u16,
    pub sidebar_collapsed_mode: SidebarCollapsedModeConfig,
    pub mobile_width_threshold: u16,
    pub tab_bar_position: TabBarPositionConfig,
    pub hide_tab_bar_when_single_tab: bool,
}

impl ClientShellLayoutConfig {
    pub(crate) fn layout(
        &self,
        cols: u16,
        rows: u16,
        sidebar_collapsed: bool,
        tab_count: usize,
        sidebar_width: u16,
    ) -> ClientShellLayout {
        if cols <= self.mobile_width_threshold {
            let header_height = rows.min(2);
            return ClientShellLayout {
                sidebar: Rect::default(),
                tab_bar: Rect::default(),
                mobile_header: Rect::new(0, 0, cols, header_height),
                pane_surface: Rect::new(0, header_height, cols, rows.saturating_sub(header_height)),
            };
        }

        let sidebar_width = if sidebar_collapsed {
            match self.sidebar_collapsed_mode {
                SidebarCollapsedModeConfig::Compact => 4,
                SidebarCollapsedModeConfig::Hidden => 0,
            }
        } else {
            let (min, max) = crate::config::validated_sidebar_bounds(
                self.sidebar_min_width,
                self.sidebar_max_width,
            )
            .unwrap_or((18, 36));
            sidebar_width.clamp(min, max)
        }
        .min(cols.saturating_sub(1));
        let main = Rect::new(sidebar_width, 0, cols.saturating_sub(sidebar_width), rows);
        let show_tab_bar = rows > 1 && !(self.hide_tab_bar_when_single_tab && tab_count == 1);
        let tab_height = u16::from(show_tab_bar);
        let (tab_bar, pane_surface) = match self.tab_bar_position {
            TabBarPositionConfig::Top => (
                Rect::new(main.x, 0, main.width, tab_height),
                Rect::new(
                    main.x,
                    tab_height,
                    main.width,
                    rows.saturating_sub(tab_height),
                ),
            ),
            TabBarPositionConfig::Bottom => (
                Rect::new(
                    main.x,
                    rows.saturating_sub(tab_height),
                    main.width,
                    tab_height,
                ),
                Rect::new(main.x, 0, main.width, rows.saturating_sub(tab_height)),
            ),
        };

        ClientShellLayout {
            sidebar: Rect::new(0, 0, sidebar_width, rows),
            tab_bar,
            mobile_header: Rect::default(),
            pane_surface,
        }
    }
}
