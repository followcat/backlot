//! The "Show me" view's input: while `backlot card show` output is
//! open the view owns the layer's keys and mouse — `a` applies the
//! card, `x` cancels it (the same commands as the row), Esc/Enter/q
//! close it, arrows and the wheel scroll it, everything else is
//! eaten. A press outside its area closes it.

use super::App;
use crate::app::input;

impl App {
    /// The open view owns the layer's input: `a`/`x` answer a card
    /// when `view.card` is `Some` (and stay inert on read-only views),
    /// Esc/Enter/q close, arrows/PageUp/PageDown scroll, everything
    /// else is eaten.
    pub(super) fn hollywood_view_key(&mut self, key: &input::TerminalKey) {
        if self.hollywood.view.is_none() {
            return;
        }
        let combo = crate::input::terminal_key_of(key);
        let (code, modifiers) = crate::config::normalize_key_combo((combo.code, combo.modifiers));
        let plain = modifiers.is_empty();
        let press = key.input.action == hc_vt::Action::Press;
        let active = key.input.action != hc_vt::Action::Release;
        let card = self.hollywood.view.as_ref().and_then(|v| v.card.clone());
        let page = self
            .hollywood_hits
            .view
            .as_ref()
            .map(|v| v.body.height.max(1) as usize)
            .unwrap_or(10);
        match code {
            crossterm::event::KeyCode::Esc
            | crossterm::event::KeyCode::Enter
            | crossterm::event::KeyCode::Char('q')
                if plain && press =>
            {
                self.hollywood.view = None;
            }
            // Card commands run only on actionable card views and on
            // key press — read-only views leave `a`/`x` inert.
            crossterm::event::KeyCode::Char('a') if plain && press => {
                if let Some(card) = card {
                    self.hollywood_card_apply(&card);
                }
            }
            crossterm::event::KeyCode::Char('x') if plain && press => {
                if let Some(card) = card {
                    self.hollywood_card_cancel(&card);
                }
            }
            crossterm::event::KeyCode::Left | crossterm::event::KeyCode::Char('[')
                if plain && press =>
            {
                self.reader_step(-1);
            }
            crossterm::event::KeyCode::Right | crossterm::event::KeyCode::Char(']')
                if plain && press =>
            {
                self.reader_step(1);
            }
            crossterm::event::KeyCode::Down | crossterm::event::KeyCode::Char('j')
                if plain && active =>
            {
                if let Some(view) = self.hollywood.view.as_mut() {
                    view.scroll = view.scroll.saturating_add(1);
                }
            }
            crossterm::event::KeyCode::Up | crossterm::event::KeyCode::Char('k')
                if plain && active =>
            {
                if let Some(view) = self.hollywood.view.as_mut() {
                    view.scroll = view.scroll.saturating_sub(1);
                }
            }
            crossterm::event::KeyCode::PageDown if plain && active => {
                if let Some(view) = self.hollywood.view.as_mut() {
                    view.scroll = view.scroll.saturating_add(page);
                }
            }
            crossterm::event::KeyCode::PageUp if plain && active => {
                if let Some(view) = self.hollywood.view.as_mut() {
                    view.scroll = view.scroll.saturating_sub(page);
                }
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

    /// The view's mouse: the wheel scrolls, `◀`/`▶` step siblings,
    /// beside-dock `Talk` clicks switch in place, other outside clicks close.
    pub(super) fn hollywood_view_mouse(&mut self, mouse: &input::MouseInput) -> bool {
        let Some(hits) = self.hollywood_hits.view.as_ref() else {
            return false;
        };
        let (area, prev_hit, next_hit, metrics) =
            (hits.area, hits.prev_hit, hits.next_hit, hits.metrics);
        let point = (mouse.col, mouse.row);
        let pos = ratatui::layout::Position::from(point);
        use input::{MouseButton, MouseEventKind};
        match mouse.kind {
            MouseEventKind::ScrollUp => {
                if let Some(view) = self.hollywood.view.as_mut() {
                    view.scroll = view.scroll.saturating_sub(1);
                }
            }
            MouseEventKind::ScrollDown => {
                if let (Some(view), Some(metrics)) = (self.hollywood.view.as_mut(), metrics) {
                    view.scroll = (view.scroll + 1).min(metrics.max_offset_from_bottom);
                }
            }
            MouseEventKind::Down(MouseButton::Left)
                if prev_hit.is_some_and(|r| r.contains(pos)) =>
            {
                self.reader_step(-1);
            }
            MouseEventKind::Down(MouseButton::Left)
                if next_hit.is_some_and(|r| r.contains(pos)) =>
            {
                self.reader_step(1);
            }
            MouseEventKind::Down(MouseButton::Left) if !area.contains(pos) => {
                let dock_talk = (self.panel_beside(self.area.0)
                    && self.sheet.is_none()
                    && !self.hollywood.expanded()
                    && self.panel_hits.area.contains(pos))
                .then(|| {
                    self.panel_hits.rows.iter().find_map(|(r, t)| {
                        if r.contains(pos)
                            && let crate::panel::Target::Talk(key, seq) = t
                        {
                            Some((key.clone(), *seq))
                        } else {
                            None
                        }
                    })
                })
                .flatten();
                if let Some((key, seq)) = dock_talk {
                    self.reader_switch_dock_talk(&key, seq);
                } else {
                    self.hollywood.view = None;
                }
            }
            _ => {}
        }
        true
    }
}
