//! Derived `decisions/current.md` builder synced under `events.lock`
//! whenever a `purpose = "decision"` message is appended to the ledger.

use std::fs;
use std::path::Path;

use backlot_model::decisions::render_current_md;
use backlot_model::event::{Event, EventKind};
use backlot_model::state::StudioState;

/// Refresh `decisions/current.md` when `stamped` includes at least one
/// `purpose = "decision"` message (called inside `store::append_many`'s
/// `post_copy` hook under `events.lock`).
pub(crate) fn sync_on_append(dir: &Path, state: &StudioState, stamped: &[Event]) {
    let touched = stamped.iter().any(|e| {
        matches!(
            &e.kind,
            EventKind::MessageQueued { purpose: Some(p), .. } if p == "decision"
        )
    });
    if !touched {
        return;
    }
    let body = render_current_md(state, |rel| fs::read_to_string(dir.join(rel)).ok());
    if let Err(e) =
        crate::design_doc::atomic_write(&dir.join("decisions/current.md"), body.as_bytes())
    {
        crate::stagelog::stage_log!("decisions/current.md write failed: {e}");
    }
}
