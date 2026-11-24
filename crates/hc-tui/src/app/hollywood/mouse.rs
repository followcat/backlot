//! The layer's mouse dispatch: a click on the collapsed strip
//! expands it; in the expanded panel a row click selects and acts
//! like Enter, a card's Apply/Cancel/Show hits run that action, the
//! input line opens for editing, the scrollbar grabs and the wheel
//! scrolls. A press outside the expanded panel collapses it (A10.1)
//! and keeps routing to whatever is under the layer.

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
    /// A press outside the expanded Call Sheet collapses it back to
    /// the pill (S5 §2, A10.1) — the press itself keeps routing, so
    /// a click on a pane focuses it and hands the keys back. An open
    /// "Show me" view swallows the press itself instead.
    pub(crate) fn hollywood_outside_press(&mut self, kind: MouseEventKind) {
        if self.hollywood.expanded()
            && matches!(kind, MouseEventKind::Down(_))
            && self.hollywood.view.is_none()
        {
            self.hollywood_collapse();
        }
    }

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
                        self.hollywood_select(crate::hollywood::Target::Input);
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
                    // A chip is its own target (S5 §2): the click
                    // selects its row (and that chip index) and runs the
                    // chip's key — `↵` chips run the row's Enter.
                    let chip = self
                        .hollywood_hits
                        .chips
                        .iter()
                        .find(|(rect, _, _)| contains(*rect, point))
                        .map(|(rect, target, key)| {
                            let idx = self
                                .hollywood_hits
                                .chips
                                .iter()
                                .filter(|(_, t, _)| t == target)
                                .position(|(r, _, _)| *r == *rect);
                            (target.clone(), *key, idx)
                        });
                    if let Some((target, key, idx)) = chip {
                        self.hollywood_select(target);
                        if idx.is_some() {
                            self.hollywood.chip_index = idx;
                        }
                        if key == '\r' {
                            self.hollywood_activate();
                        } else {
                            self.hollywood_row_key(crossterm::event::KeyCode::Char(key));
                        }
                        return;
                    }
                    // A click in the 1-cell gap between two chips
                    // selects the row instead of falling through to
                    // `hollywood_activate()`.
                    let zone = self
                        .hollywood_hits
                        .chips_zones
                        .iter()
                        .find(|(rect, _)| contains(*rect, point))
                        .map(|(_, target)| target.clone());
                    if let Some(target) = zone {
                        self.hollywood_select(target);
                        return;
                    }
                    // A row click selects and acts like Enter (S5
                    // §2), except prompt/live rows stay selected without
                    // leaving the sheet so accidental clicks on prompt
                    // text never dump into `bl-hollywood`. The
                    // `+ new studio` frame is a button, not a row.
                    let hit = self
                        .hollywood_hits
                        .rows
                        .iter()
                        .find(|(rect, _)| contains(*rect, point))
                        .map(|(_, target)| target.clone());
                    match hit {
                        Some(crate::hollywood::Target::NewStudio) => self.hollywood_new_studio(),
                        Some(
                            target @ (crate::hollywood::Target::Prompt(..)
                            | crate::hollywood::Target::Hollywood
                            | crate::hollywood::Target::HollywoodLive),
                        ) => {
                            self.hollywood_select(target);
                        }
                        Some(target) => {
                            self.hollywood_select(target);
                            self.hollywood_activate();
                        }
                        None => {}
                    }
                }
                MouseEventKind::Down(MouseButton::Right) => {
                    // The title row opens the current studio's menu
                    // (S5 §2); a row's right-click selects it and
                    // opens its menu there (S3 §1).
                    if contains(self.hollywood_hits.title, point) {
                        if let Some(key) = self
                            .studio
                            .current
                            .clone()
                            .or_else(|| self.hit_map.studio_key.clone())
                        {
                            self.studio_header_menu(&key, point);
                        }
                        return;
                    }
                    let hit = self
                        .hollywood_hits
                        .rows
                        .iter()
                        .find(|(rect, _)| contains(*rect, point))
                        .map(|(_, target)| target.clone());
                    if let Some(target) = hit {
                        self.hollywood_select(target.clone());
                        self.hollywood_menu(&target, point);
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
