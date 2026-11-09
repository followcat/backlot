//! Frame composition: sidebar half, the pane's VT screen, a status
//! line and the help/selector overlays. One `layout` function owns the
//! pane geometry — the attach allocation and the renderer see the
//! exact same inner rect, so no row or column is silently clipped.
//!
//! VT cells map to ratatui by *physical column index* — the frame
//! emits one cell per column including spacer tails, so a wide head
//! paints once and its tail cell simply isn't written. Colors resolve
//! through the frame's palette/defaults; metadata text is sanitized
//! before display. Nothing the VT emits is re-escaped to the outer
//! terminal.

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

#[cfg(test)]
#[path = "draw_tests.rs"]
mod tests;

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
        app.presentation
            .acknowledge(&mut app.projection, &pane_id, app.outer_focused);
    }
    let collapsed = app.shell_state.sidebar_collapsed;
    let layout = app.chrome_layout(area.width, area.height);
    let state = crate::chrome::render::ShellRenderState {
        machine_diagnostics: &app.shell_state.machine_diagnostics,
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
        sidebar_collapsed: collapsed,
        sidebar_section_split: app.shell_state.sidebar_section_split,
        tab_drag_insert_index: app.tab_drag_index,
        selected_workspace_id: app.navigate_target.as_ref(),
        reveal_navigation_workspace: &mut app.shell_state.reveal_navigation_workspace,
        dragged_workspace_id: None,
        workspace_drop_indicator_row: None,
    };
    app.hit_map = crate::chrome::render::render_shell(
        buf,
        layout,
        &app.projection.snapshot,
        &app.shell,
        state,
    );

    let pane_infos = pane_infos(app, layout.pane_surface);
    crate::chrome::panes_draw::render_pane_borders(
        buf,
        app.shell.pane_borders,
        app.shell.pane_gaps,
        &pane_infos,
        &split_borders(app, layout.pane_surface),
        &|id| super::pane_geometry::border_label(app, id),
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
    let hide_cursor = app.help
        || app.dialog.is_some()
        || app.menu.is_some()
        || app.navigator.is_some()
        || app.settings.is_some()
        || app.panel_fullscreen()
        || app.mode == Mode::Copy
        || (app.mode == Mode::Navigate && app.area.0 <= app.shell.mobile_width_threshold);
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
        let wish = draw_terminal(pane, content, buf, _now, suppressed);
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
    if pane_overlay_flatten {
        for cell in &mut buf.content {
            cell.modifier = Modifier::from_bits_truncate(cell.modifier.bits() & !0xf000);
        }
    }
    let mode_bar_area = if layout.mobile_header.is_empty()
        && app.shell.tab_bar_position == crate::config::TabBarPositionConfig::Bottom
        && !layout.tab_bar.is_empty()
    {
        layout.tab_bar
    } else {
        layout.pane_surface
    };
    let mobile_navigate = !layout.mobile_header.is_empty() && app.mode == Mode::Navigate;
    // Source suppresses the mode bar while any overlay is open, including
    // the context menu — Navigate can be the mode underneath it.
    let mode_bar = if app.help
        || app.dialog.is_some()
        || app.menu.is_some()
        || app.navigator.is_some()
        || app.settings.is_some()
        || mobile_navigate
    {
        None
    } else {
        crate::chrome::modebar::render_mode_bar(
            buf,
            mode_bar_area,
            match app.mode {
                Mode::Normal if app.prefix.armed() => {
                    crate::chrome::modebar::ClientShellMode::Prefix
                }
                Mode::Normal => crate::chrome::modebar::ClientShellMode::Terminal,
                Mode::Navigate => crate::chrome::modebar::ClientShellMode::Navigate,
                Mode::Copy => crate::chrome::modebar::ClientShellMode::Copy,
                Mode::Resize => crate::chrome::modebar::ClientShellMode::Resize,
            },
            app.keyboard_locked,
            app.copy_mode.as_ref(),
            None,
            false,
            &app.live,
            &app.shell.palette,
        )
    };
    if mode_bar == Some(layout.tab_bar) {
        app.hit_map.tabs.clear();
        app.hit_map.new_tab = Rect::default();
        app.hit_map.tab_scroll_left = Rect::default();
        app.hit_map.tab_scroll_right = Rect::default();
    }
    // The studio panel: beside the panes when it fits, else over the
    // whole frame — always under the popup/notice/overlay stack so
    // dialogs and action notices still win.
    app.panel_hits = if app.panel.open() {
        let rect = if app.panel_beside(area.width) {
            let width = app.panel_width();
            Rect::new(area.x + area.width - width, area.y, width, area.height)
        } else {
            area
        };
        let summary = app.panel_summary();
        let hits = crate::panel::render(
            buf,
            rect,
            summary.as_ref(),
            crate::panel::stage_running(&app.view),
            &app.panel,
            &app.shell.palette,
        );
        if hits.working_drawn {
            // Arm only when nothing is pending — a redraw before the
            // deadline must not push it out or the spin starves.
            app.panel_anim_deadline
                .get_or_insert(_now + super::panel::PANEL_SPIN);
        } else {
            app.panel_anim_deadline = None;
        }
        hits
    } else {
        app.panel_anim_deadline = None;
        crate::panel::PanelRender::default()
    };
    // The mobile switcher IS the Navigate surface under the threshold —
    // `composition.rs` renders it over the whole frame, rebuilding the
    // wire frame as ratatui cells (dropping extended underline bits).
    if layout.mobile_header.height > 0
        && app.mode == Mode::Navigate
        && !app.mobile_switcher_suspended
    {
        for cell in &mut buf.content {
            cell.modifier = Modifier::from_bits_truncate(cell.modifier.bits() & !0xf000);
        }
        crate::chrome::mobile_switcher::render_mobile_switcher(
            buf,
            area,
            &app.projection.snapshot,
            &app.projection.endpoints,
            &app.projection.active_endpoint_id,
            &app.shell,
            app.navigate_target.as_ref(),
            &mut app.shell_state.mobile_switcher_scroll,
            &mut app.shell_state.reveal_mobile_workspace,
            &mut app.hit_map,
        );
    }
    app.help_hits = if app.help {
        // Source overlays reconstruct the wire frame as ratatui cells first
        // (wire.rs u16_to_modifier drops extended underline bits), then dim
        // the whole background before the panel removes DIM in its own area.
        for cell in &mut buf.content {
            cell.modifier = Modifier::from_bits_truncate(cell.modifier.bits() & !0xf000);
            cell.set_style(cell.style().add_modifier(Modifier::DIM));
        }
        crate::chrome::help::render_help_overlay(
            buf,
            &app.help_state,
            &app.live,
            &app.shell.palette,
        )
        .unwrap_or_default()
    } else {
        crate::chrome::help::HelpRender::default()
    };
    let mut config_covered = Vec::new();
    if let Some(diagnostic) = app.shell.startup_config_diagnostic.as_deref() {
        // The reference composes the diagnostic card through the ratatui
        // rebuild, dropping extended underline bits first.
        for cell in &mut buf.content {
            cell.modifier = Modifier::from_bits_truncate(cell.modifier.bits() & !0xf000);
        }
        let diagnostic_area = if layout.mobile_header.is_empty() {
            area
        } else {
            layout.pane_surface
        };
        crate::chrome::config_diagnostic::render(
            buf,
            diagnostic_area,
            diagnostic,
            &app.shell.palette,
            |rect| config_covered.push(rect),
        );
    }
    // `composition.rs::restore_mode_bar`: the bar row is repainted over
    // the toast — save its cells, draw notices, then put them back.
    let mode_bar_cells: Vec<ratatui::buffer::Cell> = mode_bar
        .map(|bar| {
            (bar.x..bar.x + bar.width)
                .map(|x| buf[(x, bar.y)].clone())
                .collect()
        })
        .unwrap_or_default();
    // The `copied to clipboard` card sits under the popup and the
    // notices — the reference composes it after its toast and before
    // the popup. The notification hit from the last frame is the toast
    // it must not overlap.
    if let Some(feedback) = app.copy_feedback.as_ref() {
        // Same ratatui rebuild as the other overlays: extended underline
        // bits drop before the card paints.
        for cell in &mut buf.content {
            cell.modifier = Modifier::from_bits_truncate(cell.modifier.bits() & !0xf000);
        }
        let feedback_area = if layout.mobile_header.is_empty() {
            layout.pane_surface
        } else {
            area
        };
        let base = u16::from(!config_covered.is_empty());
        let offset = crate::chrome::copy_feedback::copy_feedback_offset_for_toast(
            feedback_area,
            &feedback.message,
            base,
            app.shell.clipboard_toast_position,
            app.notification_hit,
        );
        crate::chrome::copy_feedback::render_copy_feedback_buffer(
            buf,
            feedback_area,
            &feedback.message,
            offset,
            app.shell.clipboard_toast_position,
            &app.shell.palette,
        );
    }
    app.draw_popups(buf, _now);
    super::notices::render(app, buf, area);
    // The restore would repaint the bar over a full-screen panel —
    // skipped then; beside the panes the bar is never covered anyway.
    if !app.panel_fullscreen()
        && let Some(bar) = mode_bar
    {
        for (offset, cell) in mode_bar_cells.into_iter().enumerate() {
            *buf.cell_mut((bar.x + offset as u16, bar.y))
                .expect("bar cell") = cell;
        }
    }
    // A popup owns the host cursor while open; `draw_popups` already
    // stored its surface-relative wish in `app.want_cursor`.
    if app.popup.is_none() {
        app.want_cursor = CursorWish {
            pos: cursor.pos.map(|(x, y)| (inner.x + x, inner.y + y)),
            anchor: cursor.anchor.map(|(x, y)| (inner.x + x, inner.y + y)),
            ..cursor
        };
    }
    if mode_bar.is_some_and(|bar| app.want_cursor.pos.is_some_and(|(_, y)| y == bar.y)) {
        app.want_cursor.pos = None;
        app.want_cursor.anchor = None;
        app.want_cursor.shape = CursorShape::Default;
    }
    if app.want_cursor.pos.is_some_and(|(x, y)| {
        config_covered
            .iter()
            .any(|rect| rect.contains(ratatui::layout::Position::new(x, y)))
    }) {
        app.want_cursor.pos = None;
    }
    if let Some(cursor) = app.help_hits.cursor.as_ref() {
        app.want_cursor = CursorWish {
            pos: cursor.visible.then_some((cursor.x, cursor.y)),
            anchor: Some((cursor.x, cursor.y)),
            shape: CursorShape::Default,
            blinking: true,
        };
    }
    // `render_client_overlay` draws the overlay over the notices and
    // the restored mode bar; the Navigator variant is the only one the
    // reference does not dim the background for.
    app.navigator_hits = if let Some(navigator) = app.navigator.as_ref() {
        // The source composes overlays by rebuilding the wire frame as
        // ratatui cells (wire.rs u16_to_modifier drops extended underline
        // bits); it does not dim the background for the navigator.
        for cell in &mut buf.content {
            cell.modifier = Modifier::from_bits_truncate(cell.modifier.bits() & !0xf000);
        }
        crate::chrome::navigator_render::render_navigator_overlay(
            buf,
            navigator,
            &app.projection.endpoints,
            &app.projection.active_endpoint_id,
            &app.shell.palette,
        )
        .unwrap_or_default()
    } else {
        crate::chrome::navigator_render::NavigatorRender::default()
    };
    if let Some(cursor) = app.navigator_hits.cursor.as_ref() {
        app.want_cursor = CursorWish {
            pos: cursor.visible.then_some((cursor.x, cursor.y)),
            anchor: Some((cursor.x, cursor.y)),
            shape: CursorShape::Default,
            blinking: true,
        };
    }
    // The Settings overlay dims its background like help/dialogs.
    app.settings_hits = if let Some(settings) = app.settings.as_ref() {
        for cell in &mut buf.content {
            cell.modifier = Modifier::from_bits_truncate(cell.modifier.bits() & !0xf000);
            cell.set_style(cell.style().add_modifier(Modifier::DIM));
        }
        crate::chrome::settings_render::render_settings_overlay(buf, settings, &app.shell.palette)
            .unwrap_or_default()
    } else {
        crate::chrome::settings_render::SettingsRender::default()
    };
    app.render_dialog(buf);
    // The context menu is the topmost shell overlay; the reference
    // suppresses the host cursor while it is open.
    if let Some(menu) = &app.menu {
        // The source composes overlays by rebuilding the wire frame as
        // ratatui cells (wire.rs u16_to_modifier drops extended underline
        // bits); it does not dim the background for the context menu.
        for cell in &mut buf.content {
            cell.modifier = Modifier::from_bits_truncate(cell.modifier.bits() & !0xf000);
        }
        if let Some(rendered) =
            crate::chrome::menu::render_context_menu(buf, menu, &app.shell.palette)
        {
            app.hit_map.context_menu_rows = rendered.rows;
        }
        app.want_cursor.pos = None;
        app.want_cursor.anchor = None;
    } else {
        app.hit_map.context_menu_rows.clear();
    }
    // Derived from herdr protocol/render_ansi.rs::frame_with_drawn_cursor.
    if app.shell.draw_host_cursor
        && let Some(position) = app.want_cursor.pos.take()
    {
        app.last_visible_cursor = Some(position);
        buf[position].modifier ^= Modifier::REVERSED;
        app.want_cursor.anchor = Some(position);
    }
    mark_wide_tail_slots(buf);
}

/// Wide-grapheme tail slots: a cell covered by a multi-cell head in the
/// final composite is marked `CellDiffOption::Skip`. The option
/// participates in `Cell::eq`, so the slot stays suppressed while the head
/// covers it but becomes a real diff once the head narrows — the reference
/// emitter's invalidated-width rule (render_ansi.rs `affected_width`),
/// without which an orphaned host cell keeps stale attributes. Marking
/// after compositing — rather than while panes draw — keeps overlay writes
/// over a tail slot emit-ready; stale marks on uncovered cells are cleared
/// in the same pass.
pub(super) fn mark_wide_tail_slots(buf: &mut Buffer) {
    let width = buf.area.width;
    for y in 0..buf.area.height {
        let mut covered_until = 0u16;
        for x in 0..width {
            let cell = &mut buf[(x, y)];
            if x < covered_until {
                cell.set_diff_option(ratatui::buffer::CellDiffOption::Skip);
                continue;
            }
            if matches!(cell.diff_option, ratatui::buffer::CellDiffOption::Skip) {
                cell.set_diff_option(ratatui::buffer::CellDiffOption::None);
                #[allow(deprecated)]
                {
                    cell.set_skip(false);
                }
            }
            covered_until = covered_until.max(x + cell.cell_width());
        }
    }
}

pub(super) fn draw_terminal(
    pane: &mut super::pane::Pane,
    inner: Rect,
    buf: &mut Buffer,
    now: Instant,
    suppressed: bool,
) -> CursorWish {
    let no_cursor = CursorWish {
        pos: None,
        anchor: None,
        shape: CursorShape::Default,
        blinking: true,
    };
    if inner.is_empty() {
        return no_cursor;
    }
    let paint = pane.paint();
    let Some(vt) = pane.vt() else {
        // No live VT: the attach is opening/down/gone — say so instead
        // of leaving a black hole. Queued input counts ride along.
        let (pending, dropped) = pane.input_backlog();
        let mut label = pane.status_label(now);
        if pending > 0 || dropped > 0 {
            label.push_str(&format!(" · {pending} queued, {dropped} dropped"));
        }
        Paragraph::new(label)
            .style(Style::default().fg(Color::DarkGray))
            .render(Rect::new(inner.x, inner.y, inner.width, 1), buf);
        return no_cursor;
    };
    let Ok(frame) = vt.frame(0) else {
        return no_cursor;
    };
    for row in &frame.rows {
        if row.y < 0 || row.y >= inner.height as i32 {
            continue; // scrollback/below the viewport: clipped
        }
        let y = inner.y + row.y as u16;
        // Cells are indexed by physical column — one cell per column,
        // spacer tails included.
        for (col, cell) in row.cells.iter().enumerate() {
            if col >= inner.width as usize {
                break;
            }
            let x = inner.x + col as u16;
            match cell.wide {
                // Covered by the wide head at col-1: paint nothing. `draw`
                // marks the slot Skip after compositing so a later diff
                // re-emits it when the head narrows.
                Wide::SpacerTail => continue,
                // A wide grapheme at the clip edge can't show half —
                // blank it rather than bleed over the border.
                Wide::Wide if col + 1 >= inner.width as usize => {
                    buf.set_string(x, y, " ", cell_style(&frame, cell, &paint));
                    continue;
                }
                _ => {}
            }
            buf.set_string(
                x,
                y,
                if cell.text.is_empty() {
                    " "
                } else {
                    &cell.text
                },
                cell_style(&frame, cell, &paint),
            );
        }
    }
    let pos = if !suppressed && frame.cursor.visible {
        frame
            .cursor
            .position
            .filter(|(cx, cy)| *cx < inner.width && *cy < inner.height)
    } else {
        None
    };
    CursorWish {
        pos,
        anchor: (!suppressed).then_some(frame.cursor.position).flatten(),
        shape: if !suppressed && frame.cursor.position.is_some() && frame.cursor.shape_overridden {
            frame.cursor.shape
        } else {
            CursorShape::Default
        },
        blinking: frame.cursor.blinking,
    }
}

/// Bits 12-15 of the ratatui `Modifier` carry the underline kind — the
/// reference `UNDERLINE_STYLE_MASK`/`SHIFT` in protocol/wire.rs.
const UNDERLINE_STYLE_SHIFT: u16 = 12;

#[path = "draw_style.rs"]
mod draw_style;
use draw_style::*;
