//! A person row's right-hand tail (L4): the dim runner tag before the
//! state word and, while the line has room, the short model before the
//! tag. The state word owns the right edge, and the badge spends only
//! cells left over after the whole name and task — the model goes first,
//! then the tag, before either can cost the name a cell.

use backlot_model::summary::RoleSummary;

use crate::chrome::text::display_width;
use crate::studio::glyphs::short_model;

/// The most cells the model may take — [`short_model`] already cuts to
/// 10, this keeps a hostile document string honest.
const MAX_MODEL: u16 = 10;

/// What a row can carry beside the name.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct Tail {
    /// The name column's width — the full name at least, or all the
    /// cells that are left when the name alone overflows.
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
/// `state_width` cells and whose name plus flag need `name_need` cells:
/// `│ <mark> <name> [model] [runner] <state> │`. Every piece keeps one
/// blank cell before it, including the state.
///
/// The name is reserved first. The runner then takes what is left over;
/// the model takes what is left after that. So as the row narrows the
/// model disappears first, then the runner — the name and task lose a
/// cell only when they alone overflow.
pub(crate) fn tail(role: &RoleSummary, name_need: u16, state_width: u16, row_width: u16) -> Tail {
    // Cells between the name column (x+4) and the state's first cell,
    // one of which is the blank before the state.
    let before_state = row_width
        .saturating_sub(1 + state_width)
        .saturating_sub(4)
        .saturating_sub(1);
    let mut spare = before_state.saturating_sub(name_need);
    let mut used = 0;
    let runner = role
        .runner
        .as_deref()
        .filter(|runner| !runner.is_empty())
        .map(str::to_owned)
        .filter(|runner| {
            let width = display_width(runner) as u16;
            if spare > width {
                spare -= width + 1;
                used += width + 1;
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
            if spare > width {
                spare -= width + 1;
                used += width + 1;
                true
            } else {
                false
            }
        });
    Tail {
        name_width: before_state.saturating_sub(used),
        model,
        runner,
    }
}
