//! Expanded-panel keys: the panel owns every press like the studio
//! panel's focused mode — j/k/arrows and paging move the selection,
//! Enter activates, `a`/`c`/`s` run the selected card's Apply,
//! Cancel or Show me, `i` opens the input line, `x` hides the layer
//! and `Esc` collapses it to the strip. While the input line edits
//! every key is the editor's; `Esc` leaves the line.

use crossterm::event::KeyCode;

use super::App;
use crate::app::input::TerminalKey;
use crate::hollywood::{BtnKind, LayerMode};

impl App {
    /// Keys while the layer is expanded — every press is consumed
    /// here. Repeats move only; releases are dropped.
    pub(crate) fn handle_hollywood_key(&mut self, key: &TerminalKey) {
        // The "Show me" view owns its keys until it closes.
        if self.hollywood.view.is_some() {
            self.hollywood_view_key(key);
            return;
        }
        // The editing input line takes every key but Esc — Enter
        // submits a nonempty line, Esc leaves the editor.
        if self.hollywood.editing {
            let combo = crate::input::terminal_key_of(key);
            let (code, modifiers) =
                crate::config::normalize_key_combo((combo.code, combo.modifiers));
            if code == KeyCode::Esc && modifiers.is_empty() {
                self.hollywood.editing = false;
                return;
            }
            if code == KeyCode::Enter && modifiers.is_empty() {
                self.hollywood_input_submit();
                return;
            }
            let _ = self.hollywood.input.handle_key(key);
            return;
        }
        // Prefix plumbing while the layer owns the keys: an armed
        // prefix that is not the layer's own chord (that was
        // intercepted by the caller) disarms and is eaten.
        if self.prefix.armed() {
            self.prefix.reset();
            self.prefix.swallow(key);
            return;
        }
        let combo = crate::input::terminal_key_of(key);
        if self.live.matches_prefix(&combo) {
            let _ = self.prefix.feed(key, &self.live);
            return;
        }
        let (code, modifiers) = crate::config::normalize_key_combo((combo.code, combo.modifiers));
        let plain = modifiers.is_empty();
        if matches!(code, KeyCode::Down | KeyCode::Char('j')) && plain {
            self.hollywood_row_move(1);
            return;
        }
        if matches!(code, KeyCode::Up | KeyCode::Char('k')) && plain {
            self.hollywood_row_move(-1);
            return;
        }
        if matches!(code, KeyCode::Right | KeyCode::Char('l')) && plain {
            self.hollywood_frame_move(1);
            return;
        }
        if matches!(code, KeyCode::Left | KeyCode::Char('h')) && plain {
            self.hollywood_frame_move(-1);
            return;
        }
        let page = self.hollywood_hits.viewport_rows.max(1) as isize;
        if code == KeyCode::PageDown && plain {
            self.hollywood_scroll_to(self.hollywood.scroll.saturating_add(page as usize));
            return;
        }
        if code == KeyCode::PageUp && plain {
            self.hollywood_scroll_to(self.hollywood.scroll.saturating_sub(page as usize));
            return;
        }
        if key.input.action != hc_vt::Action::Press {
            return;
        }
        match code {
            KeyCode::Esc => {
                self.hollywood.mode = LayerMode::Collapsed;
                self.hollywood.editing = false;
                self.hollywood.view = None;
            }
            KeyCode::Char('x') if plain => {
                self.hollywood.mode = LayerMode::Hidden;
                self.hollywood.editing = false;
                self.hollywood.view = None;
            }
            KeyCode::Char('i') if plain && self.has_hollywood_cfg => {
                self.hollywood.editing = true;
            }
            KeyCode::Char('n') if plain => self.hollywood_new_studio(),
            KeyCode::Enter => self.hollywood_activate(),
            KeyCode::Char('a') if plain => self.hollywood_card_key(BtnKind::Apply),
            KeyCode::Char('c') if plain => self.hollywood_card_key(BtnKind::Cancel),
            KeyCode::Char('s') if plain => self.hollywood_card_key(BtnKind::Show),
            _ => {}
        }
    }
}
