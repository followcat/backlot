//! The layer's mouse dispatch: a click on the collapsed strip
//! expands it; in the expanded panel a row click selects and acts
//! like Enter, a card's Apply/Cancel/Show hits run that action, the
//! input line opens for editing, the scrollbar grabs and the wheel
//! scrolls. A press outside the expanded panel is not routed here —
//! it reaches whatever is under the layer and leaves the panel open.

use ratatui::layout::Position;

use super::App;
use crate::app::input::{MouseButton, MouseEventKind, MouseInput};
use crate::app::mouse::ChromeDrag;
use crate::hollywood::LayerMode;

fn contains(rect: ratatui::layout::Rect, point: (u16, u16)) -> bool {
    rect.contains(Position {
        x: point.0,
        y: point.1,
    })
}

impl App {
    /// The layer's mouse dispatch — called only when the press is
    /// inside `hollywood_hits.area` (or the wheel anywhere over it).
    pub(crate) fn hollywood_mouse(&mut self, mouse: MouseInput) {
        // The open "Show me" view owns the pointer until it closes.
        if self.hollywood.view.is_some() {
            self.hollywood_view_mouse(&mouse);
            return;
        }
        let point = (mouse.col, mouse.row);
        match self.hollywood.mode {
            LayerMode::Collapsed => {
                if mouse.kind == MouseEventKind::Down(MouseButton::Left) {
                    self.hollywood_toggle();
                }
            }
            LayerMode::Expanded => match mouse.kind {
                MouseEventKind::Down(MouseButton::Left) => {
                    if contains(self.hollywood_hits.input, point) {
                        self.hollywood.editing = true;
                        return;
                    }
                    if contains(self.hollywood_hits.scrollbar, point)
                        && let Some(metrics) = self.hollywood_hits.metrics
                    {
                        match crate::chrome::scroll::scrollbar_thumb_grab_offset(
                            metrics,
                            self.hollywood_hits.scrollbar,
                            point.1,
                        ) {
                            Some(grab) => {
                                self.chrome_drag = Some(ChromeDrag::HollywoodScrollbar {
                                    grab_row_offset: grab,
                                });
                            }
                            None => {
                                let offset = crate::chrome::scroll::scrollbar_offset_from_row(
                                    metrics,
                                    self.hollywood_hits.scrollbar,
                                    point.1,
                                );
                                self.hollywood_scroll_to(
                                    metrics.max_offset_from_bottom.saturating_sub(offset),
                                );
                            }
                        }
                        return;
                    }
                    // S1's mouse is select-only: a click on a row or
                    // a chip selects it (S5 adds the activate paths).
                    let hit = self
                        .hollywood_hits
                        .rows
                        .iter()
                        .find(|(rect, _)| contains(*rect, point))
                        .map(|(_, target)| target.clone());
                    if let Some(target) = hit {
                        self.hollywood.selected = Some(target);
                    }
                }
                MouseEventKind::ScrollUp => {
                    if self.hollywood_over_frames(point) {
                        self.hollywood_frame_scroll(-1);
                    } else {
                        self.hollywood_scroll_to(self.hollywood.scroll.saturating_sub(1));
                    }
                }
                MouseEventKind::ScrollDown => {
                    if self.hollywood_over_frames(point) {
                        self.hollywood_frame_scroll(1);
                    } else {
                        self.hollywood_scroll_to(self.hollywood.scroll.saturating_add(1));
                    }
                }
                _ => {}
            },
            LayerMode::Hidden => {}
        }
    }

    /// The point sits on the frames' block — the wheel scrolls it
    /// sideways there and the cards vertically.
    fn hollywood_over_frames(&self, point: (u16, u16)) -> bool {
        let rect = self.hollywood_frames_hit();
        !rect.is_empty() && contains(rect, point)
    }

    /// The wheel's sideways step over the frames: one frame, clamped
    /// into the window.
    fn hollywood_frame_scroll(&mut self, delta: isize) {
        let (visible, _) = self.hollywood_frame_window();
        let frames = self.hollywood_frames().len();
        let max = frames.saturating_sub(visible);
        let next = self.hollywood.frame_scroll as isize + delta;
        self.hollywood.frame_scroll = next.clamp(0, max as isize) as usize;
    }

    /// The scrollbar grab continues — drag math mirrors the panel's.
    pub(crate) fn hollywood_drag_scrollbar(&mut self, row: u16, grab_row_offset: u16) {
        if let Some(metrics) = self.hollywood_hits.metrics {
            let offset = crate::chrome::scroll::scrollbar_offset_from_drag_row(
                metrics,
                self.hollywood_hits.scrollbar,
                row,
                grab_row_offset,
            );
            self.hollywood_scroll_to(metrics.max_offset_from_bottom.saturating_sub(offset));
        }
    }
}
