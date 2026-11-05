// Derived from herdr src/client/shell/render.rs and src/client/shell/state.rs (Apache-2.0).

use std::collections::{HashMap, HashSet};

use ratatui::{buffer::Buffer, layout::Rect, style::Style};
use unicode_width::UnicodeWidthStr;

use super::navigate::WorkspaceNavigationTarget;
use super::palette::Palette;
use super::scroll::ScrollMetrics;
use super::snapshot::{
    ClientEndpointId, ClientShellEndpoint, ClientShellLayout, ClientShellSnapshot,
};

pub(super) const MIN_TAB_WIDTH: u16 = 8;
pub(super) const NEW_TAB_WIDTH: u16 = 3;
pub(super) const WORKSPACE_HEADER_ROWS: u16 = 2;
pub(super) const TAB_SCROLL_BUTTON_WIDTH: u16 = 3;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum ClientMobileTarget {
    Machine(ClientEndpointId),
    NewWorkspace,
    Workspace {
        endpoint_id: ClientEndpointId,
        workspace_id: String,
    },
    NewTab,
    Tab {
        endpoint_id: ClientEndpointId,
        tab_id: String,
    },
    Agent {
        endpoint_id: ClientEndpointId,
        pane_id: String,
    },
    Menu(usize),
}

#[derive(Clone)]
pub(crate) struct PaneHit {
    pub rect: Rect,
    pub inner_rect: Rect,
    pub scrollbar_rect: Option<Rect>,
    pub scroll: Option<ScrollMetrics>,
    pub pane_id: String,
}

#[derive(Clone)]
pub(crate) struct WorkspaceHit {
    pub rect: Rect,
    pub endpoint_id: ClientEndpointId,
    pub workspace_id: String,
    pub group_toggle: Option<(Rect, String)>,
}

#[derive(Clone)]
pub(crate) struct MachineHit {
    pub rect: Rect,
    pub status_badge: Rect,
    pub collapse_toggle: Rect,
    pub endpoint_id: ClientEndpointId,
}

#[derive(Default, Clone)]
pub(crate) struct ShellHitMap {
    pub machines: Vec<MachineHit>,
    pub workspaces: Vec<WorkspaceHit>,
    pub workspace_body: Rect,
    pub workspace_scrollbar: Rect,
    pub workspace_scroll_metrics: Option<ScrollMetrics>,
    pub workspace_max_scroll: usize,
    pub tabs: Vec<(Rect, String)>,
    pub panes: Vec<PaneHit>,
    pub pane_splits: Vec<crate::app::layout_mouse::SplitHit>,
    pub agents: Vec<(Rect, String)>,
    pub endpoint_agents: Vec<(Rect, ClientEndpointId, String)>,
    pub agent_body: Rect,
    pub agent_scrollbar: Rect,
    pub agent_scroll_metrics: Option<ScrollMetrics>,
    pub agent_max_scroll: usize,
    pub agent_sort_toggle: Rect,
    pub sidebar_divider: Rect,
    pub sidebar_section_divider: Rect,
    pub sidebar_toggle: Rect,
    pub new_workspace: Rect,
    pub new_tab: Rect,
    pub tab_scroll_left: Rect,
    pub tab_scroll_right: Rect,
    pub mobile_switch: Rect,
    pub mobile_close: Rect,
    pub mobile_targets: Vec<(Rect, ClientMobileTarget)>,
    pub mobile_max_scroll: usize,
    pub global_launcher: Rect,
}

/// Diagnostics shown on endpoint rows. hc-tui has no saved-endpoint auth
/// catalog, so the auth badge never applies and `badge_style` is the base.
#[derive(Default)]
pub(crate) struct MachineDiagnostics;

impl MachineDiagnostics {
    pub(crate) fn required_for(&self, _endpoint: &ClientShellEndpoint) -> bool {
        false
    }

    pub(crate) fn badge_style(
        &self,
        _endpoint: &ClientShellEndpoint,
        _palette: &Palette,
        style: Style,
    ) -> Style {
        style
    }
}

pub(crate) struct ShellRenderState<'a> {
    pub machine_diagnostics: &'a MachineDiagnostics,
    pub endpoints: &'a [ClientShellEndpoint],
    pub active_endpoint_id: &'a ClientEndpointId,
    pub collapsed_endpoints: &'a HashSet<ClientEndpointId>,
    pub collapsed_groups: &'a HashSet<String>,
    pub remote_collapsed_groups: &'a HashMap<ClientEndpointId, HashSet<String>>,
    pub workspace_scroll: &'a mut usize,
    pub agent_scroll: &'a mut usize,
    pub tab_scroll: &'a mut usize,
    pub reveal_focused_workspace: &'a mut bool,
    pub reveal_focused_tab: &'a mut bool,
    pub sidebar_collapsed: bool,
    pub sidebar_section_split: f32,
    pub tab_drag_insert_index: Option<usize>,
    pub selected_workspace_id: Option<&'a WorkspaceNavigationTarget>,
    pub reveal_navigation_workspace: &'a mut bool,
    pub dragged_workspace_id: Option<&'a str>,
    pub workspace_drop_indicator_row: Option<u16>,
}

pub(crate) fn render_sidebar_background(buffer: &mut Buffer, area: Rect, palette: &Palette) {
    buffer.set_style(area, Style::default().bg(palette.sidebar_bg));
    let separator_x = area.right().saturating_sub(1);
    for y in area.y..area.bottom() {
        if let Some(cell) = buffer.cell_mut((separator_x, y)) {
            cell.set_symbol("│");
            cell.set_style(Style::default().fg(palette.surface_dim));
        }
    }
}

pub(crate) fn render_shell(
    buffer: &mut Buffer,
    layout: ClientShellLayout,
    snapshot: &ClientShellSnapshot,
    config: &super::ClientShellConfig,
    mut state: ShellRenderState<'_>,
) -> ShellHitMap {
    let mut hits = ShellHitMap::default();
    if layout.mobile_header.height > 0 {
        super::mobile::render_mobile_header(
            buffer,
            layout.mobile_header,
            snapshot,
            config,
            &mut hits,
        );
    }
    if layout.sidebar.width > 0 {
        if state.endpoints.len() > 1 {
            if state.sidebar_collapsed {
                super::endpoint_sidebar::render_collapsed(
                    buffer,
                    layout.sidebar,
                    config,
                    &mut state,
                    &mut hits,
                );
            } else {
                super::endpoint_sidebar_expanded::render_expanded(
                    buffer,
                    layout.sidebar,
                    Some(snapshot),
                    config,
                    &mut state,
                    &mut hits,
                );
            }
        } else if state.sidebar_collapsed {
            super::workspace::render_collapsed_sidebar(
                buffer,
                layout.sidebar,
                snapshot,
                config,
                state
                    .selected_workspace_id
                    .map(|target| target.workspace_id.as_str()),
                &mut hits,
            );
        } else {
            super::sidebar_render::render_sidebar(
                buffer,
                layout.sidebar,
                snapshot,
                config,
                &mut state,
                &mut hits,
            );
        }
    }
    if layout.tab_bar.height > 0 {
        super::tabs::render_tab_bar(
            buffer,
            layout.tab_bar,
            snapshot,
            config,
            state.tab_scroll,
            state.reveal_focused_tab,
            state.tab_drag_insert_index,
            &mut hits,
        );
    }
    if !config.mouse_capture {
        hits.sidebar_divider = Rect::default();
        hits.sidebar_section_divider = Rect::default();
        hits.workspace_scrollbar = Rect::default();
        hits.agent_scrollbar = Rect::default();
        hits.agent_sort_toggle = Rect::default();
        hits.new_workspace = Rect::default();
        hits.machines.clear();
        hits.workspaces.clear();
        hits.agents.clear();
        hits.endpoint_agents.clear();
        hits.tab_scroll_left = Rect::default();
        hits.tab_scroll_right = Rect::default();
        hits.new_tab = Rect::default();
    }
    hits
}

pub(super) fn put_right_text(buffer: &mut Buffer, area: Rect, y: u16, text: &str, style: Style) {
    let width = display_width(text).min(area.width);
    put_text(
        buffer,
        area.right().saturating_sub(width),
        y,
        width,
        text,
        style,
    );
}

pub(super) fn put_segment(
    buffer: &mut Buffer,
    x: u16,
    y: u16,
    right: u16,
    text: &str,
    style: Style,
) -> u16 {
    let width = display_width(text).min(right.saturating_sub(x));
    put_text(buffer, x, y, width, text, style);
    x.saturating_add(width)
}

pub(super) fn put_text(buffer: &mut Buffer, x: u16, y: u16, width: u16, text: &str, style: Style) {
    if width == 0 || y >= buffer.area.bottom() || x >= buffer.area.right() {
        return;
    }
    buffer.set_stringn(x, y, text, width as usize, style);
}

pub(super) fn display_width(text: &str) -> u16 {
    UnicodeWidthStr::width(text).min(u16::MAX as usize) as u16
}
