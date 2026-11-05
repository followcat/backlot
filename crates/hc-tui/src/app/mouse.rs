// Derived from herdr src/client/shell/mouse.rs (Apache-2.0).
//! Mouse routing over the last frame's `ShellHitMap`, in the
//! reference's press order: sidebar toggle → dividers → scrollbars →
//! sort toggle → machine rows → tab arrows → group toggles → workspace
//! press → tab press → agent rows → panes. Scroll wheels move the
//! workspace/agent lists by the configured `mouse_scroll_lines`…
//! reference scrolls by 1 regardless of the setting for the lists; the
//! config lines only apply to pane scroll.
//!
//! hc-tui is single-attach: a "focus" resolves to the attached
//! terminal behind a workspace/tab/agent hit, and pane wheel scroll is
//! the VT's own viewport scroll (`scroll_delta`) — or SGR bytes to the
//! program when it enabled mouse tracking.

use ratatui::layout::{Position, Rect};
use std::time::Instant;

use crate::chrome::scroll;

use super::input::{MouseButton, MouseEventKind, MouseInput};
use super::{App, Mode};

/// The reference's `super::contains`.
fn contains(rect: Rect, point: (u16, u16)) -> bool {
    rect.contains(Position {
        x: point.0,
        y: point.1,
    })
}

/// What a left-press grabbed — the reference's `ClientChromeDrag`
/// subset hc-tui implements.
#[derive(Debug, Clone, Copy)]
pub(crate) enum ChromeDrag {
    SidebarWidth,
    SidebarSection,
    WorkspaceScrollbar { grab_row_offset: u16 },
    AgentScrollbar { grab_row_offset: u16 },
    PaneScrollbar { grab_row_offset: Option<u16> },
}

/// A workspace row press pending release-or-drag — the reference
/// `ClientWorkspacePress`.
#[derive(Debug, Clone)]
pub(crate) struct WorkspacePress {
    pub endpoint_id: crate::chrome::snapshot::ClientEndpointId,
    pub workspace_id: String,
    pub start_column: u16,
    pub start_row: u16,
}

#[derive(Debug, Clone)]
pub(crate) struct TabPress {
    pub tab_id: String,
    pub start_column: u16,
    pub start_row: u16,
}

/// Double-click window for the sidebar-divider reset.
const DOUBLE_CLICK: std::time::Duration = std::time::Duration::from_millis(350);
/// Press must move this far to become a workspace/tab drag (drag
/// reorder is deferred; the threshold still gates the release-click).
pub(crate) const DRAG_THRESHOLD: u16 = 2;

#[path = "mouse_drag.rs"]
mod drag;

#[path = "mouse_press.rs"]
mod press;

impl App {
    /// `layout.mobile`: under `mobile_width_threshold` the shell hides
    /// the sidebar and the switcher is the Navigate surface.
    fn mobile(&self) -> bool {
        self.area.0 <= self.shell.mobile_width_threshold
    }

    pub(crate) fn on_mouse(&mut self, mouse: MouseInput) {
        if self.pane_mouse_gesture.is_some() && self.route_pane_mouse(mouse) {
            self.sched.mark_dirty(Instant::now());
            return;
        }
        if self.help {
            self.handle_help_mouse(mouse);
            self.sched.mark_dirty(Instant::now());
            return;
        }
        if contains(self.take_control_hit, (mouse.col, mouse.row)) {
            if mouse.kind == MouseEventKind::Down(MouseButton::Left) {
                self.pane.take_control();
                self.sched.mark_dirty(Instant::now());
            }
            return;
        }
        // The mobile switcher consumes every event in Navigate mode —
        // `mobile.rs` routes scroll/close/targets and eats the rest.
        if self.mobile() && self.mode == Mode::Navigate {
            self.mobile_navigate_mouse(mouse);
            self.sched.mark_dirty(std::time::Instant::now());
            return;
        }
        if self.route_pane_mouse(mouse) {
            self.sched.mark_dirty(Instant::now());
            return;
        }
        let point = (mouse.col, mouse.row);
        match mouse.kind {
            MouseEventKind::Drag(MouseButton::Left) => {
                self.mouse_drag(point);
            }
            MouseEventKind::Up(MouseButton::Left) => {
                self.mouse_release(point);
            }
            MouseEventKind::Down(MouseButton::Left) => {
                self.mouse_press(point);
            }
            MouseEventKind::ScrollUp => self.mouse_scroll(point, -1),
            MouseEventKind::ScrollDown => self.mouse_scroll(point, 1),
            MouseEventKind::ScrollLeft | MouseEventKind::ScrollRight => {}
            _ => {}
        }
        self.sched.mark_dirty(std::time::Instant::now());
    }

    /// `mobile.rs::handle_navigate_mouse`: wheel moves the switcher
    /// list ±2; the close chip cancels; a target press commits.
    fn mobile_navigate_mouse(&mut self, mouse: MouseInput) {
        let point = (mouse.col, mouse.row);
        match mouse.kind {
            MouseEventKind::ScrollUp | MouseEventKind::ScrollDown => {
                let delta: isize = if matches!(mouse.kind, MouseEventKind::ScrollUp) {
                    -2
                } else {
                    2
                };
                let next = (self.shell_state.mobile_switcher_scroll as isize + delta)
                    .max(0)
                    .min(self.hit_map.mobile_max_scroll as isize)
                    as usize;
                self.shell_state.mobile_switcher_scroll = next;
            }
            MouseEventKind::Down(MouseButton::Left) => {
                if contains(self.hit_map.mobile_close, point) {
                    self.on_navigate(super::input::Navigate::Cancel);
                    return;
                }
                let target = self
                    .hit_map
                    .mobile_targets
                    .iter()
                    .find(|(rect, _)| contains(*rect, point))
                    .map(|(_, target)| target.clone());
                if let Some(target) = target {
                    self.on_mobile_target(&target);
                }
            }
            _ => {}
        }
    }

    /// Wheel: agent/workspace list bodies scroll by the reference's
    /// unit step. Over a pane the VT scrolls by `mouse_scroll_lines` —
    /// or the program gets the SGR wheel report when it enabled mouse
    /// tracking.
    fn mouse_scroll(&mut self, point: (u16, u16), dir: isize) {
        let hits = &self.hit_map;
        if contains(hits.agent_body, point) {
            let next = if dir < 0 {
                self.shell_state.agent_scroll.saturating_sub(1)
            } else {
                self.shell_state
                    .agent_scroll
                    .saturating_add(1)
                    .min(hits.agent_max_scroll)
            };
            if next != self.shell_state.agent_scroll {
                self.shell_state.agent_scroll = next;
            }
            return;
        }
        if contains(hits.workspace_body, point) {
            let next = if dir < 0 {
                self.shell_state.workspace_scroll.saturating_sub(1)
            } else {
                self.shell_state
                    .workspace_scroll
                    .saturating_add(1)
                    .min(hits.workspace_max_scroll)
            };
            if next != self.shell_state.workspace_scroll {
                self.shell_state.workspace_scroll = next;
            }
            return;
        }
        // Tab row + its buttons: wheel advances tabs like the reference's
        // NextTab binding (read-only: focus the adjacent tab's pane).
        if hits.tabs.iter().any(|(rect, _)| contains(*rect, point))
            || contains(hits.tab_scroll_left, point)
            || contains(hits.tab_scroll_right, point)
            || contains(hits.new_tab, point)
        {
            self.focus_adjacent_tab(dir);
        }
    }
}

/// `pane_scrollbar_offset`: row → VT scroll offset via the hit's
/// metrics — `offset_from_bottom` space.
fn pane_scrollbar_offset(
    hit: &crate::chrome::render::PaneHit,
    row: u16,
    grab_row_offset: Option<u16>,
) -> Option<usize> {
    let track = hit.scrollbar_rect?;
    let metrics = hit.scroll?;
    (metrics.max_offset_from_bottom > 0).then(|| match grab_row_offset {
        Some(grab) => scroll::scrollbar_offset_from_drag_row(metrics, track, row, grab),
        None => scroll::scrollbar_offset_from_row(metrics, track, row),
    })
}
