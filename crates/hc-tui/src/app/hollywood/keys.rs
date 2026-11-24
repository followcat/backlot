//! Expanded Call Sheet keys (S1 §5): the sheet owns every press
//! like the studio panel's focused mode — j/k/arrows and paging
//! move the selection, `1`–`9` jump to a frame, Enter activates the
//! row, each waiting kind's letters run its commands (`y`/`d`/`p` on
//! a prompt, `s` on a stage, `a`/`x`/`s` on a card, `f` with its
//! `y/n` ask on a ready studio), `i` opens the `tell` line, `?` is
//! the sheet's keys in `Ctrl+B ?`, and `Esc` collapses it. While
//! the input line edits every key is the editor's; `Esc` leaves it.

use crossterm::event::{KeyCode, KeyModifiers};

use super::App;
use crate::app::input::TerminalKey;
use crate::hollywood::{LayerMode, Section, Target};

impl App {
    /// Keys while the layer is expanded — every press is consumed
    /// here. Repeats move only; releases are dropped.
    pub(crate) fn handle_hollywood_key(&mut self, key: &TerminalKey) {
        // The "Show me" view owns its keys until it closes.
        if self.hollywood.view.is_some() {
            self.hollywood_view_key(key);
            return;
        }
        let combo = crate::input::terminal_key_of(key);
        let (code, modifiers) = crate::config::normalize_key_combo((combo.code, combo.modifiers));
        // An armed `y/n` confirm takes the next key before the editor
        // — a question the sheet asked must not read typed letters as
        // its answer, and typing must not answer it by accident.
        if self.hollywood_confirm_key(code) && matches!(key.input.action, hc_vt::Action::Press) {
            return;
        }
        if self.hollywood.confirm.is_some() {
            return;
        }
        // `Tab` / `Shift+Tab` switches between the upper studio frames
        // and the lower waiting/exchange/input band, even from `Compose`
        // (resting the input line while preserving any draft).
        if matches!(code, KeyCode::Tab | KeyCode::BackTab)
            && (modifiers.is_empty() || modifiers == KeyModifiers::SHIFT)
            && matches!(
                key.input.action,
                hc_vt::Action::Press | hc_vt::Action::Repeat
            )
        {
            self.hollywood_switch_section();
            return;
        }
        // `Compose` (R3-B2/R3-2): the `›` line owns every key —
        // `⏎` sends and stays in Compose for the next `tell
        // hollywood`, `esc` rests the line keeping its draft, `↑`
        // rests and steps up one row, the rest is the editor's.
        if self.hollywood.editing {
            if code == KeyCode::Esc && modifiers.is_empty() {
                // One Esc out of Compose rests the line — draft and
                // all. The Esc that collapses the sheet is the
                // resting one below, a press later.
                self.hollywood.editing = false;
                return;
            }
            if code == KeyCode::Enter && modifiers.is_empty() {
                self.hollywood_input_submit();
                return;
            }
            // `↑` is how a composing line steps out into the rows —
            // Esc no longer doubles as vertical navigation.
            if code == KeyCode::Up && modifiers.is_empty() {
                self.hollywood.editing = false;
                self.hollywood_row_move(-1);
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
        if self.live.matches_prefix(&combo) {
            let _ = self.prefix.feed(key, &self.live);
            return;
        }
        let plain = modifiers.is_empty();
        // `?` needs Shift on most layouts, and a kitty-reporting host
        // (REPORT_ALTERNATE_KEYS) reports that Shift beside the key —
        // still the bare `?` here, never a dead key.
        let question = code == KeyCode::Char('?')
            && (plain || modifiers == crossterm::event::KeyModifiers::SHIFT);
        if matches!(code, KeyCode::Down | KeyCode::Char('j')) && plain {
            self.hollywood_row_move(1);
            return;
        }
        if matches!(code, KeyCode::Up | KeyCode::Char('k')) && plain {
            self.hollywood_row_move(-1);
            return;
        }
        if matches!(code, KeyCode::Right | KeyCode::Char('l')) && plain {
            match self.hollywood_active_section() {
                Section::Upper => self.hollywood_frame_move(1),
                Section::Lower => self.hollywood_chip_move(1),
            }
            return;
        }
        if matches!(code, KeyCode::Left | KeyCode::Char('h')) && plain {
            match self.hollywood_active_section() {
                Section::Upper => self.hollywood_frame_move(-1),
                Section::Lower => self.hollywood_chip_move(-1),
            }
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
        // `Resting` Home/End: the sheet's first frame / its band's
        // last row (R3-M11) — in Compose they are the editor's own.
        if code == KeyCode::Home && plain {
            if let Some(first) = self.hollywood_frames().first().cloned() {
                self.hollywood_select(first);
                self.hollywood_keep_frame_visible();
            }
            return;
        }
        if code == KeyCode::End && plain {
            let band = self.hollywood_band();
            if let Some(last) = band.last().cloned() {
                let index = band.len() - 1;
                self.hollywood_select(last);
                self.hollywood_reveal_card(&band, index);
            }
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
                self.hollywood_select(Target::Input);
                self.hollywood.editing = true;
            }
            KeyCode::Char('n') if plain => self.hollywood_new_studio(),
            KeyCode::Char('m') if plain => self.hollywood_menu_selected(),
            KeyCode::Char('?') if question => {
                self.help = true;
                self.help_state = crate::chrome::help::HelpState::default();
            }
            KeyCode::Enter => self.hollywood_enter(),
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
            self.hollywood_select(target);
            self.hollywood_keep_frame_visible();
        }
    }
}
