//! Dropping notices that no longer stand (S9d): at send time a
//! queued message that carries a `source_seq` — a stage notice or a
//! nudge — is checked against what that source says now. When the
//! source has moved on, `message.dropped` is recorded and nothing is
//! sent; the drop still counts as handled so the notice is never
//! re-queued. A message with no `source_seq` (`tell`, `ask`,
//! `dispatch`, the `plan` request) and reports are never checked —
//! a person wrote them and the words stand.

use backlot_model::design::{DesignGate, DesignPhase, DesignSlot, SignMode};
use backlot_model::design_steps::{has_version_plan, last_call_after};
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
/// or `None` when its source still says what it said. A notice
/// addressed to a role the ledger no longer wants live — a closed
/// actor, or everyone once `studio.finished` lands — is dead mail
/// whatever its label (S12f): nothing is coming back to read it,
/// so it drops `no longer wanted` instead of sitting in the
/// outbox forever. `user` is always there. Otherwise an unknown
/// label or a missing source record is left alone: the table
/// drops only what it can prove.
pub(crate) fn stale_reason(state: &StudioState, m: &Message) -> Option<&'static str> {
    let seq = m.source_seq?;
    if m.to != "user"
        && !backlot_model::wanted::wanted_roles(state)
            .iter()
            .any(|r| r.role == m.to)
    {
        return Some("no longer wanted");
    }
    match notices::label(m)? {
        // The attention family all die the same death: the raise
        // they point at cleared.
        "attention" | "question" | "stalled" | "lost" | "owed" | "over budget" => state
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
        "nudge" => (!next_steps(state)
            .iter()
            .any(|s| s.key == seq && s.owner == m.to))
        .then_some("step done"),
        "design-open" => {
            let gate_on = state
                .studio
                .as_ref()
                .is_some_and(|s| s.design_gate == DesignGate::Required);
            let same_req = state
                .requirements
                .iter()
                .rfind(|r| r.version == state.requirement_version)
                .is_some_and(|r| r.seq == seq);
            (!gate_on || !same_req || state.design_phase() != DesignPhase::Idle)
                .then_some("design moved on")
        }
        "design-proposal" => {
            let matches_round = state.design_phase() == DesignPhase::WaitingProposal
                && state
                    .design
                    .as_ref()
                    .filter(|d| d.version == state.requirement_version)
                    .and_then(|d| d.current_round())
                    .is_some_and(|r| r.opened_seq == seq);
            (!matches_round).then_some("design moved on")
        }
        "design-buildability" => {
            let matches_entry = state.design_phase() == DesignPhase::WaitingBuildability
                && state
                    .design
                    .as_ref()
                    .filter(|d| d.version == state.requirement_version)
                    .and_then(|d| d.current_round())
                    .is_some_and(|r| {
                        r.entries
                            .iter()
                            .any(|e| e.seq == seq && e.slot == DesignSlot::Proposal)
                    });
            (!matches_entry).then_some("design moved on")
        }
        "design-summary" => {
            let matches_entry = state.design_phase() == DesignPhase::WaitingSummary
                && state
                    .design
                    .as_ref()
                    .filter(|d| d.version == state.requirement_version)
                    .and_then(|d| d.current_round())
                    .is_some_and(|r| {
                        r.entries
                            .iter()
                            .any(|e| e.seq == seq && e.slot == DesignSlot::Buildability)
                    });
            (!matches_entry).then_some("design moved on")
        }
        "design-supplement" => {
            let matches_entry = state.design_phase() == DesignPhase::WaitingUser
                && state
                    .design
                    .as_ref()
                    .filter(|d| d.version == state.requirement_version)
                    .and_then(|d| d.current_round())
                    .is_some_and(|r| {
                        r.entries
                            .iter()
                            .any(|e| e.seq == seq && e.slot == DesignSlot::Supplement)
                    });
            (!matches_entry).then_some("design moved on")
        }
        "design-signed" => {
            let still_owed = state.design_phase() == DesignPhase::Signed
                && state
                    .design
                    .as_ref()
                    .filter(|d| d.version == state.requirement_version)
                    .is_some_and(|d| {
                        d.signed_seq == Some(seq)
                            && last_call_after(state, seq).is_none()
                            && !state
                                .tasks
                                .values()
                                .any(|t| t.created_seq > seq || t.requirement_version == d.version)
                            && (d.sign_mode != Some(SignMode::Approved)
                                || !has_version_plan(state, d.version))
                    });
            (!still_owed).then_some("design moved on")
        }
        // `recovered`, `denied`, `req` and unlabelled sources keep.
        _ => None,
    }
}

/// Whether a queued message should be dropped before delivery — either
/// because its `source_seq` notice went stale ([`stale_reason`]), or
/// because it targets a specific actor id (`actor-<task>-c<n>`) that
/// has closed for good with no pending resume and no live terminal
/// for that actor id.
///
/// Fixed-role mail (`producer`, `director`, `writer`), user mail, and
/// messages for paused, recovering, reopened, restarting, or still-live
/// actors are never dropped here.
pub(crate) fn drop_reason(
    studio: &str,
    state: &StudioState,
    live: &[crate::reconcile::LiveTerminal],
    m: &Message,
) -> Option<&'static str> {
    if let Some(reason) = stale_reason(state, m) {
        return Some(reason);
    }
    actor_closed_reason(studio, state, live, m)
}

fn actor_closed_reason(
    studio: &str,
    state: &StudioState,
    live: &[crate::reconcile::LiveTerminal],
    m: &Message,
) -> Option<&'static str> {
    backlot_model::state::actor_task(&m.to)?;
    if backlot_model::wanted::wanted_roles(state)
        .iter()
        .any(|r| r.role == m.to)
    {
        return None;
    }
    if actor_has_live_terminal(studio, state, live, &m.to) {
        return None;
    }
    Some(backlot_model::event::ACTOR_CLOSED)
}

fn actor_has_live_terminal(
    studio: &str,
    state: &StudioState,
    live: &[crate::reconcile::LiveTerminal],
    role: &str,
) -> bool {
    let view = crate::reconcile::studio_view(studio, live);
    let Some(term_name) = crate::reconcile::terminal_name(studio, role) else {
        return false;
    };
    let Some(live_addr) = view.get(&term_name) else {
        return false;
    };
    let owned_by_other = state.roles.iter().any(|(other_role, other)| {
        other_role != role
            && crate::reconcile::terminal_name(studio, other_role).as_deref()
                == Some(term_name.as_str())
            && (other.live || other.terminal.as_deref() == Some(live_addr.as_str()))
    });
    match state.roles.get(role) {
        Some(r) if r.closed && !r.live => {
            !owned_by_other && r.terminal.as_deref() == Some(live_addr.as_str())
        }
        Some(r) if !r.live => false,
        Some(r) => {
            !owned_by_other
                && r.terminal
                    .as_deref()
                    .is_none_or(|t| t == live_addr.as_str())
        }
        None => !owned_by_other,
    }
}
