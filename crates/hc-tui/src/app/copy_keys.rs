// Derived from herdr src/client/shell/copy_mode.rs key routing,
// `request_copy_motion`, `request_copy_search`, `open_copy_search` and
// `repeat_copy_search` (Apache-2.0).
//! Copy-mode key routing and the synchronous counterparts of the
//! reference's motion/search dispatches. The reference queued
//! `pane.copy_motion`/`pane.copy_search` RPCs; hc-tui resolves both
//! locally against the pane's retained VT, so results apply in the same
//! call and no generation bookkeeping is needed.

use std::time::Instant;

use crossterm::event::{KeyCode, KeyModifiers};

use crate::chrome::text_editor::TextEditor;
use crate::copy_buffer::{COPY_SEARCH_QUERY_MAX, COPY_SEARCH_RETAINED_MAX, WordMotion};
use crate::copy_mode::{
    CopyMotion, CopySearchDirection, PaneTextPoint, copy_mode_command_char, first_non_blank_col,
    last_character_col,
};
use crate::copy_window::{paragraph_motion_target, search_text_window, word_motion_target};

use super::App;
use super::copy_mode::CopySearchPrompt;
use super::input::TerminalKey;

impl App {
    /// The reference's `route_copy_mode_key`. Order matters: the search
    /// prompt swallows keys first, then the fixed keys, then the
    /// copy-mode command characters.
    pub(super) fn route_copy_mode_key(&mut self, key: &TerminalKey) {
        if self.route_copy_search_prompt_key(key) {
            return;
        }
        let combo = crate::input::terminal_key_of(key);
        match combo.code {
            KeyCode::Esc => {
                let should_clear = self.copy_mode.as_ref().is_some_and(|copy_mode| {
                    copy_mode.selection.is_some()
                        || !copy_mode.search_query.is_empty()
                        || !copy_mode.search_matches.is_empty()
                        || copy_mode.search_direction.is_some()
                });
                if should_clear {
                    if let Some(copy_mode) = self.copy_mode.as_mut() {
                        copy_mode.selection = None;
                        copy_mode.search_query.clear();
                        copy_mode.search_direction = None;
                        copy_mode.search_matches.clear();
                        copy_mode.search_total = 0;
                        copy_mode.search_current = None;
                        copy_mode.search_current_global = None;
                    }
                    self.selection = None;
                } else {
                    self.exit_copy_mode(false);
                }
                self.sched.mark_dirty(Instant::now());
                return;
            }
            KeyCode::Enter => {
                self.exit_copy_mode(true);
                return;
            }
            KeyCode::Left => {
                self.move_copy_cursor(0, -1);
                return;
            }
            KeyCode::Down => {
                self.move_copy_cursor(1, 0);
                return;
            }
            KeyCode::Up => {
                self.move_copy_cursor(-1, 0);
                return;
            }
            KeyCode::Right => {
                self.move_copy_cursor(0, 1);
                return;
            }
            KeyCode::PageUp => {
                self.move_copy_page(-1, false);
                return;
            }
            KeyCode::PageDown => {
                self.move_copy_page(1, false);
                return;
            }
            KeyCode::Home => {
                self.set_copy_cursor_col(0);
                return;
            }
            KeyCode::End => {
                self.request_copy_motion(CopyMotion::LineEnd);
                return;
            }
            _ => {}
        }

        match (combo.code, combo.modifiers) {
            (KeyCode::Char('b'), modifiers) if modifiers.contains(KeyModifiers::CONTROL) => {
                self.move_copy_page(-1, false);
                return;
            }
            (KeyCode::Char('f'), modifiers) if modifiers.contains(KeyModifiers::CONTROL) => {
                self.move_copy_page(1, false);
                return;
            }
            (KeyCode::Char('u'), modifiers) if modifiers.contains(KeyModifiers::CONTROL) => {
                self.move_copy_page(-1, true);
                return;
            }
            (KeyCode::Char('d'), modifiers) if modifiers.contains(KeyModifiers::CONTROL) => {
                self.move_copy_page(1, true);
                return;
            }
            _ => {}
        }

        let Some(command) = copy_mode_command_char(&combo) else {
            return;
        };
        match command {
            'q' => self.exit_copy_mode(false),
            'y' => self.exit_copy_mode(true),
            'v' | ' ' => self.begin_copy_selection(false),
            'V' => self.begin_copy_selection(true),
            'h' => self.move_copy_cursor(0, -1),
            'j' => self.move_copy_cursor(1, 0),
            'k' => self.move_copy_cursor(-1, 0),
            'l' => self.move_copy_cursor(0, 1),
            'g' => self.move_copy_history(true),
            'G' => self.move_copy_history(false),
            '0' => self.set_copy_cursor_col(0),
            '$' => self.request_copy_motion(CopyMotion::LineEnd),
            '^' => self.request_copy_motion(CopyMotion::FirstNonBlank),
            '/' => self.open_copy_search(CopySearchDirection::Forward),
            '?' => self.open_copy_search(CopySearchDirection::Backward),
            'n' => self.repeat_copy_search(false),
            'N' => self.repeat_copy_search(true),
            'w' => self.request_copy_motion(CopyMotion::NextWordStart),
            'b' => self.request_copy_motion(CopyMotion::PreviousWordStart),
            'e' => self.request_copy_motion(CopyMotion::NextWordEnd),
            'W' => self.request_copy_motion(CopyMotion::NextBigWordStart),
            'B' => self.request_copy_motion(CopyMotion::PreviousBigWordStart),
            'E' => self.request_copy_motion(CopyMotion::NextBigWordEnd),
            '{' => self.request_copy_motion(CopyMotion::PreviousParagraph),
            '}' => self.request_copy_motion(CopyMotion::NextParagraph),
            _ => return,
        }
        self.sched.mark_dirty(Instant::now());
    }

    /// The reference's `route_copy_search_prompt_key`: while the `/`/`?`
    /// prompt is open it consumes every key — Enter submits, Esc closes,
    /// anything else edits the query.
    fn route_copy_search_prompt_key(&mut self, key: &TerminalKey) -> bool {
        let Some(prompt) = self
            .copy_mode
            .as_ref()
            .and_then(|copy_mode| copy_mode.search_prompt.as_ref())
        else {
            return false;
        };
        let mut submit = None;
        match key.input.key {
            hc_vt::Key::Escape => {
                if let Some(copy_mode) = self.copy_mode.as_mut() {
                    copy_mode.search_prompt = None;
                }
            }
            hc_vt::Key::Enter => {
                submit = Some((prompt.query.as_str().to_owned(), prompt.direction));
                if let Some(copy_mode) = self.copy_mode.as_mut() {
                    copy_mode.search_prompt = None;
                }
            }
            _ => {
                if let Some(prompt) = self
                    .copy_mode
                    .as_mut()
                    .and_then(|copy_mode| copy_mode.search_prompt.as_mut())
                {
                    prompt.query.handle_key(key);
                }
            }
        }
        if let Some((query, direction)) = submit {
            self.request_copy_search(query, direction, false);
        }
        self.sched.mark_dirty(Instant::now());
        true
    }

    /// The reference's `insert_copy_search_text`: pasted text lands in
    /// the open search prompt instead of the pane. The reference gates
    /// on its single `overlay` slot; the equivalent set here is dialog,
    /// menu, navigator and settings.
    pub(super) fn insert_copy_search_text(&mut self, text: &str) -> bool {
        if self.mode != super::Mode::Copy
            || self.dialog.is_some()
            || self.menu.is_some()
            || self.navigator.is_some()
            || self.settings.is_some()
            || self.popup.is_some()
            || self.popup_pending.is_some()
        {
            return false;
        }
        let Some(prompt) = self
            .copy_mode
            .as_mut()
            .and_then(|copy_mode| copy_mode.search_prompt.as_mut())
        else {
            return false;
        };
        prompt.query.insert(text);
        true
    }

    /// The reference's `open_copy_search`: `/` searches forward, `?`
    /// backward. The prompt starts empty every time.
    fn open_copy_search(&mut self, direction: CopySearchDirection) {
        let Some(copy_mode) = self.copy_mode.as_mut() else {
            return;
        };
        copy_mode.search_prompt = Some(CopySearchPrompt {
            direction,
            query: TextEditor::new("", false),
        });
    }

    /// The reference's `repeat_copy_search`: `n` repeats in the stored
    /// direction, `N` flips it.
    fn repeat_copy_search(&mut self, reverse: bool) {
        let Some(copy_mode) = self.copy_mode.as_ref() else {
            return;
        };
        if copy_mode.search_query.is_empty() {
            return;
        }
        let Some(direction) = copy_mode.search_direction else {
            return;
        };
        let direction = match (direction, reverse) {
            (CopySearchDirection::Forward, true) => CopySearchDirection::Backward,
            (CopySearchDirection::Backward, true) => CopySearchDirection::Forward,
            (direction, _) => direction,
        };
        let query = copy_mode.search_query.clone();
        self.request_copy_search(query, direction, true);
    }

    /// The reference's `request_copy_search` plus
    /// `dispatch_next_copy_operation`'s `previous` computation and
    /// `apply_copy_search_result` — collapsed because the local search
    /// resolves synchronously.
    fn request_copy_search(&mut self, query: String, direction: CopySearchDirection, repeat: bool) {
        if query.is_empty() || query.len() > COPY_SEARCH_QUERY_MAX || self.copy_mode.is_none() {
            return;
        }
        let (pane_id, cursor, previous) = {
            let copy_mode = self.copy_mode.as_ref().expect("checked above");
            // The reference's repeat origin: the retained match whose
            // start sits on the cursor, so `n` steps to the next match.
            let previous = repeat
                .then(|| {
                    copy_mode
                        .search_current
                        .and_then(|current| copy_mode.search_matches.get(current).copied())
                        .filter(|range| range.start == copy_mode.cursor)
                })
                .flatten();
            (copy_mode.pane_id.clone(), copy_mode.cursor, previous)
        };
        let case_sensitive = query.chars().any(char::is_uppercase);
        let result = self
            .displayed_pane(&pane_id)
            .and_then(|pane| pane.vt_ref())
            .map(|vt| {
                search_text_window(
                    vt,
                    &query,
                    case_sensitive,
                    direction,
                    cursor,
                    previous,
                    COPY_SEARCH_RETAINED_MAX,
                )
            })
            .unwrap_or_default();
        let Some(copy_mode) = self.copy_mode.as_mut() else {
            return;
        };
        copy_mode.search_query = query;
        if !repeat {
            copy_mode.search_direction = Some(direction);
        }
        copy_mode.search_matches = result.matches;
        copy_mode.search_total = result.total;
        let current = result
            .current
            .filter(|index| *index < copy_mode.search_matches.len());
        copy_mode.search_current = current;
        copy_mode.search_current_global = result.current_global;
        let target = current.and_then(|index| copy_mode.search_matches.get(index).copied());
        if let Some(target) = target {
            copy_mode.cursor = target.start;
            self.reveal_copy_cursor(true);
            self.sync_copy_selection();
        }
        self.sched.mark_dirty(Instant::now());
    }

    /// The reference's `request_copy_motion` plus
    /// `apply_copy_motion_target` — resolved synchronously.
    fn request_copy_motion(&mut self, motion: CopyMotion) {
        let Some(copy_mode) = self.copy_mode.as_ref() else {
            return;
        };
        let pane_id = copy_mode.pane_id.clone();
        let origin = copy_mode.cursor;
        let Some(vt) = self.displayed_pane(&pane_id).and_then(|pane| pane.vt_ref()) else {
            return;
        };
        let target = match motion {
            CopyMotion::LineEnd | CopyMotion::FirstNonBlank => {
                // The reference reads the cursor row through the pane's
                // retained text at terminal width (`extract_selection` of
                // the full row); an unavailable row reports an error and
                // leaves the cursor where it is.
                let width = vt.size().map(|(cols, _)| cols).unwrap_or(1).max(1);
                match vt.screen_row_text(origin.row, width) {
                    Err(_) => origin,
                    Ok(text) => {
                        let col = match motion {
                            CopyMotion::LineEnd => last_character_col(&text).unwrap_or(0),
                            _ => first_non_blank_col(&text).unwrap_or(0),
                        };
                        PaneTextPoint {
                            row: origin.row,
                            col: col.min(width.saturating_sub(1)),
                        }
                    }
                }
            }
            CopyMotion::NextWordStart => {
                word_motion_target(vt, origin.row, origin.col, WordMotion::NextStart)
                    .unwrap_or(origin)
            }
            CopyMotion::PreviousWordStart => {
                word_motion_target(vt, origin.row, origin.col, WordMotion::PreviousStart)
                    .unwrap_or(origin)
            }
            CopyMotion::NextWordEnd => {
                word_motion_target(vt, origin.row, origin.col, WordMotion::NextEnd)
                    .unwrap_or(origin)
            }
            CopyMotion::NextBigWordStart => {
                word_motion_target(vt, origin.row, origin.col, WordMotion::NextBigStart)
                    .unwrap_or(origin)
            }
            CopyMotion::PreviousBigWordStart => {
                word_motion_target(vt, origin.row, origin.col, WordMotion::PreviousBigStart)
                    .unwrap_or(origin)
            }
            CopyMotion::NextBigWordEnd => {
                word_motion_target(vt, origin.row, origin.col, WordMotion::NextBigEnd)
                    .unwrap_or(origin)
            }
            CopyMotion::PreviousParagraph => paragraph_motion_target(vt, origin.row, -1)
                .map(|target| PaneTextPoint {
                    row: target.row,
                    col: origin.col,
                })
                .unwrap_or(origin),
            CopyMotion::NextParagraph => paragraph_motion_target(vt, origin.row, 1)
                .map(|target| PaneTextPoint {
                    row: target.row,
                    col: origin.col,
                })
                .unwrap_or(origin),
        };
        if let Some(copy_mode) = self.copy_mode.as_mut() {
            copy_mode.cursor = target;
        }
        self.reveal_copy_cursor(false);
        self.sync_copy_selection();
        self.sched.mark_dirty(Instant::now());
    }
}
