//! Expanded Call Sheet keys (S1 §5): the sheet owns every press
//! like the studio panel's focused mode — j/k/arrows and paging
//! move the selection, `1`–`9` jump to a frame, Enter activates the
//! row, each waiting kind's letters run its commands (`y`/`d`/`p` on
//! a prompt, `r` on a stage, `a`/`x`/`s` on a card, `f` with its
//! `y/n` ask on a ready studio), `i` opens the `tell` line, `?` is
//! the sheet's keys in `Ctrl+B ?`, and `Esc` collapses it. While
//! the input line edits every key is the editor's; `Esc` leaves it.

use crossterm::event::KeyCode;

use super::App;
use crate::app::input::TerminalKey;
use crate::hollywood::LayerMode;

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
        // An armed `y/n` confirm takes the next key before anything.
        if self.hollywood_confirm_key(code) && matches!(key.input.action, hc_vt::Action::Press) {
            return;
        }
        if self.hollywood.confirm.is_some() {
            return;
        }
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
            KeyCode::Esc => self.hollywood_collapse(),
            // A digit jumps to that frame — `0` is the new-studio
            // frame trailing them.
            KeyCode::Char(d @ '0'..='9') if plain => self.hollywood_jump(d),
            KeyCode::Char('i') if plain && self.has_hollywood_cfg => {
                self.hollywood.editing = true;
            }
            KeyCode::Char('n') if plain => self.hollywood_new_studio(),
            KeyCode::Char('m') if plain => self.hollywood_menu_selected(),
            KeyCode::Char('?') if plain => self.help = true,
            KeyCode::Enter => self.hollywood_activate(),
            _ if plain => {
                self.hollywood_row_key(code);
            }
            _ => {}
        }
    }

    /// Collapse the sheet to the lot pill — `Esc`, the toggle's close
    /// arm, and the paths that leave it share this.
    pub(crate) fn hollywood_collapse(&mut self) {
        self.hollywood.mode = LayerMode::Collapsed;
        self.hollywood.editing = false;
        self.hollywood.view = None;
        self.hollywood.confirm = None;
    }

    /// `1`–`9` select that studio's frame; `0` the new-studio frame.
    /// The jump lands whether the frames are rows or chips.
    fn hollywood_jump(&mut self, digit: char) {
        let frames = self.hollywood_frames();
        let index = match digit {
            '0' => frames.len().saturating_sub(1),
            d => (d as usize - '1' as usize).min(frames.len().saturating_sub(1)),
        };
        if let Some(target) = frames.get(index).cloned() {
            self.hollywood.selected = Some(target);
            self.hollywood_keep_frame_visible();
        }
    }
}
