//! The "Show me" view's input: while `backlot card show` output is
//! open the view owns the layer's keys and mouse — `a` applies the
//! card, `x` cancels it (the same commands as the row), Esc/Enter/q
//! close it, arrows and the wheel scroll it, everything else is
//! eaten. A press outside its area closes it.

use super::App;
use crate::app::input;

impl App {
    /// The open view owns the layer's input: `a`/`x` answer a card
    /// when `view.card` is `Some` and unlocked, `y` copies raw source,
    /// `o` focuses/reopens, `p` opens prompt-less peek, `v` jumps to Sheet,
    /// `Enter` closes read-only views (no-op on cards), `←`/`→`/`[`/`]` step,
    /// `Esc`/`q` close, scroll keys move cleanly, `d` stays inert.
    pub(super) fn hollywood_view_key(&mut self, key: &input::TerminalKey) {
        if self.hollywood.view.is_none() {
            return;
        }
        if self.reader_handle_tell_key(key, false) {
            return;
        }
        let combo = crate::input::terminal_key_of(key);
        let (code, modifiers) = crate::config::normalize_key_combo((combo.code, combo.modifiers));
        let plain = modifiers.is_empty();
        let press = key.input.action == hc_vt::Action::Press;
        let card = self.hollywood.view.as_ref().and_then(|v| v.card.clone());
        let (page, metrics) = self
            .hollywood_hits
            .view
            .as_ref()
            .map_or((10usize, None), |v| {
                (v.body.height.max(1) as usize, v.metrics)
            });

        if let Some(view) = self.hollywood.view.as_mut()
            && Self::reader_scroll_key(
                &mut view.scroll,
                code,
                modifiers,
                key.input.action,
                page,
                metrics,
            )
        {
            return;
        }

        if !press {
            return;
        }

        match code {
            crossterm::event::KeyCode::Esc | crossterm::event::KeyCode::Char('q') if plain => {
                self.hollywood.view = None;
            }
            crossterm::event::KeyCode::Enter if plain => {
                if card.is_none() {
                    self.hollywood.view = None;
                }
            }
            crossterm::event::KeyCode::Char('?')
                if plain || modifiers == crossterm::event::KeyModifiers::SHIFT =>
            {
                self.help = true;
                self.help_state = crate::chrome::help::HelpState::default();
            }
            crossterm::event::KeyCode::Char('y') if plain => {
                if let Some((raw, lines)) = self
                    .hollywood
                    .view
                    .as_ref()
                    .map(|v| (v.raw_body.clone(), v.lines.clone()))
                {
                    self.reader_copy_raw(&raw, &lines);
                }
            }
            crossterm::event::KeyCode::Char('t' | 'i') if plain => {
                let _ = self.reader_open_compose(false);
            }
            crossterm::event::KeyCode::Char('o') if plain => {
                if let Some(meta) = self.hollywood.view.as_ref().map(|v| v.meta.clone()) {
                    self.reader_run_o(&meta);
                }
            }
            crossterm::event::KeyCode::Char('p') if plain => {
                if let Some(meta) = self.hollywood.view.as_ref().map(|v| v.meta.clone()) {
                    self.reader_open_peek(&meta);
                }
            }
            crossterm::event::KeyCode::Char('v') if plain => {
                if let Some(meta) = self.hollywood.view.as_ref().map(|v| v.meta.clone()) {
                    self.reader_open_sheet(&meta);
                }
            }
            crossterm::event::KeyCode::Char('a') if plain => {
                if let Some(card) = card
                    && self.card_pending_label(Some(&card)).is_none()
                {
                    self.hollywood_card_apply(&card);
                }
            }
            crossterm::event::KeyCode::Char('x') if plain => {
                if let Some(card) = card
                    && self.card_pending_label(Some(&card)).is_none()
                {
                    self.hollywood_card_cancel(&card);
                }
            }
            crossterm::event::KeyCode::Left | crossterm::event::KeyCode::Char('[') if plain => {
                self.reader_step(-1);
            }
            crossterm::event::KeyCode::Right | crossterm::event::KeyCode::Char(']') if plain => {
                self.reader_step(1);
            }
            _ => {}
        }
    }

    /// The view's mouse: the wheel scrolls, `◀`/`▶` step siblings,
    /// action chips `[k]` run their key, beside-dock `Talk` clicks switch
    /// in place, other outside clicks close.
    pub(super) fn hollywood_view_mouse(&mut self, mouse: &input::MouseInput) -> bool {
        let Some(hits) = self.hollywood_hits.view.as_ref() else {
            return false;
        };
        let (area, prev_hit, next_hit, compose_hit, metrics) = (
            hits.area,
            hits.prev_hit,
            hits.next_hit,
            hits.compose_rect,
            hits.metrics,
        );
        let chips = hits.chips.clone();
        let point = (mouse.col, mouse.row);
        let pos = ratatui::layout::Position::from(point);
        use input::{MouseButton, MouseEventKind};
        match mouse.kind {
            MouseEventKind::ScrollUp => {
                self.reader_dismiss_sent(false);
                if let Some(view) = self.hollywood.view.as_mut() {
                    view.scroll = view.scroll.saturating_sub(3);
                }
            }
            MouseEventKind::ScrollDown => {
                self.reader_dismiss_sent(false);
                if let (Some(view), Some(metrics)) = (self.hollywood.view.as_mut(), metrics) {
                    view.scroll = (view.scroll + 3).min(metrics.max_offset_from_bottom);
                }
            }
            MouseEventKind::Down(MouseButton::Left)
                if compose_hit.is_some_and(|r| r.contains(pos)) =>
            {
                let _ = self.reader_open_compose(false);
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
            MouseEventKind::Down(MouseButton::Left)
                if chips.iter().any(|(r, _)| r.contains(pos)) =>
            {
                if let Some((_, ch)) = chips.iter().find(|(r, _)| r.contains(pos)) {
                    self.reader_activate_chip(*ch);
                }
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
