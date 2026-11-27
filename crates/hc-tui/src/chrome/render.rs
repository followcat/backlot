// Derived from herdr src/client/shell/render.rs and src/client/shell/state.rs (Apache-2.0).

use std::collections::{HashMap, HashSet};

use ratatui::{buffer::Buffer, layout::Rect, style::Style};

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

/// U6c: a machine row's version sub-line in the expanded sidebar —
/// the app renders the text and tone, the sidebar only draws it.
/// `None` lines (plain mode) keep the row one line tall, so Herdr's
/// `--plain` frames are untouched.
#[derive(Debug, Clone)]
pub(crate) struct MachineLine {
    /// The endpoint's `hc` version when `machine.list` knows it —
    /// drawn dim as `hc <v>` ahead of the backlot text when it fits.
    pub hc: Option<String>,
    /// `backlot <v>` / `↑<latest>` / `no backlot` / `backlot …`.
    pub text: String,
    /// The accent (`↑` update hint), normal or dim tone.
    pub tone: MachineTone,
    /// Optional stage health suffix (`· ● ok` / `· ◌ down`) when installed.
    pub stage_ok: Option<bool>,
}

/// A machine sub-line's palette choice.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MachineTone {
    /// `backlot <v>` — plain text.
    Normal,
    /// `↑<latest>` — the accent colour.
    Accent,
    /// `no backlot`, `backlot …`, `backlot ?` — dim.
    Dim,
}

#[derive(Clone)]
pub(crate) struct MachineHit {
    pub rect: Rect,
    pub status_badge: Rect,
    pub collapse_toggle: Rect,
    pub endpoint_id: ClientEndpointId,
}

/// What a click on a studio-mode machines-tree row does (W1).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum StudioRowAct {
    /// A ledger studio whose machine's stage is down — start it.
    StartStage,
    /// The `finished (n)` group — fold or unfold it.
    ToggleFinished,
    /// One finished studio — open its finished-studio menu.
    OpenFinished { id: String },
    /// `Archived (n) …` — open the machine's archived-studios list.
    OpenArchived,
}

/// A studio-mode machines-tree row's hit area.
#[derive(Clone)]
pub(crate) struct StudioRowHit {
    pub rect: Rect,
    pub endpoint_id: ClientEndpointId,
    pub act: StudioRowAct,
}

#[derive(Default, Clone)]
pub(crate) struct ShellHitMap {
    pub machines: Vec<MachineHit>,
    pub workspaces: Vec<WorkspaceHit>,
    /// Studio-mode machines-tree rows that are not spaces (W1).
    pub studio_rows: Vec<StudioRowHit>,
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
    pub lot_pill_segments: Vec<(Rect, super::lot::PillAction)>,
    pub sidebar_help: Rect,
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
    /// Whether the sidebar is the machine-first (federated) view:
    /// studio mode always, `--plain` only with several endpoints.
    pub federated: bool,
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
    pub reveal_focused_agent: &'a mut bool,
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
    /// U6c's per-machine version lines, keyed by endpoint id —
    /// `None` in plain mode so Herdr's rows stay exactly one line.
    pub machine_lines: Option<&'a std::collections::BTreeMap<ClientEndpointId, MachineLine>>,
    /// Studio mode hides the internal `backlot` stage space from the sidebar.
    pub hide_stage_space: bool,
    /// Studio currently owning `StudioFocus::Studio(key)`, highlighted in the sidebar.
    pub focused_studio: Option<&'a crate::studio::StudioKey>,
    /// W1: the extra studio rows under each machine in studio mode —
    /// `None` in plain mode, where herdr's tree is untouched.
    pub studio_tree: Option<&'a crate::chrome::studio_tree::Tree>,
    /// The current studio — the machines tree highlights its row and
    /// keeps its machine in view.
    pub current_studio: Option<&'a crate::studio::StudioKey>,
    /// The studio selected inside a focused Lot — quietly marks the
    /// endpoint row and backlot server row that host it without scrolling.
    pub lot_selected_studio: Option<&'a crate::studio::StudioKey>,
    /// The machines tree must scroll to the current studio's machine
    /// row this frame (W1, M4).
    pub reveal_studio_row: &'a mut bool,
    /// W1b: the machines tree scrolls to this row this frame, and the
    /// cursor paints the row it names.
    pub reveal_tree_row: &'a mut bool,
    /// The left tree's keyboard cursor (`Ctrl+B t`) — the row drawn as
    /// selected, `None` while the tree does not own the keys.
    pub tree_cursor: Option<&'a crate::chrome::studio_tree::SidebarRow>,
    /// True while `WorkspaceRegion::Tree` owns keyboard focus.
    pub tree_focused: bool,
    /// Remembered cursor rendered as faint `·` when Tree is unfocused.
    pub tree_remembered: Option<&'a crate::chrome::studio_tree::SidebarRow>,
}

pub(crate) fn sidebar_surface_and_edge(
    palette: &Palette,
) -> (ratatui::style::Color, ratatui::style::Color) {
    let elev = palette.elevation();
    // Translucent themes keep `Color::Reset` + `surface_dim` for Herdr
    // parity. Opaque `backlot` joins `elevation.surface` unless the
    // user explicitly overrode `[theme.custom] sidebar_bg`.
    if palette.sidebar_bg == ratatui::style::Color::Reset {
        (palette.sidebar_bg, palette.surface_dim)
    } else if palette.sidebar_bg == Palette::backlot().sidebar_bg {
        (elev.surface, elev.border_muted)
    } else {
        (palette.sidebar_bg, elev.border_muted)
    }
}

pub(crate) fn render_sidebar_background(buffer: &mut Buffer, area: Rect, palette: &Palette) {
    let (bg, edge) = sidebar_surface_and_edge(palette);
    buffer.set_style(area, Style::default().bg(bg));
    let separator_x = area.right().saturating_sub(1);
    for y in area.y..area.bottom() {
        if let Some(cell) = buffer.cell_mut((separator_x, y)) {
            cell.set_symbol("│");
            cell.set_style(Style::default().fg(edge).bg(bg));
        }
    }
}

/// Paint bottom-left `? keys` next to `«` in expanded studio sidebars.
pub(crate) fn render_sidebar_help(
    buffer: &mut Buffer,
    area: Rect,
    palette: &Palette,
    hits: &mut ShellHitMap,
) {
    if area.height == 0 || area.width < 12 {
        return;
    }
    let y = area.bottom().saturating_sub(1);
    let (bg, _) = sidebar_surface_and_edge(palette);
    put_text(
        buffer,
        area.x.saturating_add(1),
        y,
        1,
        "?",
        Style::default().fg(palette.subtext0).bg(bg),
    );
    put_text(
        buffer,
        area.x.saturating_add(2),
        y,
        5,
        " keys",
        Style::default().fg(palette.overlay0).bg(bg),
    );
    hits.sidebar_help = Rect::new(area.x.saturating_add(1), y, 6, 1);
}

/// Display width of a structured footer hint pair slice rendered via `put_hint_bar`.
pub(crate) fn hint_bar_width(pairs: &[(&str, &str)]) -> u16 {
    let mut w = 0u16;
    for (idx, (key, label)) in pairs.iter().enumerate() {
        if idx > 0 {
            w = w.saturating_add(2);
        }
        w = w.saturating_add(crate::chrome::text::display_width(key) as u16);
        if !label.is_empty() {
            if !key.is_empty() {
                w = w.saturating_add(1);
            }
            w = w.saturating_add(crate::chrome::text::display_width(label) as u16);
        }
    }
    w
}

/// Draw a unified two-tone footer hint bar from structured `(key, label)` pairs:
/// keys render in `subtext0`, explanatory labels in `elevation.muted_surface`,
/// across the row on `elevation.surface` (Call Sheet Band + Studio Dock footer).
pub(crate) fn put_hint_bar(
    buf: &mut Buffer,
    x: u16,
    y: u16,
    max_w: u16,
    pairs: &[(&str, &str)],
    palette: &Palette,
) {
    if max_w == 0 || pairs.is_empty() {
        return;
    }
    let elev = palette.elevation();
    let bg = elev.surface;
    let key_style = Style::default().fg(palette.subtext0).bg(bg);
    let label_style = Style::default().fg(elev.muted_surface).bg(bg);
    let gap_style = Style::default().bg(bg);
    let mut cx = x;
    let limit = x.saturating_add(max_w);
    for (idx, (key, label)) in pairs.iter().enumerate() {
        if cx >= limit {
            break;
        }
        if idx > 0 {
            let take = 2u16.min(limit - cx);
            put_text(buf, cx, y, take, &"  "[..usize::from(take)], gap_style);
            cx = cx.saturating_add(take);
        }
        let w_key = crate::chrome::text::display_width(key) as u16;
        if w_key > 0 {
            if cx.saturating_add(w_key) > limit {
                break;
            }
            put_text(buf, cx, y, limit - cx, key, key_style);
            cx = cx.saturating_add(w_key);
        }
        if !label.is_empty() && cx < limit {
            if !key.is_empty() {
                if cx + 1 > limit {
                    break;
                }
                put_text(buf, cx, y, 1, " ", gap_style);
                cx += 1;
            }
            let w_lbl = (crate::chrome::text::display_width(label) as u16).min(limit - cx);
            if w_lbl > 0 {
                put_text(buf, cx, y, w_lbl, label, label_style);
                cx = cx.saturating_add(w_lbl);
            }
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
        if state.federated {
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
                super::lot::render_pill_recorded(
                    buffer,
                    Rect::new(layout.tab_bar.right() - pill_w, layout.tab_bar.y, pill_w, 1),
                    &lot.map(|l| l.counts).unwrap_or_default(),
                    tier,
                    lot.is_some_and(|l| l.agent),
                    lot.map_or(0, |l| l.spin),
                    &config.palette,
                    &mut hits,
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
    // The reveal belongs to the frame that armed it: whichever agents list
    // drew consumed it, and a sidebar that scrolls no agent list at all
    // drops it so a later manual scroll is never overridden.
    *state.reveal_focused_agent = false;
    hits
}

pub(super) use super::text_draw::display_width;
pub(crate) use super::text_draw::{put_right_text, put_role_badged_text, put_segment, put_text};

pub(crate) use super::badge::UpdateBadge;
pub(super) use super::badge::{
    put_version_badge, version_badge_compact_width, version_badge_width,
};
