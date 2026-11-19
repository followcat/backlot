//! The S3 native dialogs' model (§2, A6.2): what each kind asks,
//! what its Enter/y answers, and the multi-line editor the Tell and
//! Revise bodies share. The app owns opening, keys, mouse and the
//! submit — this file keeps the data and the small pure rules so the
//! render and the tests read them without an `App`.

use crate::chrome::text_editor::TextEditor;
use crate::studio::StudioKey;

/// A reason-taking command (§2.1) — the dialog's Enter builds the
/// action its `for` names.
#[derive(Debug, Clone)]
pub(crate) enum ReasonFor {
    /// `task <verb> --task <id> --reason <r>` — pause/resume/abandon
    /// require the reason; takeover's stays optional in the parser.
    TaskVerb {
        key: StudioKey,
        task: String,
        verb: &'static str,
    },
    /// `studio finish [--reason <r>]` — optional.
    Finish { key: StudioKey },
    /// `card cancel <id> [--reason <r>]` — optional.
    CardCancel { id: String },
}

/// A confirmed command (§2.2).
#[derive(Debug, Clone)]
pub(crate) enum ConfirmFor {
    /// `close --person <role>` — "the role's terminal is removed…".
    Close { key: StudioKey, role: String },
    /// `stage stop` (G1) — "the stage's terminal exits…".
    StageStop { machine: Option<String> },
    /// `attention clear <seq>` — a stale prompt the role already
    /// left behind.
    ClearAttention {
        key: StudioKey,
        role: String,
        seq: u64,
    },
    /// The D9 override — a director-routed prompt the user answers
    /// after confirming (`backlot approve --as user`).
    DirOverride {
        key: StudioKey,
        role: String,
        allow: bool,
    },
}

/// What the multi-line dialogs' focus rings walk through — Tab
/// advances, Shift-Tab backs up, Enter on Send submits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DialogField {
    Body,
    Summary,
    Send,
}

/// `Tell <role> · <studio>[ · <machine>]` (§2.3).
pub(crate) struct TellDialog {
    /// The dialog title line — `Tell alice · acme-shop · host-b`.
    pub(crate) title: String,
    pub(crate) key: StudioKey,
    /// The role key `backlot tell` takes.
    pub(crate) role: String,
    pub(crate) body: BodyEditor,
    pub(crate) field: DialogField,
    /// A refused submit's inline error (`empty message`).
    pub(crate) error: Option<String>,
}

/// `Revise requirement (v<n> → v<n+1>) · <studio>[ · <machine>]` (§2.4).
pub(crate) struct ReviseDialog {
    pub(crate) title: String,
    pub(crate) key: StudioKey,
    /// `None` while the prefill fetch is in flight; `Some` once the
    /// body editor holds the current text (empty on a failed fetch —
    /// the error field explains).
    pub(crate) body: Option<BodyEditor>,
    pub(crate) summary: TextEditor,
    pub(crate) field: DialogField,
    /// `^E` is local-only — a remote studio's body stays in the
    /// dialog's own editor.
    pub(crate) local: bool,
    /// A failed prefill or a refused submit explains itself here.
    pub(crate) error: Option<String>,
    /// Bumps per `$EDITOR` write so a closed editor's late file is
    /// never re-read.
    pub(crate) editor_gen: u64,
}

/// `Pause task t1 · acme-shop · Local` (§2.1).
pub(crate) struct ReasonDialog {
    pub(crate) title: String,
    pub(crate) reason: TextEditor,
    /// The error an empty required submit shows.
    pub(crate) required: bool,
    pub(crate) error: Option<String>,
    pub(crate) for_: ReasonFor,
}

/// `[Enter / y] Confirm · [Esc] Cancel` (§2.2).
pub(crate) struct ConfirmDialog {
    pub(crate) title: String,
    /// The explanation the spec sets per kind.
    pub(crate) body: String,
    pub(crate) for_: ConfirmFor,
}

/// The four dialog kinds.
pub(crate) enum StudioDialog {
    Reason(ReasonDialog),
    Confirm(ConfirmDialog),
    Tell(TellDialog),
    Revise(ReviseDialog),
}

impl StudioDialog {
    /// `Reason <verb> task t1 · acme-shop · Local` titles and the
    /// shared `reason: ` line.
    pub(crate) fn reason(title: String, required: bool, for_: ReasonFor) -> StudioDialog {
        StudioDialog::Reason(ReasonDialog {
            title,
            reason: TextEditor::new("", false),
            required,
            error: None,
            for_,
        })
    }

    /// The confirm card — its body is the spec's wording per kind.
    pub(crate) fn confirm(title: String, body: String, for_: ConfirmFor) -> StudioDialog {
        StudioDialog::Confirm(ConfirmDialog { title, body, for_ })
    }

    /// The machine suffix every §2 title shares: `· host-b`, or `·
    /// Local` when the studio sits on this core.
    pub(crate) fn place(key: &StudioKey) -> String {
        format!("{} · {}", key.id, key.machine.as_deref().unwrap_or("Local"))
    }

    /// The D9 override confirm (§2.2): a director-routed prompt the
    /// user answers themself, worded as the spec sets it.
    pub(crate) fn dir_override(key: &StudioKey, role: &str, allow: bool) -> StudioDialog {
        StudioDialog::Confirm(ConfirmDialog {
            title: format!("Answer {}'s prompt? · {}", role, Self::place(key)),
            body: format!(
                "This prompt is routed to the director. Answering \
                 {}s it as the user — the director's own wait clears \
                 with it.",
                if allow { "allow" } else { "deny" }
            ),
            for_: ConfirmFor::DirOverride {
                key: key.clone(),
                role: role.to_owned(),
                allow,
            },
        })
    }
}

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

#[cfg(test)]
#[path = "studio_dialog_tests.rs"]
mod tests;
