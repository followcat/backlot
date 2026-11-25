//! The panel's own mouse dispatch: the title flips the mode, a row
//! selects and acts like Enter, the scrollbar grabs and drags, the
//! wheel scrolls by one.

use ratatui::layout::{Position, Rect};

use super::super::input::{MouseButton, MouseEventKind, MouseInput};
use super::super::mouse::ChromeDrag;
use super::App;
use crate::chrome::scroll;

/// The reference's `super::contains`.
fn contains(rect: Rect, point: (u16, u16)) -> bool {
    rect.contains(Position {
        x: point.0,
        y: point.1,
    })
}

impl App {
    /// The panel's own mouse dispatch while open — the title flips the
    /// mode, a row selects and acts like Enter, the scrollbar
    /// grabs/jumps, the wheel scrolls by one.
    pub(crate) fn panel_mouse(&mut self, mouse: MouseInput) {
        let point = (mouse.col, mouse.row);
        match mouse.kind {
            MouseEventKind::Down(MouseButton::Left) => {
                self.panel.focused = true;
                // The expanded dock's left border is a drag handle
                // (S5 §2) — beside the panes it resizes within
                // `28..=48`. An overlay dock's edge is not a handle.
                if self.panel_mode_at(self.area.0) == crate::panel::PanelMode::Expanded
                    && self.panel_beside(self.area.0)
                    && point.0 == self.panel_hits.area.x
                {
                    self.chrome_drag = Some(ChromeDrag::PanelWidth);
                    return;
                }
                if contains(self.panel_hits.title, point) {
                    self.panel_flip_mode();
                    return;
                }
                if contains(self.panel_hits.scrollbar, point)
                    && let Some(metrics) = self.panel_hits.metrics
                {
                    match scroll::scrollbar_thumb_grab_offset(
                        metrics,
                        self.panel_hits.scrollbar,
                        point.1,
                    ) {
                        Some(grab) => {
                            self.chrome_drag = Some(ChromeDrag::PanelScrollbar {
                                grab_row_offset: grab,
                            });
                        }
                        None => {
                            let offset = scroll::scrollbar_offset_from_row(
                                metrics,
                                self.panel_hits.scrollbar,
                                point.1,
                            );
                            self.panel_scroll_to(
                                metrics.max_offset_from_bottom.saturating_sub(offset),
                                metrics.viewport_rows,
                            );
                        }
                    }
                    return;
                }
                let hit = self
                    .panel_hits
                    .rows
                    .iter()
                    .enumerate()
                    .find(|(_, (rect, _))| contains(*rect, point))
                    .map(|(pos, (_, target))| (pos, target.clone()));
                if let Some((pos, target)) = hit {
                    // Hits draw from `1 + scroll-start` in order — the
                    // absolute row index pins which occurrence of a
                    // repeated `Studio` target was clicked.
                    let start = self
                        .panel_hits
                        .metrics
                        .map(|m| m.max_offset_from_bottom - m.offset_from_bottom)
                        .unwrap_or(0);
                    if target != crate::panel::Target::None {
                        self.panel.selected = Some(target);
                        self.panel.selected_row = Some(1 + start + pos);
                        // `panel_activate` re-arms Compose only on
                        // the `›` row — a click elsewhere rests it.
                        self.panel.editing = false;
                        self.panel.reset_history_nav();
                        self.panel_activate();
                    }
                }
            }
            MouseEventKind::Down(MouseButton::Right) => {
                // A press on the dock's surface claims its keys like
                // the left press does — the menu's Esc must hand them
                // back to the dock, not the focused terminal (S3 R2).
                self.panel.focused = true;
                // Right-click selects the row under the pointer and
                // opens its menu there (S3 §1) — the dock never
                // swallowed right-click before, it just had none.
                let hit = self
                    .panel_hits
                    .rows
                    .iter()
                    .enumerate()
                    .find(|(_, (rect, _))| contains(*rect, point))
                    .map(|(pos, (_, target))| (pos, target.clone()));
                if let Some((pos, target)) = hit
                    && target != crate::panel::Target::None
                {
                    let start = self
                        .panel_hits
                        .metrics
                        .map(|m| m.max_offset_from_bottom - m.offset_from_bottom)
                        .unwrap_or(0);
                    self.panel.selected = Some(target.clone());
                    self.panel.selected_row = Some(1 + start + pos);
                    match &target {
                        crate::panel::Target::Studio(key)
                        | crate::panel::Target::Block(key)
                        | crate::panel::Target::Strip(key)
                        | crate::panel::Target::Fold(key)
                        | crate::panel::Target::Input(key)
                        | crate::panel::Target::OutwardCard(key, _)
                        | crate::panel::Target::ReopenProducer(key) => {
                            self.studio.set_studio_focus(key.clone());
                        }
                        crate::panel::Target::Person(key, role_name) => {
                            let machines = self.panel_summaries();
                            if let Some(term) = crate::panel::data::studio(&machines, key)
                                .and_then(|s| s.studio.roles.iter().find(|r| r.role == *role_name))
                                .and_then(|r| r.terminal.clone())
                            {
                                self.studio.current = Some(key.clone());
                                self.studio.focus =
                                    Some(crate::studio::StudioFocus::Role(key.clone(), term));
                            }
                        }
                        _ => {}
                    }
                    self.panel_menu(&target, point);
                }
            }
            MouseEventKind::ScrollUp => {
                let viewport = self
                    .panel_hits
                    .metrics
                    .map(|m| m.viewport_rows)
                    .unwrap_or(1);
                self.panel_scroll_to(self.panel.scroll.saturating_sub(1), viewport);
            }
            MouseEventKind::ScrollDown => {
                let viewport = self
                    .panel_hits
                    .metrics
                    .map(|m| m.viewport_rows)
                    .unwrap_or(1);
                self.panel_scroll_to(self.panel.scroll.saturating_add(1), viewport);
            }
            _ => {}
        }
    }

    /// The scrollbar grab continues — drag math mirrors the
    /// navigator's.
    pub(crate) fn panel_drag_scrollbar(&mut self, row: u16, grab_row_offset: u16) {
        if let Some(metrics) = self.panel_hits.metrics {
            let offset = scroll::scrollbar_offset_from_drag_row(
                metrics,
                self.panel_hits.scrollbar,
                row,
                grab_row_offset,
            );
            self.panel_scroll_to(
                metrics.max_offset_from_bottom.saturating_sub(offset),
                metrics.viewport_rows,
            );
        }
    }
}
