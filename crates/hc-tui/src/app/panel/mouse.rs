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
                    .find(|(rect, _)| contains(*rect, point))
                    .map(|(_, target)| target.clone());
                if let Some(target) = hit {
                    self.panel.selected = Some(target);
                    self.panel_activate();
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
