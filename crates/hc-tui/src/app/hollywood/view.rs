//! The "Show me" view's input: while `backlot card show` output is
//! open the view owns the layer's keys and mouse — Esc/x/q/Enter
//! close it, arrows and the wheel scroll it, everything else is
//! eaten. A press outside its area closes it.

use super::App;
use crate::app::input;

impl App {
    /// The open view owns the layer's input: Esc/x/q/Enter close,
    /// arrows scroll, everything else is eaten.
    pub(super) fn hollywood_view_key(&mut self, key: &input::TerminalKey) {
        let combo = crate::input::terminal_key_of(key);
        let (code, modifiers) = crate::config::normalize_key_combo((combo.code, combo.modifiers));
        let plain = modifiers.is_empty();
        let Some(view) = self.hollywood.view.as_mut() else {
            return;
        };
        match code {
            crossterm::event::KeyCode::Esc
            | crossterm::event::KeyCode::Enter
            | crossterm::event::KeyCode::Char('x')
            | crossterm::event::KeyCode::Char('q')
                if plain =>
            {
                self.hollywood.view = None;
            }
            crossterm::event::KeyCode::Down | crossterm::event::KeyCode::Char('j') if plain => {
                view.scroll = view.scroll.saturating_add(1);
            }
            crossterm::event::KeyCode::Up | crossterm::event::KeyCode::Char('k') if plain => {
                view.scroll = view.scroll.saturating_sub(1);
            }
            _ => {}
        }
        // Clamp into the last drawn metrics so the wheel cannot run
        // the scroll past the end.
        if let (Some(view), Some(metrics)) = (
            self.hollywood.view.as_mut(),
            self.hollywood_hits.view.as_ref().and_then(|v| v.metrics),
        ) {
            view.scroll = view.scroll.min(metrics.max_offset_from_bottom);
        }
    }

    /// The view's mouse: the wheel scrolls, a press outside closes,
    /// inside is eaten.
    pub(super) fn hollywood_view_mouse(&mut self, mouse: &input::MouseInput) -> bool {
        let Some(hits) = self.hollywood_hits.view.as_ref() else {
            return false;
        };
        let point = (mouse.col, mouse.row);
        use input::{MouseButton, MouseEventKind};
        match mouse.kind {
            MouseEventKind::ScrollUp => {
                if let Some(view) = self.hollywood.view.as_mut() {
                    view.scroll = view.scroll.saturating_sub(1);
                }
            }
            MouseEventKind::ScrollDown => {
                if let (Some(view), Some(metrics)) = (self.hollywood.view.as_mut(), hits.metrics) {
                    view.scroll = (view.scroll + 1).min(metrics.max_offset_from_bottom);
                }
            }
            MouseEventKind::Down(MouseButton::Left) if !hits.area.contains(point.into()) => {
                self.hollywood.view = None;
            }
            _ => {}
        }
        true
    }
}
