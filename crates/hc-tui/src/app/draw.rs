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
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::widgets::{Paragraph, Widget};

use super::pane::Paint;
pub(crate) use super::pane_geometry::pane_inner;
use super::pane_geometry::{focused_tab_count, inner_for, pane_infos, split_borders};
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
    let layout = app.shell.layout_config().layout(
        area.width,
        area.height,
        collapsed,
        focused_tab_count(app),
        app.sidebar_width,
    );
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
        || (app.mode == Mode::Navigate && app.area.0 <= app.shell.mobile_width_threshold);
    let scrollbars = app.shell.pane_scrollbars;
    for info in &pane_infos {
        let Some(pane) = app.displayed_pane_mut(&info.id) else {
            continue;
        };
        let (content, track, scroll) = inner_for(pane, info, scrollbars);
        let suppressed = !info.is_focused || hide_cursor;
        let wish = draw_terminal(pane, content, buf, _now, suppressed);
        if info.is_focused {
            cursor = wish;
        }
        if !info.id.is_empty() {
            app.hit_map.panes.push(crate::chrome::render::PaneHit {
                rect: info.rect,
                inner_rect: content,
                scrollbar_rect: track,
                scroll,
                pane_id: info.id.clone(),
            });
        }
        crate::chrome::panes_draw::render_pane_scrollbar(
            buf,
            scroll,
            track,
            &app.shell.palette,
            info.is_focused,
        );
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
    let mode_bar = if app.help || app.dialog.is_some() || app.menu.is_some() || mobile_navigate {
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
                Mode::Resize => crate::chrome::modebar::ClientShellMode::Resize,
            },
            app.keyboard_locked,
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
    // The mobile switcher IS the Navigate surface under the threshold —
    // `composition.rs` renders it over the whole frame.
    if layout.mobile_header.height > 0
        && app.mode == Mode::Navigate
        && !app.mobile_switcher_suspended
    {
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
    super::notices::render(app, buf, area);
    if let Some(bar) = mode_bar {
        for (offset, cell) in mode_bar_cells.into_iter().enumerate() {
            *buf.cell_mut((bar.x + offset as u16, bar.y))
                .expect("bar cell") = cell;
        }
    }
    app.want_cursor = CursorWish {
        pos: cursor.pos.map(|(x, y)| (inner.x + x, inner.y + y)),
        anchor: cursor.anchor.map(|(x, y)| (inner.x + x, inner.y + y)),
        ..cursor
    };
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
}

fn draw_terminal(
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
                // Covered by the wide head at col-1: paint nothing.
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
