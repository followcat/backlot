//! Opening-design sign-off choice dialog (`StudioDialog::DesignSign`): shows
//! every proposed check verbatim ahead of activation and lets the person choose
//! to sign with checks, sign without checks (`--no-checks`), or cancel.

use backlot_model::event::CheckDecl;

use crate::studio::StudioKey;

/// The three choices offered when signing an opening design with proposed checks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DesignSignChoice {
    SignWithChecks,
    SignWithoutChecks,
    Cancel,
}

/// Modal state shared across Call Sheet, Lot Dock, and Design Reader (`a`).
#[derive(Debug, Clone)]
pub(crate) struct DesignSignDialog {
    pub(crate) title: String,
    pub(crate) key: StudioKey,
    pub(crate) summary_line: String,
    pub(crate) checks: Vec<CheckDecl>,
    /// 0 = sign & activate checks, 1 = sign without checks, 2 = cancel.
    pub(crate) selected: usize,
    pub(crate) scroll: usize,
}

impl DesignSignDialog {
    pub(crate) fn new(key: &StudioKey, file: Option<&str>, checks: &[CheckDecl]) -> Self {
        let doc = file.unwrap_or("design doc");
        let frozen = file.and_then(|f| {
            if f.ends_with("-final.md") {
                Some(f.to_owned())
            } else {
                f.strip_suffix(".md").map(|stem| format!("{stem}-final.md"))
            }
        });
        let freeze_note = match frozen.as_deref() {
            Some(final_file) => format!("Freeze `{final_file}` from `{doc}`"),
            None => format!("Freeze `{doc}`"),
        };
        let count = crate::hollywood::decision::checks_count_phrase(checks.len());
        Self {
            title: format!(
                "Sign off opening design? · {}",
                crate::studio_dialog::StudioDialog::place(key)
            ),
            key: key.clone(),
            summary_line: format!("{freeze_note} — signing off also activates {count}:"),
            checks: checks.to_vec(),
            selected: 0,
            scroll: 0,
        }
    }

    pub(crate) fn choices(&self) -> [(DesignSignChoice, String); 3] {
        let count = crate::hollywood::decision::checks_count_phrase(self.checks.len());
        [
            (
                DesignSignChoice::SignWithChecks,
                format!("Sign off and activate {count}"),
            ),
            (
                DesignSignChoice::SignWithoutChecks,
                "Sign off without checks (--no-checks)".to_owned(),
            ),
            (DesignSignChoice::Cancel, "Cancel".to_owned()),
        ]
    }

    pub(crate) fn chosen(&self) -> DesignSignChoice {
        match self.selected {
            0 => DesignSignChoice::SignWithChecks,
            1 => DesignSignChoice::SignWithoutChecks,
            _ => DesignSignChoice::Cancel,
        }
    }

    pub(crate) fn move_selection(&mut self, delta: isize) {
        self.selected = (self.selected as isize + delta).rem_euclid(3) as usize;
    }

    pub(crate) fn scroll_by(&mut self, delta: isize) {
        if delta < 0 {
            self.scroll = self.scroll.saturating_sub(delta.unsigned_abs());
        } else {
            self.scroll = self.scroll.saturating_add(delta as usize);
        }
    }
}
