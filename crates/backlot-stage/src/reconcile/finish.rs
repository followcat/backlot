//! The finish sweep: `studio.finished` releases every role, and the
//! stop path kills its terminal — but the core may refuse the
//! `terminal.rm` that follows while the kill is still landing. The
//! record outlives its program as `finished`, and every client keeps
//! drawing the dead pane (`new`, `cast · <id>`, `producer` with
//! "Terminal unavailable") until someone closes it by hand.
//!
//! The record is the stage's to remove: the terminal carries the
//! studio's own `backlot.studio` label, so ownership is a label match,
//! never a name. A shell is the user's own and a finished studio keeps
//! its shells (`guide.md` › "Starting"), so those stay. Nothing is
//! recorded — a terminal record is not history, the ledger is — and a
//! record already gone is a tolerated `not_found`.

use backlot_model::state::StudioState;

use super::{Action, LiveTerminal};

/// The removal each of this studio's finished records owes, once the
/// studio has finished. Pure: the caller supplies the `finished`
/// records its inventory holds and carries the actions out.
pub fn finish_sweep(studio: &str, state: &StudioState, finished: &[LiveTerminal]) -> Vec<Action> {
    if state.finished.is_none() {
        return Vec::new();
    }
    finished
        .iter()
        .filter(|term| owned(studio, term))
        .map(|term| Action::RemoveFinished {
            terminal: term.name.clone(),
            address: term.address.clone(),
        })
        .collect()
}

/// This studio's own role or pane record: the `backlot.studio` label
/// names it, and a `backlot.shell` is the user's own terminal.
fn owned(studio: &str, term: &LiveTerminal) -> bool {
    term.labels
        .get("backlot.studio")
        .is_some_and(|s| s == studio)
        && !term.labels.contains_key("backlot.shell")
}
