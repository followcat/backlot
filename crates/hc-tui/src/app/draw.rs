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
    if app.pane.has_fresh_paint()
        && let Some(pane_id) = app.pane.target().map(str::to_string)
    {
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
        tab_drag_insert_index: None,
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

    // The pane surface: `apply_pane_chrome` decides the single pane's
    // borders/inner rect; the VT grid paints into the terminal inner.
    let pane_infos = pane_infos(app, layout.pane_surface);
    crate::chrome::panes_draw::render_pane_borders(
        buf,
        app.shell.pane_borders,
        app.shell.pane_gaps,
        &pane_infos,
        &[],
        &|id| {
            app.projection
                .snapshot
                .panes
                .iter()
                .find(|p| p.pane_id == id)
                .and_then(|p| p.label.clone())
        },
        &app.shell.palette,
    );
    let inner = pane_inner(app, area.width, area.height);
    let scroll = app.pane.vt().and_then(|vt| vt.scrollbar()).map(|state| {
        let maximum = state.total.saturating_sub(state.len) as usize;
        crate::chrome::scroll::ScrollMetrics {
            offset_from_bottom: maximum.saturating_sub(state.offset as usize),
            max_offset_from_bottom: maximum,
            viewport_rows: state.len as usize,
        }
    });
    let alternate = app
        .pane
        .vt()
        .and_then(|vt| vt.alt_screen().ok())
        .unwrap_or(false);
    let content = crate::chrome::panes::pane_inner_rect(pane_infos[0].rect, pane_infos[0].borders);
    let (_, track) = crate::chrome::panes::stable_scrollbar_gutter(
        content,
        app.shell.pane_scrollbars,
        alternate,
        scroll,
    );
    if !pane_infos[0].id.is_empty() {
        app.hit_map.panes.push(crate::chrome::render::PaneHit {
            rect: pane_infos[0].rect,
            inner_rect: inner,
            scrollbar_rect: track,
            scroll,
            pane_id: pane_infos[0].id.clone(),
        });
    }
    crate::chrome::panes_draw::render_pane_scrollbar(buf, scroll, track, &app.shell.palette, true);
    let cursor = draw_pane(app, inner, buf, _now);
    let mode_bar_area = if layout.mobile_header.is_empty()
        && app.shell.tab_bar_position == crate::config::TabBarPositionConfig::Bottom
        && !layout.tab_bar.is_empty()
    {
        layout.tab_bar
    } else {
        layout.pane_surface
    };
    let mobile_navigate = !layout.mobile_header.is_empty() && app.mode == Mode::Navigate;
    let mode_bar = if app.help || mobile_navigate {
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
            },
            false,
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
    if layout.mobile_header.height > 0 && app.mode == Mode::Navigate {
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
    app.help_hits = if app.app_help() {
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
    super::notices::render(app, buf, area);
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
    if let Some(cursor) = app.help_hits.cursor.as_ref() {
        app.want_cursor = CursorWish {
            pos: cursor.visible.then_some((cursor.x, cursor.y)),
            anchor: Some((cursor.x, cursor.y)),
            shape: CursorShape::Default,
            blinking: true,
        };
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

/// The terminal-cell inner rect `cols x rows` maps to — the same rect
/// `draw` paints the grid into, so the attach allocation and the
/// paintable area are identical by construction. Mirrors the `draw`
/// path: shell layout → `apply_pane_chrome` → `pane_inner_rect` →
/// `stable_scrollbar_gutter`.
pub(crate) fn pane_inner(app: &mut App, cols: u16, rows: u16) -> Rect {
    let collapsed = app.shell_state.sidebar_collapsed;
    let layout = app.shell.layout_config().layout(
        cols,
        rows,
        collapsed,
        focused_tab_count(app),
        app.sidebar_width,
    );
    let infos = pane_infos(app, layout.pane_surface);
    let inner = crate::chrome::panes::pane_inner_rect(infos[0].rect, infos[0].borders);
    let alt = app
        .pane
        .vt()
        .and_then(|vt| vt.alt_screen().ok())
        .unwrap_or(false);
    crate::chrome::panes::stable_scrollbar_gutter(inner, app.shell.pane_scrollbars, alt, None).0
}

/// The focused workspace's tab count — what
/// `hide_tab_bar_when_single_tab` measures.
fn focused_tab_count(app: &App) -> usize {
    app.projection
        .snapshot
        .focused_workspace_id
        .as_deref()
        .map(|ws| {
            app.projection
                .snapshot
                .tabs
                .iter()
                .filter(|t| t.workspace_id == ws)
                .count()
        })
        .unwrap_or(0)
}

/// The single focused `PaneInfo` `apply_pane_chrome` resolves —
/// hc-tui attaches one terminal across `pane_surface`.
fn pane_infos(app: &App, surface: Rect) -> Vec<crate::chrome::panes::PaneInfo> {
    crate::chrome::panes::apply_pane_chrome(
        vec![crate::chrome::panes::PaneInfo {
            id: app
                .projection
                .snapshot
                .focused_pane_id
                .clone()
                .unwrap_or_default(),
            rect: surface,
            borders: ratatui::widgets::Borders::NONE,
            is_focused: true,
        }],
        app.shell.pane_borders,
        app.shell.pane_gaps,
        app.shell.pane_outer_borders,
    )
}

/// Paints the retained VT grid into `inner` — observer grids clip/pad
/// to it without a resize. Returns the outer cursor wish; selector or
/// help overlays suppress it. A pane with no live VT shows the attach
/// status line instead of an empty grid.
fn draw_pane(app: &mut App, inner: Rect, buf: &mut Buffer, now: Instant) -> CursorWish {
    let no_cursor = CursorWish {
        pos: None,
        anchor: None,
        shape: CursorShape::Default,
        blinking: true,
    };
    if inner.is_empty() {
        return no_cursor;
    }
    let paint = app.app_pane().paint();
    let Some(vt) = app.app_pane().vt() else {
        // No live VT: the attach is opening/down/gone — say so instead
        // of leaving a black hole. Queued input counts ride along.
        let (pending, dropped) = app.app_pane().input_backlog();
        let mut label = app.app_pane().status_label(now);
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
    // Real cursor: position + shape, only when the program shows one
    // and no overlay is up. Navigate-mode previews suppress it — the
    // observed screen's cursor is not ours.
    let suppressed = app.app_help()
        || (*app.app_mode() == Mode::Navigate && app.area.0 <= app.shell.mobile_width_threshold);
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
