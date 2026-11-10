//! Dropping notices that no longer stand (S9d): at send time a
//! queued message that carries a `source_seq` — a stage notice or a
//! nudge — is checked against what that source says now. When the
//! source has moved on, `message.dropped` is recorded and nothing is
//! sent; the drop still counts as handled so the notice is never
//! re-queued. A message with no `source_seq` (`tell`, `ask`,
//! `dispatch`, the `plan` request) and reports are never checked —
//! a person wrote them and the words stand.

use backlot_model::state::{DeliveryRecord, Message, StudioState, Task, TaskStatus};
use backlot_model::steps::next_steps;

use crate::notices;

/// The task and the delivery `seq` names, when it names one.
fn delivery_at(state: &StudioState, seq: u64) -> Option<(&Task, &DeliveryRecord)> {
    state
        .tasks
        .values()
        .find_map(|t| t.deliveries.iter().find(|d| d.seq == seq).map(|d| (t, d)))
}

/// The task holding a review recorded at `seq`, when one does.
fn review_at(state: &StudioState, seq: u64) -> Option<&Task> {
    state
        .tasks
        .values()
        .find(|t| t.reviews.iter().any(|r| r.seq == seq))
}

/// The task holding a `task.state` change recorded at `seq`.
fn change_at(state: &StudioState, seq: u64) -> Option<&Task> {
    state
        .tasks
        .values()
        .find(|t| t.state_changes.iter().any(|c| c.seq == seq))
}

/// Why `m` no longer stands — the phrase `message.dropped` records —
/// or `None` when its source still says what it said. An unknown
/// label or a missing source record is left alone: the table drops
/// only what it can prove.
pub(crate) fn stale_reason(state: &StudioState, m: &Message) -> Option<&'static str> {
    let seq = m.source_seq?;
    match notices::label(m)? {
        // The attention family all die the same death: the raise
        // they point at cleared.
        "attention" | "stalled" | "lost" | "owed" => state
            .attention
            .iter()
            .any(|a| a.seq == seq && !a.open)
            .then_some("attention cleared"),
        "delivered" => {
            let (t, d) = delivery_at(state, seq)?;
            if t.review_of(d.n).is_some() {
                Some("delivery reviewed")
            } else if t.deliveries.iter().any(|o| o.seq > seq) {
                Some("superseded")
            } else {
                t.status.is_terminal().then_some("task closed")
            }
        }
        "rework" => {
            let t = review_at(state, seq)?;
            if t.deliveries.iter().any(|d| d.seq > seq) {
                Some("superseded")
            } else {
                (t.status != TaskStatus::Running).then_some("no longer running")
            }
        }
        "at-cap" => {
            let t = review_at(state, seq)?;
            (t.status != TaskStatus::AtCap).then_some("no longer at cap")
        }
        "blocked" => {
            let t = change_at(state, seq)?;
            (t.status != TaskStatus::Blocked).then_some("no longer blocked")
        }
        "resumed" => {
            let t = change_at(state, seq)?;
            t.status.is_terminal().then_some("task closed")
        }
        "nudge" => (!next_steps(state).iter().any(|s| s.key == seq)).then_some("step done"),
        // `recovered`, `denied`, `req` and unlabelled sources keep.
        _ => None,
    }
}
