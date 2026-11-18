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
    /// The floating popup's hit — the reference's `hits.popup`, rebuilt
    /// each frame by `draw_popups`.
    pub popup: Option<PaneHit>,
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
    /// The update hint inside the version badge — just the hint, so
    /// the version text itself stays inert.
    pub update_badge: Rect,
    pub mobile_targets: Vec<(Rect, ClientMobileTarget)>,
    pub mobile_max_scroll: usize,
    pub global_launcher: Rect,
    /// Row hit areas of the open context menu, filled during draw.
    pub context_menu_rows: Vec<(Rect, usize)>,
    /// The studio cast's `others` toggle row (empty in herdr mode).
    pub cast_others: Rect,
    /// The lot pill's segment at the bar's right end — clicking it
    /// toggles the Hollywood bar.
    pub lot_pill: Rect,
    /// The studio name inside a studio header — clicking it focuses
    /// the dock on that studio.
    pub studio_header: Rect,
    pub studio_key: Option<crate::studio::StudioKey>,
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
    /// The badge's update hint: a newer release's ` ↑v…` or a
    /// verified install's ` ↻v…` while it awaits its restart.
    pub update_badge: UpdateBadge<'a>,
    /// The studio cast's frame input — `Some` replaces the agents
    /// section in the expanded sidebar (studio mode).
    pub cast: Option<crate::cast::CastView<'a>>,
    /// The cast section's scroll — written back like `agent_scroll`.
    pub cast_scroll: &'a mut usize,
    /// The lot latch for this frame — `None` in plain mode and
    /// whenever there is nothing for the pill to say.
    pub lot: Option<super::lot::LotFrame<'a>>,
}

pub(crate) fn render_sidebar_background(buffer: &mut Buffer, area: Rect, palette: &Palette) {
    buffer.set_style(area, Style::default().bg(palette.sidebar_bg));
    // The edge faces the panes and takes the sidebar's own last
    // column, never width from them. An opaque sidebar gets the
    // theme's surface0 rule; a translucent herdr one keeps the
    // reference's dim separator.
    let edge = if palette.sidebar_bg == ratatui::style::Color::Reset {
        palette.surface_dim
    } else {
        palette.surface0
    };
    let separator_x = area.right().saturating_sub(1);
    for y in area.y..area.bottom() {
        if let Some(cell) = buffer.cell_mut((separator_x, y)) {
            cell.set_symbol("│");
            cell.set_style(Style::default().fg(edge));
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
            state.update_badge,
            state.lot.as_ref(),
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
                state.update_badge,
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
        let lot = state.lot.as_ref();
        let tier = lot
            .map(|l| super::lot::pill_tier(l.cols, config.mobile_width_threshold))
            .unwrap_or(super::lot::PillTier::Full);
        if let Some(lot) = lot.filter(|l| l.header.is_some()) {
            // A studio workspace: its own one-row header where the
            // tab bar was, with the pill at the right end.
            super::lot::render_studio_header(
                buffer,
                layout.tab_bar,
                lot,
                tier,
                &config.palette,
                &mut hits,
            );
        } else {
            // Any other workspace keeps herdr's tab bar; the pill is
            // a reserved segment at its right end.
            let pill_w = lot
                .filter(|l| l.pill)
                .map(|l| super::lot::pill_width(&l.counts, tier, l.agent))
                .unwrap_or(0)
                .min(layout.tab_bar.width);
            let tabs_area = Rect::new(
                layout.tab_bar.x,
                layout.tab_bar.y,
                layout.tab_bar.width - pill_w,
                layout.tab_bar.height,
            );
            super::tabs::render_tab_bar(
                buffer,
                tabs_area,
                snapshot,
                config,
                state.tab_scroll,
                state.reveal_focused_tab,
                state.tab_drag_insert_index,
                &mut hits,
            );
            if pill_w > 0 {
                hits.lot_pill = super::lot::render_pill(
                    buffer,
                    Rect::new(layout.tab_bar.right() - pill_w, layout.tab_bar.y, pill_w, 1),
                    &lot.map(|l| l.counts).unwrap_or_default(),
                    tier,
                    lot.is_some_and(|l| l.agent),
                    &config.palette,
                );
            }
        }
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
        hits.update_badge = Rect::default();
    }
    hits
}

pub(crate) fn put_right_text(buffer: &mut Buffer, area: Rect, y: u16, text: &str, style: Style) {
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

pub(crate) fn put_text(buffer: &mut Buffer, x: u16, y: u16, width: u16, text: &str, style: Style) {
    if width == 0 || y >= buffer.area.bottom() || x >= buffer.area.right() {
        return;
    }
    buffer.set_stringn(x, y, text, width as usize, style);
}

pub(super) fn display_width(text: &str) -> u16 {
    UnicodeWidthStr::width(text).min(u16::MAX as usize) as u16
}

/// The version badge's update hint — ` ↑v…` in accent while a newer
/// release is offered, ` ↻v…` in green once a verified install only
/// waits on its restart. The restart hint wins while both are known.
#[derive(Clone, Copy)]
pub(crate) enum UpdateBadge<'a> {
    None,
    Update(&'a str),
    Restart(&'a str),
}

impl<'a> UpdateBadge<'a> {
    pub(crate) fn new(update: Option<&'a str>, restart: Option<&'a str>) -> Self {
        match (restart, update) {
            (Some(version), _) => Self::Restart(version),
            (None, Some(version)) => Self::Update(version),
            (None, None) => Self::None,
        }
    }
}

impl UpdateBadge<'_> {
    /// The accent of the update dot and the collapsed toggle: accent while
    /// an update waits, green once an install waits for its restart.
    pub(crate) fn accent_style(self, palette: &Palette) -> Option<Style> {
        match self {
            UpdateBadge::None => None,
            UpdateBadge::Update(_) => Some(
                Style::default()
                    .fg(palette.accent)
                    .add_modifier(ratatui::style::Modifier::BOLD),
            ),
            UpdateBadge::Restart(_) => Some(
                Style::default()
                    .fg(palette.green)
                    .add_modifier(ratatui::style::Modifier::BOLD),
            ),
        }
    }
}

/// The badge's two parts: the running `vX.Y.Z`, and — once the
/// update check reported a newer release — the ` ↑v…` hint in the
/// notice toast's accent (or ` ↻v…` in green for a pending restart).
/// The hint stays tight so a two-digit pair still fits the
/// default-width header.
fn version_badge_text(update_badge: UpdateBadge<'_>) -> (String, Option<String>) {
    let hint = match update_badge {
        UpdateBadge::Update(latest) => Some(format!(" ↑v{latest}")),
        UpdateBadge::Restart(version) => Some(format!(" ↻v{version}")),
        UpdateBadge::None => None,
    };
    (format!("v{}", env!("CARGO_PKG_VERSION")), hint)
}

/// Width of the fully drawn badge — callers reserve a slot with it.
pub(super) fn version_badge_width(update_badge: UpdateBadge<'_>) -> u16 {
    let (version, hint) = version_badge_text(update_badge);
    display_width(&version) + hint.as_deref().map_or(0, display_width)
}

/// Right-aligns the badge inside `row` without crossing `min_left`:
/// the update hint drops first under pressure, then the whole badge.
/// Returns the hint's rect — the clickable slice — empty otherwise.
pub(super) fn put_version_badge(
    buffer: &mut Buffer,
    row: Rect,
    min_left: u16,
    update_badge: UpdateBadge<'_>,
    palette: &Palette,
) -> Rect {
    let (version, hint) = version_badge_text(update_badge);
    let ver_w = display_width(&version);
    let hint_w = hint.as_deref().map_or(0, display_width);
    let right = row.right();
    let draw_hint = hint.is_some() && right >= min_left.saturating_add(ver_w + hint_w);
    let total = ver_w + if draw_hint { hint_w } else { 0 };
    if right < min_left.saturating_add(total) {
        return Rect::default();
    }
    let ver_x = right.saturating_sub(total);
    put_text(
        buffer,
        ver_x,
        row.y,
        ver_w,
        &version,
        Style::default().fg(palette.overlay0),
    );
    if !draw_hint {
        return Rect::default();
    }
    let hint_x = ver_x.saturating_add(ver_w);
    let hint_color = match update_badge {
        UpdateBadge::Restart(_) => palette.green,
        _ => palette.accent,
    };
    put_text(
        buffer,
        hint_x,
        row.y,
        hint_w,
        hint.as_deref().unwrap_or_default(),
        Style::default().fg(hint_color),
    );
    Rect::new(hint_x, row.y, hint_w, 1)
}
