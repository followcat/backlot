// Derived from herdr src/client/shell/mouse.rs (Apache-2.0).
//! Mouse routing over the last frame's `ShellHitMap`, in the
//! reference's press order: sidebar toggle → dividers → scrollbars →
//! sort toggle → machine rows → tab arrows → group toggles → workspace
//! press → tab press → agent rows → panes. Scroll wheels move the
//! workspace/agent lists by the configured `mouse_scroll_lines`…
//! reference scrolls by 1 regardless of the setting for the lists; the
//! config lines only apply to pane scroll.
//!
//! Input has one focused connection: a "focus" resolves to the attached
//! terminal behind a workspace/tab/agent hit, and pane wheel scroll is
//! the VT's own viewport scroll (`scroll_delta`) — or SGR bytes to the
//! program when it enabled mouse tracking.

use ratatui::layout::{Position, Rect};
use std::time::Instant;

use crate::chrome::scroll;

use super::input::{MouseButton, MouseEventKind, MouseInput};
use super::{App, Mode};

/// The reference's `super::contains`.
pub(super) fn contains(rect: Rect, point: (u16, u16)) -> bool {
    rect.contains(Position {
        x: point.0,
        y: point.1,
    })
}

/// What a left-press grabbed — the reference's `ClientChromeDrag`
/// subset hc-tui implements.
#[derive(Clone)]
pub(crate) enum ChromeDrag {
    PaneSplit {
        hit: Box<super::layout_mouse::SplitHit>,
        edit: Box<crate::operations::Edit>,
        grab: i32,
        last: Option<Instant>,
        core: String,
    },
    SidebarWidth,
    SidebarSection,
    WorkspaceScrollbar {
        grab_row_offset: u16,
    },
    AgentScrollbar {
        grab_row_offset: u16,
    },
    PaneScrollbar {
        pane_id: String,
        grab_row_offset: Option<u16>,
    },
    NavigatorScrollbar {
        grab_row_offset: u16,
    },
    /// The studio panel's body scrollbar.
    PanelScrollbar {
        grab_row_offset: u16,
    },
    /// The beside dock's left border — dragging it resizes the dock
    /// within `28..=48` (S5 §2).
    PanelWidth,
    /// The Hollywood layer's body scrollbar.
    HollywoodScrollbar {
        grab_row_offset: u16,
    },
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
        self.sync_focus();
        if self.popup_mouse(&mouse) {
            self.sched.mark_dirty(Instant::now());
            return;
        }
        if self.peek_mouse(&mouse) {
            self.sched.mark_dirty(Instant::now());
            return;
        }
        if self.pane_mouse_gesture.is_some() && self.route_pane_mouse(mouse) {
            self.sched.mark_dirty(Instant::now());
            return;
        }
        // A chrome grab owns drag and release even when the pointer moves
        // into a different pane's content or outside its original track.
        if self.chrome_drag.is_some() {
            match mouse.kind {
                MouseEventKind::Drag(MouseButton::Left) => self.mouse_drag((mouse.col, mouse.row)),
                MouseEventKind::Up(MouseButton::Left) => self.mouse_release((mouse.col, mouse.row)),
                _ => {}
            }
            if matches!(
                mouse.kind,
                MouseEventKind::Drag(MouseButton::Left) | MouseEventKind::Up(MouseButton::Left)
            ) {
                self.sched.mark_dirty(Instant::now());
                return;
            }
        }
        if self.dialog.is_some() {
            self.dialog_mouse(mouse);
            return;
        }
        // The S3 studio dialogs are modals the same way: field/button
        // hits route, a press outside cancels (§2).
        if self.studio_dialog.is_some() {
            self.studio_dialog_mouse(mouse);
            self.sched.mark_dirty(Instant::now());
            return;
        }
        // The New Studio dialog is a modal like the dialogs above:
        // its cells activate on press, a click outside cancels.
        if self.new_studio.is_some() {
            self.new_studio_mouse(mouse);
            self.sched.mark_dirty(Instant::now());
            return;
        }
        // The Hollywood setup dialog is a modal the same way — its
        // cells and list activate on press, outside cancels, the
        // wheel walks the open model list.
        if self.hw_switch.is_some() {
            self.hw_switch_mouse(mouse);
            self.sched.mark_dirty(Instant::now());
            return;
        }
        // The Task & Delivery Sheet is modal the same way: a click
        // outside its frame cancels, the wheel scrolls its body.
        if self.sheet.is_some() {
            self.sheet_mouse(mouse);
            self.sched.mark_dirty(Instant::now());
            return;
        }
        // The context menu overlays everything but a dialog; it owns move
        // highlight and press activation until it closes.
        if self.menu.is_some() {
            self.menu_mouse(mouse);
            self.sched.mark_dirty(Instant::now());
            return;
        }
        // The failure toast's `copy cmd` chip copies, a press on the
        // card dismisses (§3.3) — other presses route normally.
        if mouse.kind == MouseEventKind::Down(MouseButton::Left)
            && self.action_toast_press((mouse.col, mouse.row))
        {
            self.sched.mark_dirty(Instant::now());
            return;
        }
        if self.help {
            self.handle_help_mouse(mouse);
            self.sched.mark_dirty(Instant::now());
            return;
        }
        // The navigator overlay owns every mouse event while open —
        // `mouse.rs`'s Navigator branch routes row/search/scrollbar
        // hits, outside dismissal and the wheel before any chrome hit.
        if self.navigator.is_some() {
            self.handle_navigator_mouse(mouse);
            self.sched.mark_dirty(Instant::now());
            return;
        }
        // The Settings overlay owns its popup: tab/choice/button hits,
        // outside-press cancel — the reference's Settings mouse branch.
        if self.update_modal.is_some() {
            self.handle_update_mouse(mouse);
            return;
        }
        if self.settings.is_some() {
            self.handle_settings_mouse(mouse);
            self.sched.mark_dirty(Instant::now());
            return;
        }
        // The Hollywood layer draws over the panel, so its hits win
        // theirs: a click on the strip expands it; inside the expanded
        // panel rows act like Enter, buttons run, the wheel scrolls.
        // A press outside the expanded panel falls through to whatever
        // is under it and leaves the panel open — unless the "Show me"
        // view is open: it owns the pointer until an outside click
        // closes it.
        if self.hollywood.view.is_some() || self.hollywood_hits.contains((mouse.col, mouse.row)) {
            self.hollywood_mouse(mouse);
            self.sched.mark_dirty(Instant::now());
            return;
        }
        // A10.1: a press outside the expanded Call Sheet collapses
        // it; the press itself keeps routing (`hollywood/mouse.rs`).
        self.hollywood_outside_press(mouse.kind);
        // The open panel owns its rect: rows act like Enter, the wheel
        // scrolls, the scrollbar grabs. A press outside a focused panel
        // hands focus back to the panes and routes normally.
        if self.panel_mode_at(self.area.0) != crate::panel::PanelMode::Closed {
            if contains(self.panel_hits.area, (mouse.col, mouse.row)) {
                self.panel_mouse(mouse);
                self.sched.mark_dirty(Instant::now());
                return;
            }
            if mouse.kind == MouseEventKind::Down(MouseButton::Left) && self.panel.focused {
                self.set_workspace_focus(crate::focus::WorkspaceRegion::Terminal);
            }
        }
        if contains(self.take_control_hit, (mouse.col, mouse.row)) {
            if mouse.kind == MouseEventKind::Down(MouseButton::Left) {
                self.pane.take_control();
                self.sched.mark_dirty(Instant::now());
            }
            return;
        }
        // The toast's `notification_toast` hit — a click is `prefix+o`.
        if contains(self.notification_hit, (mouse.col, mouse.row)) {
            if mouse.kind == MouseEventKind::Down(MouseButton::Left) {
                self.focus_visible_notification();
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
        if self.tab_press.is_some()
            && matches!(
                mouse.kind,
                MouseEventKind::Drag(MouseButton::Left) | MouseEventKind::Up(MouseButton::Left)
            )
        {
            match mouse.kind {
                MouseEventKind::Drag(_) => self.mouse_drag((mouse.col, mouse.row)),
                _ => self.mouse_release((mouse.col, mouse.row)),
            }
            self.sched.mark_dirty(Instant::now());
            return;
        }
        // Down(Left) prologue: every left press reaching the main
        // dispatch retires the selection family first — the reference's
        // `Down(Left)` arm before any hit routing.
        let previous_pane_click = if mouse.kind == MouseEventKind::Down(MouseButton::Left) {
            self.clear_selection_for_press()
        } else {
            None
        };
        if mouse.kind == MouseEventKind::Down(MouseButton::Left)
            && self.grab_split((mouse.col, mouse.row))
        {
            return;
        }
        // An in-progress selection owns the pointer wherever it roams:
        // drags extend (clamped, edge-autoscrolling), releases finish and
        // copy, and the wheel scrolls the selection's own pane — none of
        // it reaches the pane under the cursor or chrome routing.
        if mouse.kind == MouseEventKind::Drag(MouseButton::Left)
            && let Some(hit) = self.active_selection_pane()
        {
            self.update_selection_drag(&hit, mouse.col, mouse.row);
            self.sched.mark_dirty(Instant::now());
            return;
        }
        if mouse.kind == MouseEventKind::Up(MouseButton::Left) && self.word_selection.is_some() {
            self.finish_word_selection();
            self.sched.mark_dirty(Instant::now());
            return;
        }
        if mouse.kind == MouseEventKind::Up(MouseButton::Left) && self.selection.is_some() {
            self.finish_selection();
            self.sched.mark_dirty(Instant::now());
            return;
        }
        if self.scroll_in_progress_selection(mouse) {
            self.sched.mark_dirty(Instant::now());
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
                self.mouse_press(&mouse, previous_pane_click);
            }
            MouseEventKind::Down(MouseButton::Right) => {
                self.context_menu_press(point);
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
            // U1: the cast section shares the agents body rect — its
            // wheel scrolls `studio.cast_scroll` instead.
            let scroll = if self.studio.active {
                &mut self.studio.cast_scroll
            } else {
                &mut self.shell_state.agent_scroll
            };
            let next = if dir < 0 {
                scroll.saturating_sub(1)
            } else {
                scroll.saturating_add(1).min(hits.agent_max_scroll)
            };
            if next != *scroll {
                *scroll = next;
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
        // NextTab binding (focus the adjacent tab's remembered pane).
        if hits.tabs.iter().any(|(rect, _)| contains(*rect, point))
            || contains(hits.tab_scroll_left, point)
            || contains(hits.tab_scroll_right, point)
            || contains(hits.new_tab, point)
        {
            self.focus_adjacent_tab(dir);
        }
    }
}
