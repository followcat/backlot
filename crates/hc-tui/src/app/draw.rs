//! Frame composition: sidebar half, the pane's VT screen, a status
//! line and the help/selector overlays. One `layout` function owns the
//! pane geometry — the attach allocation and the renderer see the
//! exact same inner rect, so no row or column is silently clipped.
//!
//! Visible panes paint through their `PaneRenderCache` (R2-T4): a
//! clean VT replays its cached surface for zero cell reads while a
//! dirty one repaints only the rows the render state reports, so one
//! dirty pane among many costs its share of the frame's cell work.

use std::sync::atomic::Ordering;
use std::time::Instant;

use hc_vt::{CellColor, CursorShape, Rgb, Wide};
use ratatui::buffer::{Buffer, CellWidth};
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::widgets::{Paragraph, Widget};

use super::pane::Paint;
pub(crate) use super::pane_geometry::pane_inner;
use super::pane_geometry::{inner_for, pane_infos, split_borders};
use super::{App, Mode};

#[path = "draw_overlays.rs"]
mod draw_overlays;
#[path = "draw_overlays_s3.rs"]
mod draw_overlays_s3;
#[path = "draw_overlays_s5.rs"]
mod draw_overlays_s5;
#[path = "draw_terminal.rs"]
mod draw_terminal;
#[cfg(test)]
#[path = "draw_tests.rs"]
mod tests;

pub(in crate::app) use draw_terminal::{draw_terminal, mark_wide_tail_slots};
use draw_terminal::{draw_terminal_counted, mark_wide_tail_slots_skipping};

/// What the frame wants the outer cursor to look like: a real
/// position inside `inner` (suppressed for overlays/observe) and the
/// program's shape, which `run` applies to the backend on change.
#[derive(Debug, Clone, Copy)]
pub(crate) struct CursorWish {
    /// Inner-rect-relative position, or none (hidden).
    pub pos: Option<(u16, u16)>,
    /// Explicit hidden cursor position, or None for Source fallback memory.
    pub anchor: Option<(u16, u16)>,
    /// The inner program's cursor shape.
    pub shape: CursorShape,
    /// Whether it blinks (`CSI 5 SP q` shows a blinking bar, not steady).
    pub blinking: bool,
}

/// One displayed frame into `frame`'s buffer.
///
/// The chrome (sidebar, tab bar, mobile header) is drawn by the ported
/// `render_shell`; the attached pane's VT grid fills `pane_surface`'s
/// inner rect under the reference pane-chrome rules; the mode bar
/// overlays the pane surface's last row outside Terminal mode.
pub fn draw(app: &mut App, frame: &mut ratatui::Frame, _now: Instant) {
    let area = frame.area();
    let buf = frame.buffer_mut();
    // Source acknowledges a current visible surface during composition, so an
    // already viewed completion does not flash Done before its Idle frame.
    let painted = app
        .visible_panes()
        .into_iter()
        .filter(|id| app.displayed_pane(id).is_some_and(|p| p.has_fresh_paint()))
        .collect::<Vec<_>>();
    for pane_id in painted {
        app.presentation.acknowledge(
            &mut app.projection,
            &pane_id,
            app.outer_focused,
            &app.waiting,
        );
    }
    let layout = app.chrome_layout(area.width, area.height);
    // U1: studio mode replaces the agents section with the current
    // studio's cast — borrowed views only, every machine's summaries.
    let machines = app.panel_summaries();
    let studio_focused = matches!(
        app.studio.focus,
        Some(crate::studio::StudioFocus::Studio(_))
    );
    let cast_focused = match &app.studio.focus {
        Some(crate::studio::StudioFocus::Studio(_)) => None,
        Some(crate::studio::StudioFocus::Role(_, term)) => Some(term.as_str()),
        None if app.studio.lot.studio_workspace => app.focused.as_deref(),
        None => None,
    };
    let mut cast = app.studio.active.then(|| {
        crate::cast::CastView::build(
            &machines,
            &app.projection.snapshot,
            &app.view,
            app.studio.current.as_ref(),
            Some(crate::project::LOCAL_LABEL),
            &app.shell,
            app.studio.others_expanded,
            cast_focused,
            app.panel.spin,
            crate::update_state::now_ms(),
        )
    });
    if let Some(view) = cast.as_mut() {
        view.studio_focused = studio_focused;
        view.region_focused = app.focus.workspace == crate::focus::WorkspaceRegion::Cast
            && app.focus.modals.is_empty();
        view.cursor_target = app.cast_focus.clone();
        view.remembered_target = app.cast_remembered.clone();
        view.active_chip = app.cast_chip;
        // W1b: with no studio current and no stage running, the cast
        // says `stage stopped — s to start` rather than drawing blank.
        // The `select a studio` hint (studios exist, none current) keeps
        // its own line — this fills the otherwise-empty cast.
        let stage_running = app.projection.endpoints.iter().any(|endpoint| {
            let machine = match &endpoint.endpoint_id {
                crate::chrome::snapshot::ClientEndpointId::Local => None,
                crate::chrome::snapshot::ClientEndpointId::Remote(name) => Some(name.as_str()),
            };
            app.machine_stage_running(machine)
        });
        let server_down = app.server_stopped_command(None).is_some();
        view.server_stopped = view.studio.is_none() && !view.hint && server_down;
        view.stopped = view.studio.is_none() && !view.hint && !stage_running && !server_down;
    }
    // W1: the extra studio rows under each machine, and the current
    // studio the tree highlights and keeps in view — owned before the
    // `shell_state`/`studio` mutable borrows below.
    let studio_tree = app.studio.active.then(|| app.studio_sidebar_tree());
    let current_studio = app.studio.current.clone();
    let pane_focus_active = app.focus.workspace == crate::focus::WorkspaceRegion::Terminal
        && app.focus.modals.is_empty();
    let single_has_border = app
        .projection
        .snapshot
        .focused_tab_id
        .as_ref()
        .and_then(|id| app.projection.layouts.get(id))
        .and_then(|t| t.root.as_ref())
        .is_some_and(|r| {
            app.shell
                .pane_borders
                .shows_borders(r.terminals().len() > 1)
        });
    let lot = (app.studio.lot.pill || app.studio.lot.header.is_some()).then_some(
        crate::chrome::lot::LotFrame {
            counts: app.studio.lot.counts,
            agent: app.has_hollywood_cfg,
            pill: app.studio.lot.pill,
            header: app.studio.lot.header.as_ref(),
            unfocused_cue: !app.plain
                && app.studio.active
                && !pane_focus_active
                && app.focus.modals.is_empty()
                && !app.panel_fullscreen_at(area.width)
                && !single_has_border
                && !layout.pane_surface.is_empty()
                && layout.pane_surface.width >= 40,
            cols: app.area.0,
            spin: app.panel.spin,
        },
    );
    let update_latest = app.update_ready_version().map(str::to_owned);
    let restart_ready = app.restart_ready.clone();
    let update_badge =
        crate::chrome::render::UpdateBadge::new(update_latest.as_deref(), restart_ready.as_deref());
    // U6c's per-machine version lines — `None` keeps plain mode
    // cell-identical to Herdr; resolved before `shell_state`'s
    // mutable borrows below.
    let machine_lines = (!app.plain).then(|| app.machine_lines());
    // Resolved before the `shell_state` / `studio` borrows below.
    let federated = app.federated_sidebar();
    let focused_studio = (!app.plain)
        .then(|| app.studio.focused_studio())
        .flatten()
        .cloned();
    let lot_selected_studio = (!app.plain && app.panel.open() && app.panel.focused)
        .then(|| match app.panel.selected.as_ref() {
            Some(crate::panel::Target::Stage(machine)) => app
                .studio
                .current
                .as_ref()
                .filter(|k| k.machine.as_deref() == machine.as_deref())
                .cloned()
                .or_else(|| {
                    crate::panel::data::studios(&machines)
                        .into_iter()
                        .find(|s| s.machine == machine.as_deref())
                        .map(|s| s.key())
                }),
            Some(target) => target.studio().cloned(),
            None => None,
        })
        .flatten();
    let state = crate::chrome::render::ShellRenderState {
        machine_diagnostics: &app.shell_state.machine_diagnostics,
        federated,
        endpoints: &app.projection.endpoints,
        active_endpoint_id: &app.projection.active_endpoint_id,
        collapsed_endpoints: &app.shell_state.collapsed_endpoints,
        collapsed_groups: &app.shell_state.collapsed_groups,
        remote_collapsed_groups: &app.shell_state.remote_collapsed_groups,
        workspace_scroll: &mut app.shell_state.workspace_scroll,
        agent_scroll: &mut app.shell_state.agent_scroll,
        tab_scroll: &mut app.shell_state.tab_scroll,
        reveal_focused_workspace: &mut app.shell_state.reveal_focused_workspace,
        reveal_focused_tab: &mut app.shell_state.reveal_focused_tab,
        reveal_focused_agent: &mut app.shell_state.reveal_focused_agent,
        sidebar_collapsed: app.shell_state.sidebar_collapsed,
        sidebar_section_split: app.shell_state.sidebar_section_split,
        tab_drag_insert_index: app.tab_drag_index,
        selected_workspace_id: app.navigate_target.as_ref(),
        reveal_navigation_workspace: &mut app.shell_state.reveal_navigation_workspace,
        dragged_workspace_id: None,
        workspace_drop_indicator_row: None,
        update_badge,
        cast,
        cast_scroll: &mut app.studio.cast_scroll,
        lot,
        machine_lines: machine_lines.as_ref(),
        hide_stage_space: !app.plain,
        focused_studio: focused_studio.as_ref(),
        studio_tree: studio_tree.as_ref(),
        current_studio: current_studio.as_ref(),
        lot_selected_studio: lot_selected_studio.as_ref(),
        reveal_studio_row: &mut app.shell_state.reveal_studio_row,
        reveal_tree_row: &mut app.shell_state.reveal_tree_row,
        tree_cursor: app.tree_focus.as_ref(),
        tree_focused: app.focus.workspace == crate::focus::WorkspaceRegion::Tree
            && app.focus.modals.is_empty(),
        tree_remembered: app.tree_remembered.as_ref(),
    };
    app.hit_map = crate::chrome::render::render_shell(
        buf,
        layout,
        &app.projection.snapshot,
        &app.shell,
        state,
    );

    let pane_infos = pane_infos(app, layout.pane_surface);
    // Resolved before the cache borrow: the border/title lookups read
    // `app` while `border_cache` mutably borrows `shell_state`.
    let pane_splits = split_borders(app, layout.pane_surface);
    let mut border_titles = pane_infos
        .iter()
        .map(|info| super::pane_geometry::border_label(app, &info.id))
        .collect::<Vec<_>>();
    let border_pane_infos: Vec<crate::chrome::panes::PaneInfo> = pane_infos
        .iter()
        .enumerate()
        .map(|(i, info)| {
            let mut copy = crate::chrome::panes::PaneInfo {
                id: info.id.clone(),
                rect: info.rect,
                borders: info.borders,
                is_focused: info.is_focused,
            };
            if !pane_focus_active && app.studio.active && copy.is_focused {
                copy.is_focused = false;
                let base = border_titles[i].take().unwrap_or_else(|| copy.id.clone());
                let suffix = if copy.rect.width >= 48 {
                    " [unfocused · Esc returns]"
                } else {
                    " [unfocused]"
                };
                border_titles[i] = Some(format!("{base}{suffix}"));
            }
            copy
        })
        .collect();
    crate::chrome::panes_draw::render_pane_borders_cached(
        &mut app.shell_state.border_cache,
        buf,
        app.shell.pane_borders,
        app.shell.pane_gaps,
        &border_pane_infos,
        &pane_splits,
        &|id| {
            pane_infos
                .iter()
                .position(|info| info.id == id)
                .and_then(|index| border_titles[index].clone())
        },
        &app.shell.palette,
    );
    if app.shell.mouse_capture
        && let Some(tab) = app.projection.snapshot.focused_tab_id.as_ref()
        && let Some(root) = app
            .projection
            .layouts
            .get(tab)
            .filter(|t| t.zoomed.is_none())
            .and_then(|t| t.root.as_ref())
    {
        let frames = pane_infos.iter().map(|p| p.rect).collect::<Vec<_>>();
        app.hit_map.pane_splits = root
            .layout(app.focused.as_deref())
            .0
            .splits(layout.pane_surface)
            .into_iter()
            .filter_map(|split| {
                let rect = super::layout_mouse::split_hit_rect(
                    &split,
                    app.shell.pane_borders.draws_borders(),
                    app.shell.pane_gaps,
                    &frames,
                )?;
                Some(super::layout_mouse::SplitHit {
                    rect,
                    split,
                    root: root.clone(),
                    tab: tab.clone(),
                })
            })
            .collect();
    }
    let inner = pane_inner(app, area.width, area.height);
    let mut cursor = CursorWish {
        pos: None,
        anchor: None,
        shape: CursorShape::Default,
        blinking: true,
    };
    let hide_cursor = app.hide_pane_cursor(area.width);
    let scrollbars = app.shell.pane_scrollbars;
    // The reference rebuilds the whole frame as ratatui cells whenever a
    // selection/search highlight or the copy-mode cursor paints
    // (wire.rs u16_to_modifier drops the extended underline bits), so the
    // styled underlines of every already-painted pane cell collapse to
    // single. Track whether this frame hits any of those paths.
    let mut pane_overlay_flatten = app
        .selection
        .as_ref()
        .is_some_and(|selection| selection.is_visible())
        || app
            .copy_mode
            .as_ref()
            .is_some_and(|copy_mode| !copy_mode.search_matches.is_empty());
    let probe = app.probe.clone();
    // Cache-blitted pane surfaces — their wide-tail slots arrive
    // pre-marked, so the final marking pass can skip their interiors.
    let mut cached_rects = Vec::with_capacity(pane_infos.len());
    for info in &pane_infos {
        let Some((content, track, scroll)) = app
            .displayed_pane(&info.id)
            .map(|pane| inner_for(pane, info, scrollbars))
        else {
            continue;
        };
        let hit = crate::chrome::render::PaneHit {
            rect: info.rect,
            inner_rect: content,
            scrollbar_rect: track,
            scroll,
            pane_id: info.id.clone(),
        };
        // The reference's `apply_pane_surface`: the frame's metrics sync
        // the copy snapshot before its highlights and cursor paint.
        app.sync_copy_surface(&info.id, content, scroll);
        let copy_coherent = super::copy_render::copy_surface_coherent(app.copy_mode.as_ref(), &hit);
        let suppressed = !info.is_focused || hide_cursor;
        let Some(pane) = app.displayed_pane_mut(&info.id) else {
            continue;
        };
        let (wish, cells_read, used_cache) =
            draw_terminal_counted(pane, content, buf, _now, suppressed);
        probe.cells_painted.fetch_add(cells_read, Ordering::Relaxed);
        if used_cache {
            cached_rects.push(content);
        }
        // The reference paints non-current matches over the already
        // composed pane surface, then the selection, then the current
        // match and the copy cursor.
        if copy_coherent {
            super::copy_render::render_copy_search_highlights(
                buf,
                app.copy_mode.as_ref(),
                &hit,
                &app.shell.palette,
                false,
            );
        }
        let stale_copy_projection = !copy_coherent
            && app
                .copy_mode
                .as_ref()
                .is_some_and(|copy_mode| copy_mode.pane_id == info.id)
            && app
                .selection
                .as_ref()
                .is_some_and(|selection| selection.pane_id == info.id);
        if !stale_copy_projection {
            crate::chrome::selection_highlight::render_selection_highlight(
                app.selection.as_ref(),
                buf,
                &info.id,
                content,
                scroll,
                &app.shell.palette,
                &app.host_facts.theme,
            );
        }
        if copy_coherent {
            super::copy_render::render_copy_search_highlights(
                buf,
                app.copy_mode.as_ref(),
                &hit,
                &app.shell.palette,
                true,
            );
            if app.mode == Mode::Copy
                && let Some(copy_mode) = app.copy_mode.as_ref()
            {
                pane_overlay_flatten |= super::copy_render::render_copy_cursor(
                    buf,
                    copy_mode,
                    &hit,
                    &app.shell.palette,
                );
            }
        }
        if info.is_focused {
            cursor = wish;
        }
        if !info.id.is_empty() {
            app.hit_map.panes.push(hit);
        }
        crate::chrome::panes_draw::render_pane_scrollbar(
            buf,
            scroll,
            track,
            &app.shell.palette,
            info.is_focused,
        );
    }
    // U2: the opening page covers the pane surface while it is up —
    // the terminals beneath stay attached, overlays still paint on top.
    if app.opening_visible() {
        let frame = app.opening_frame();
        let stopped_cmd = app.server_stopped_command(Some(layout.pane_surface.width as usize));
        crate::opening::render_opening_server(
            buf,
            layout.pane_surface,
            &app.shell.palette,
            app.opening_opts(),
            frame,
            stopped_cmd.as_deref(),
        );
        app.arm_opening_anim(_now);
    } else if app.opening.ready() {
        // No page, no run: leaving before the last frame finishes it,
        // so Esc-and-back and resizes show the static page. While the
        // entry decision still waits the run is untouched — the card
        // plays its animation when it is finally judged.
        app.opening_finish_anim();
    }
    let (mode_bar, overlay_painted) = draw_overlays::render_overlays_and_cursor(
        app,
        buf,
        area,
        &layout,
        inner,
        _now,
        cursor,
        pane_overlay_flatten,
    );
    // Any overlay composited over a pane surface may have written over a
    // wide-tail slot — a full rescan is required. Without overlays the
    // cache-blitted interiors are already marked and the walk skips them.
    if overlay_painted {
        mark_wide_tail_slots(buf);
    } else {
        mark_wide_tail_slots_skipping(buf, &cached_rects, mode_bar);
    }
}

/// Bits 12-15 of the ratatui `Modifier` carry the underline kind — the
/// reference `UNDERLINE_STYLE_MASK`/`SHIFT` in protocol/wire.rs.
const UNDERLINE_STYLE_SHIFT: u16 = 12;

#[path = "draw_style.rs"]
mod draw_style;
use draw_style::*;
