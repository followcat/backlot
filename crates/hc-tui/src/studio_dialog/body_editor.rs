//! The multi-line editor the Tell and Revise bodies share (§2.3/§2.4):
//! lines, a cursor, Enter splits, paste lands as text — the body a
//! submit serialises with `\n` joins.

/// The 4-row editor the Tell and Revise bodies share (§2.3/§2.4):
/// lines, a cursor, Enter splits, paste lands as text — the body the
/// submit serialises with `\n` joins.
#[derive(Debug)]
pub(crate) struct BodyEditor {
    pub(crate) lines: Vec<Vec<char>>,
    pub(crate) row: usize,
    /// The cursor's char index inside `lines[row]`.
    pub(crate) col: usize,
}

impl Default for BodyEditor {
    /// An empty body — one empty row, since every edit indexes
    /// `lines[row]`.
    fn default() -> Self {
        BodyEditor {
            lines: vec![Vec::new()],
            row: 0,
            col: 0,
        }
    }
}

impl BodyEditor {
    /// A body seeded with `text` (the revise prefill) — the cursor
    /// starts at the top.
    pub(crate) fn new(text: &str) -> BodyEditor {
        let mut lines: Vec<Vec<char>> = text.lines().map(|l| l.chars().collect()).collect();
        if lines.is_empty() {
            lines.push(Vec::new());
        }
        BodyEditor {
            lines,
            ..BodyEditor::default()
        }
    }

    /// The buffer text — `\n`-joined rows.
    pub(crate) fn text(&self) -> String {
        self.lines
            .iter()
            .map(|l| l.iter().collect::<String>())
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn clamp(&mut self) {
        if self.row >= self.lines.len() {
            self.row = self.lines.len() - 1;
        }
        self.col = self.col.min(self.lines[self.row].len());
    }

    /// Insert `text` — `\n` splits rows; `\r` pairs merge.
    pub(crate) fn insert(&mut self, text: &str) {
        for ch in text.chars() {
            match ch {
                '\r' => {}
                '\n' => {
                    let rest: Vec<char> = self.lines[self.row].split_off(self.col);
                    self.lines.insert(self.row + 1, rest);
                    self.row += 1;
                    self.col = 0;
                }
                c if !c.is_control() => {
                    self.lines[self.row].insert(self.col, c);
                    self.col += 1;
                }
                _ => {}
            }
        }
    }

    /// The key handling Tell/Revise share — `Enter` is the caller's
    /// (Tell submits on a single line, Revise splits), `Esc` always
    /// cancels above this. Answers `false` for keys it does not own.
    pub(crate) fn key(
        &mut self,
        code: crossterm::event::KeyCode,
        mods: crossterm::event::KeyModifiers,
    ) -> bool {
        use crossterm::event::{KeyCode, KeyModifiers};
        let ctrl = mods.contains(KeyModifiers::CONTROL);
        match (code, ctrl) {
            (KeyCode::Char(c), false) if mods.difference(KeyModifiers::SHIFT).is_empty() => {
                self.insert(&c.to_string());
            }
            (KeyCode::Char(c), false) if mods == KeyModifiers::SHIFT => self.insert(&c.to_string()),
            (KeyCode::Char(' '), _) if mods.is_empty() => self.insert(" "),
            (KeyCode::Enter, _) => {
                let rest: Vec<char> = self.lines[self.row].split_off(self.col);
                self.lines.insert(self.row + 1, rest);
                self.row += 1;
                self.col = 0;
            }
            (KeyCode::Backspace, _) => {
                if self.col > 0 {
                    self.lines[self.row].remove(self.col - 1);
                    self.col -= 1;
                } else if self.row > 0 {
                    let row = self.lines.remove(self.row);
                    self.row -= 1;
                    self.col = self.lines[self.row].len();
                    self.lines[self.row].extend(row);
                }
            }
            (KeyCode::Delete, _) => {
                if self.col < self.lines[self.row].len() {
                    self.lines[self.row].remove(self.col);
                } else if self.row + 1 < self.lines.len() {
                    let row = self.lines.remove(self.row + 1);
                    self.lines[self.row].extend(row);
                }
            }
            (KeyCode::Left, _) => {
                if self.col > 0 {
                    self.col -= 1;
                } else if self.row > 0 {
                    self.row -= 1;
                    self.col = self.lines[self.row].len();
                }
            }
            (KeyCode::Right, _) => {
                if self.col < self.lines[self.row].len() {
                    self.col += 1;
                } else if self.row + 1 < self.lines.len() {
                    self.row += 1;
                    self.col = 0;
                }
            }
            (KeyCode::Up, _) => {
                self.row = self.row.saturating_sub(1);
            }
            (KeyCode::Down, _) => {
                self.row = (self.row + 1).min(self.lines.len() - 1);
            }
            (KeyCode::Home, _) | (KeyCode::Char('a'), true) => self.col = 0,
            (KeyCode::End, _) | (KeyCode::Char('e'), true) => self.col = self.lines[self.row].len(),
            (KeyCode::Char('u'), true) => {
                self.lines[self.row].drain(..self.col);
                self.col = 0;
            }
            (KeyCode::Char('k'), true) => {
                self.lines[self.row].truncate(self.col);
            }
            (KeyCode::Char('w'), true) => {
                // Word-back: drop spaces, then the word behind them.
                let mut at = self.col;
                while at > 0 && self.lines[self.row][at - 1] == ' ' {
                    at -= 1;
                }
                while at > 0 && self.lines[self.row][at - 1] != ' ' {
                    at -= 1;
                }
                self.lines[self.row].drain(at..self.col);
                self.col = at;
            }
            _ => return false,
        }
        self.clamp();
        true
    }
}
