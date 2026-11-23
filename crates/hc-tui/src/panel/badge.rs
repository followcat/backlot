//! A person row's right-hand tail (L4): the dim runner tag before the
//! state word and, while the line has room, the short model before the
//! tag. The state word owns the right edge, so neither can push it off;
//! the model drops first, then the tag, and the name takes what is
//! left.

use backlot_model::summary::RoleSummary;

use crate::chrome::text::display_width;
use crate::studio::glyphs::short_model;

/// The fewest cells the name keeps for itself before a tail piece is
/// dropped — enough for a person's own name plus the cut mark.
const MIN_NAME: u16 = 6;
/// The most cells the model may take — [`short_model`] already cuts
/// to 10, this keeps a hostile document string honest.
const MAX_MODEL: u16 = 10;

/// What a row can carry beside the name.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct Tail {
    /// The name column's width once the tail took its cells.
    pub name_width: u16,
    /// The short model, drawn before the runner tag.
    pub model: Option<String>,
    /// The runner tag, drawn before the state word.
    pub runner: Option<String>,
}

impl Tail {
    /// The runner's width in cells.
    pub(crate) fn runner_width(&self) -> u16 {
        self.runner
            .as_deref()
            .map(|r| display_width(r) as u16)
            .unwrap_or(0)
    }

    /// The model's width in cells.
    pub(crate) fn model_width(&self) -> u16 {
        self.model
            .as_deref()
            .map(|m| display_width(m) as u16)
            .unwrap_or(0)
    }
}

/// The tail for a `row_width`-wide line whose state word is
/// `state_width` cells: `│ <mark> <name> [model] [runner] <state> │`.
/// Every piece keeps one blank cell before it, including the state, so
/// the tag and model never touch it. The state's cells are reserved
/// first, then the name's floor; a piece that would eat into the floor
/// is dropped — the model before the runner.
pub(crate) fn tail(role: &RoleSummary, state_width: u16, row_width: u16) -> Tail {
    // Cells between the name column (x+4) and the state's first cell,
    // one of which is the blank before the state.
    let room = row_width.saturating_sub(1 + state_width).saturating_sub(4);
    let mut avail = room.saturating_sub(1);
    let runner = role
        .runner
        .as_deref()
        .filter(|runner| !runner.is_empty())
        .map(str::to_owned)
        .filter(|runner| {
            let width = display_width(runner) as u16;
            if avail >= width + 1 + MIN_NAME {
                avail -= width + 1;
                true
            } else {
                false
            }
        });
    let model = role
        .model
        .as_deref()
        .map(short_model)
        .filter(|model| !model.is_empty())
        .filter(|model| {
            let width = (display_width(model) as u16).min(MAX_MODEL);
            if avail >= width + 1 + MIN_NAME {
                avail -= width + 1;
                true
            } else {
                false
            }
        });
    Tail {
        name_width: avail,
        model,
        runner,
    }
}
