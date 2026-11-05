// Derived from herdr src/client/shell/text_editor.rs (Apache-2.0).
use crossterm::event::{KeyCode, KeyEventKind, KeyModifiers};
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct TextEditor {
    text: String,
    cursor: usize,
    replace_on_type: bool,
    killed: String,
}

impl std::ops::Deref for TextEditor {
    type Target = str;
    fn deref(&self) -> &str {
        self.as_str()
    }
}

impl std::fmt::Display for TextEditor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.text.fmt(f)
    }
}

impl From<&str> for TextEditor {
    fn from(text: &str) -> Self {
        Self::new(text, false)
    }
}

impl TextEditor {
    pub fn new(text: &str, replace_on_type: bool) -> Self {
        let mut editor = Self::default();
        editor.insert(text);
        editor.replace_on_type = replace_on_type;
        editor
    }

    pub fn as_str(&self) -> &str {
        &self.text
    }

    pub fn clear(&mut self) {
        self.text.clear();
        self.cursor = 0;
        self.replace_on_type = false;
    }

    fn repair_cursor(&mut self) {
        // Insertion/deletion can join clusters across the edit. Snap forward, never
        // leave an insertion offset inside the newly formed grapheme.
        self.cursor = self
            .text
            .grapheme_indices(true)
            .map(|(index, _)| index)
            .find(|index| *index >= self.cursor)
            .unwrap_or(self.text.len());
    }

    pub fn insert(&mut self, text: &str) -> bool {
        let mut normalized = String::new();
        let mut chars = text.chars().peekable();
        while let Some(ch) = chars.next() {
            match ch {
                '\r' => {
                    if chars.peek() == Some(&'\n') {
                        chars.next();
                    }
                    normalized.push(' ');
                }
                '\n' | '\t' => normalized.push(' '),
                ch if !ch.is_control() => normalized.push(ch),
                _ => {}
            }
        }
        if normalized.is_empty() {
            return false;
        }
        let content_changed = !self.replace_on_type || self.text != normalized;
        if self.replace_on_type {
            self.clear();
        }
        self.text.insert_str(self.cursor, &normalized);
        self.cursor += normalized.len();
        self.repair_cursor();
        content_changed
    }

    fn previous(&self) -> usize {
        self.text[..self.cursor]
            .grapheme_indices(true)
            .next_back()
            .map(|(index, _)| index)
            .unwrap_or(0)
    }

    fn next(&self) -> usize {
        self.cursor
            + self.text[self.cursor..]
                .graphemes(true)
                .next()
                .map(str::len)
                .unwrap_or(0)
    }

    fn word_boundary(&self, backward: bool) -> usize {
        let class = |grapheme: &str| {
            let ch = grapheme.chars().next().unwrap_or(' ');
            if ch.is_whitespace() {
                0
            } else if ch.is_alphanumeric() || ch == '_' {
                1
            } else {
                2
            }
        };
        let mut boundary = self.cursor;
        let mut run = 0;
        if backward {
            for (index, grapheme) in self.text[..self.cursor].grapheme_indices(true).rev() {
                let current = class(grapheme);
                if run != 0 && current != run {
                    break;
                }
                run = current;
                boundary = index;
            }
        } else {
            for (index, grapheme) in self.text[self.cursor..].grapheme_indices(true) {
                let current = class(grapheme);
                if run != 0 && current != run {
                    break;
                }
                run = current;
                boundary = self.cursor + index + grapheme.len();
            }
        }
        boundary
    }

    fn remove(&mut self, start: usize, end: usize, kill: bool) {
        self.replace_on_type = false;
        if start == end {
            return;
        }
        if kill {
            self.killed = self.text[start..end].to_owned();
        }
        self.text.replace_range(start..end, "");
        self.cursor = start;
        self.repair_cursor();
    }

    pub fn handle_key(&mut self, key: &crate::app::input::TerminalKey) -> Option<bool> {
        let combo = crate::input::terminal_key_of(key);
        if combo.kind == KeyEventKind::Release {
            return None;
        }
        if matches!(combo.code, KeyCode::Enter | KeyCode::Esc) {
            return None;
        }
        let previous_len = self.text.len();
        let mut content_changed = false;
        let (code, modifiers) = crate::config::normalize_key_combo((combo.code, combo.modifiers));
        // Explicit text from the host is authoritative, including AltGr/composition.
        if let Some(text) = key.text_commit.as_deref().filter(|text| !text.is_empty()) {
            content_changed = self.insert(text);
        } else {
            let ctrl = modifiers == KeyModifiers::CONTROL;
            let alt = modifiers == KeyModifiers::ALT;
            let plain = modifiers.is_empty();
            let movement = match code {
                KeyCode::Left if plain => Some(self.previous()),
                KeyCode::Char('b') if ctrl => Some(self.previous()),
                KeyCode::Right if plain => Some(self.next()),
                KeyCode::Char('f') if ctrl => Some(self.next()),
                KeyCode::Home if plain => Some(0),
                KeyCode::Char('a') if ctrl => Some(0),
                KeyCode::End if plain => Some(self.text.len()),
                KeyCode::Char('e') if ctrl => Some(self.text.len()),
                KeyCode::Char('b') if alt => Some(self.word_boundary(true)),
                KeyCode::Char('f') if alt => Some(self.word_boundary(false)),
                _ => None,
            };
            if let Some(position) = movement {
                self.cursor = position;
                self.replace_on_type = false;
            } else {
                match code {
                    KeyCode::Backspace | KeyCode::Char('h')
                        if code == KeyCode::Backspace && plain
                            || code == KeyCode::Char('h') && ctrl =>
                    {
                        if self.replace_on_type {
                            self.clear();
                        } else {
                            self.remove(self.previous(), self.cursor, false);
                        }
                    }
                    KeyCode::Delete if plain => self.remove(self.cursor, self.next(), false),
                    KeyCode::Char('d') if ctrl => self.remove(self.cursor, self.next(), false),
                    KeyCode::Char('u') if ctrl => self.remove(0, self.cursor, true),
                    KeyCode::Char('k') if ctrl => self.remove(self.cursor, self.text.len(), true),
                    KeyCode::Char('w') if ctrl => {
                        self.remove(self.word_boundary(true), self.cursor, true)
                    }
                    KeyCode::Backspace if ctrl || alt => {
                        self.remove(self.word_boundary(true), self.cursor, true)
                    }
                    KeyCode::Char('d') if alt => {
                        self.remove(self.cursor, self.word_boundary(false), true)
                    }
                    KeyCode::Char('y') if ctrl => {
                        content_changed = self.insert(&self.killed.clone());
                    }
                    KeyCode::Char(_) if modifiers.difference(KeyModifiers::SHIFT).is_empty() => {
                        if let Some(ch) = crate::keybind_help::keybind_help_text_char(key) {
                            content_changed = self.insert(&ch.to_string());
                        }
                    }
                    _ => return None,
                }
            }
        }
        Some(content_changed || self.text.len() != previous_len)
    }

    pub fn viewport(&self, width: u16) -> (&str, u16) {
        if width == 0 {
            return ("", 0);
        }
        let mut start = self.cursor;
        let mut cells = 0;
        for (index, grapheme) in self.text[..self.cursor].grapheme_indices(true).rev() {
            let next = cells + grapheme.width();
            if next >= usize::from(width) {
                break;
            }
            start = index;
            cells = next;
        }
        let mut end = self.cursor;
        let mut used = cells;
        for (index, grapheme) in self.text[self.cursor..].grapheme_indices(true) {
            used += grapheme.width();
            if used > usize::from(width) {
                break;
            }
            end = self.cursor + index + grapheme.len();
        }
        (&self.text[start..end], cells as u16)
    }
}

pub(crate) fn render(
    buffer: &mut ratatui::buffer::Buffer,
    area: ratatui::layout::Rect,
    editor: &TextEditor,
    style: ratatui::style::Style,
) -> Option<super::help::CursorState> {
    let area = area.intersection(buffer.area);
    if area.is_empty() {
        return None;
    }
    let (text, cursor) = editor.viewport(area.width);
    for x in area.x..area.right() {
        buffer[(x, area.y)].set_symbol(" ").set_style(style);
    }
    buffer.set_stringn(area.x, area.y, text, usize::from(area.width), style);
    Some(super::help::CursorState {
        x: area.x + cursor,
        y: area.y,
        visible: true,
    })
}

#[cfg(test)]
#[path = "text_editor_tests.rs"]
mod tests;
