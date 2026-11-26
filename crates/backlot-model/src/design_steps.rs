//! Opening design step, writer-wanted, and plan-readiness queries.

use crate::design::{DesignGate, DesignPhase, DesignSlot, SignMode};
use crate::event::PlanKind;
use crate::state::{Message, StudioState};
use crate::steps::Step;

/// Whether the ledger already holds a complete `(Plan, Acceptance)` pair.
pub fn has_usable_plan(state: &StudioState) -> bool {
    let has_plan = state.plans.iter().any(|p| p.plan_kind == PlanKind::Plan);
    let has_acc = state
        .plans
        .iter()
        .any(|p| p.plan_kind == PlanKind::Acceptance);
    has_plan && has_acc
}

fn first_version_plan(state: &StudioState, kind: PlanKind, version: u32) -> Option<(u64, u64)> {
    state
        .plans
        .iter()
        .filter(|p| p.plan_kind == kind && p.requirement_version == version)
        .map(|p| (p.seq, p.ts_ms))
        .min_by_key(|(s, _)| *s)
}

fn first_plan_after(state: &StudioState, kind: PlanKind, seq: u64) -> Option<(u64, u64)> {
    state
        .plans
        .iter()
        .filter(|p| p.plan_kind == kind && p.seq > seq)
        .map(|p| (p.seq, p.ts_ms))
        .min_by_key(|(s, _)| *s)
}

/// Whether both `Plan` and `Acceptance` exist for `version`.
pub fn has_version_plan(state: &StudioState, version: u32) -> bool {
    first_version_plan(state, PlanKind::Plan, version).is_some()
        && first_version_plan(state, PlanKind::Acceptance, version).is_some()
}

/// Latest `plan` or `dispatch` command message recorded after `after_seq`.
pub fn last_call_after(state: &StudioState, after_seq: u64) -> Option<&Message> {
    state.messages.iter().rev().find(|m| {
        m.seq > after_seq && matches!(m.purpose.as_deref(), Some("plan") | Some("dispatch"))
    })
}

/// True when `design_gate == required` and a requirement version is active.
pub(crate) fn design_gate_active(state: &StudioState) -> bool {
    state
        .studio
        .as_ref()
        .is_some_and(|s| s.design_gate == DesignGate::Required)
        && state.requirement_version > 0
}

fn step_dispatch(key: u64, since_ms: u64) -> Step {
    Step {
        key,
        label: "dispatch tasks".into(),
        owner: "producer".into(),
        command: "backlot dispatch --file <file>".into(),
        since_ms,
    }
}

fn step_plan(key: u64, label: &str, since_ms: u64) -> Step {
    Step {
        key,
        label: label.into(),
        owner: "writer".into(),
        command: "backlot plan publish --kind plan --file <file> (and --kind acceptance)".into(),
        since_ms,
    }
}

/// Owed opening-design or post-sign step when `design_gate == required`.
pub(crate) fn opening_step(state: &StudioState) -> Option<Step> {
    let rv = state.requirement_version;
    match state.design_phase() {
        DesignPhase::Idle => {
            let req = state.requirements.iter().rfind(|r| r.version == rv)?;
            let since_ms = state
                .last_by_role
                .values()
                .find(|e| e.seq == req.seq)
                .map_or(0, |e| e.ts_ms);
            Some(Step {
                key: req.seq,
                label: "open design".into(),
                owner: "producer".into(),
                command: "backlot design open".into(),
                since_ms,
            })
        }
        DesignPhase::SuggestSkip => {
            let d = state.design.as_ref()?;
            let key = state
                .requirements
                .iter()
                .rfind(|r| r.version == rv)
                .map_or(0, |r| r.seq);
            Some(Step {
                key,
                label: "review skip suggestion".into(),
                owner: "user".into(),
                command: "backlot design sign --skip (or open round 1 with backlot design open)"
                    .into(),
                since_ms: d.waiting_since_ms.unwrap_or(0),
            })
        }
        DesignPhase::WaitingProposal => {
            let round = state.design.as_ref()?.current_round()?;
            Some(Step {
                key: round.opened_seq,
                label: "write design proposal".into(),
                owner: "writer".into(),
                command: "backlot design write --slot proposal --file <file>".into(),
                since_ms: round.opened_ts_ms,
            })
        }
        DesignPhase::WaitingBuildability => {
            let round = state.design.as_ref()?.current_round()?;
            let prop = round
                .entries
                .iter()
                .rev()
                .find(|e| e.slot == DesignSlot::Proposal)?;
            Some(Step {
                key: prop.seq,
                label: "review buildability".into(),
                owner: "director".into(),
                command: "backlot design write --slot buildability --file <file>".into(),
                since_ms: prop.ts_ms,
            })
        }
        DesignPhase::WaitingSummary => {
            let round = state.design.as_ref()?.current_round()?;
            let build = round
                .entries
                .iter()
                .rev()
                .find(|e| e.slot == DesignSlot::Buildability)?;
            Some(Step {
                key: build.seq,
                label: "summarize design round".into(),
                owner: "producer".into(),
                command: "backlot design write --slot summary --file <file>".into(),
                since_ms: build.ts_ms,
            })
        }
        DesignPhase::WaitingUser => {
            let d = state.design.as_ref()?;
            let round = d.current_round()?;
            let sum_seq = round
                .summary_seq
                .or_else(|| d.latest_summary_seq())
                .unwrap_or(round.opened_seq);
            let since_ms = round
                .waiting_since_ms
                .or(d.waiting_since_ms)
                .unwrap_or(round.opened_ts_ms);
            Some(Step {
                key: sum_seq,
                label: "review design & decide".into(),
                owner: "user".into(),
                command: "backlot design sign (or backlot tell producer --text <reply>)".into(),
                since_ms,
            })
        }
        DesignPhase::Signed => {
            let d = state.design.as_ref()?;
            let signed_seq = d.signed_seq.unwrap_or(0);
            let signed_ts = d.signed_ts_ms.unwrap_or(0);
            let plan_label = if d.sign_mode == Some(SignMode::Approved) {
                "derive plan & acceptance"
            } else {
                "write plan"
            };
            match last_call_after(state, signed_seq) {
                Some(m) if m.purpose.as_deref() == Some("dispatch") => {
                    if m.delivered && !state.tasks.values().any(|t| t.created_seq > m.seq) {
                        Some(Step {
                            key: m.seq,
                            label: "tasks".into(),
                            owner: "director".into(),
                            command:
                                "backlot task new --title <title> --kind <kind> --acceptance-file <file>"
                                    .into(),
                            since_ms: m.delivered_ms.unwrap_or(m.ts_ms),
                        })
                    } else {
                        None
                    }
                }
                Some(m) => {
                    let plan = first_plan_after(state, PlanKind::Plan, m.seq);
                    let acc = first_plan_after(state, PlanKind::Acceptance, m.seq);
                    match (plan, acc) {
                        (Some((s1, t1)), Some((s2, t2))) => {
                            Some(step_dispatch(s1.max(s2), t1.max(t2)))
                        }
                        _ => Some(step_plan(m.seq, plan_label, m.ts_ms)),
                    }
                }
                None => {
                    if state
                        .tasks
                        .values()
                        .any(|t| t.created_seq > signed_seq || t.requirement_version == rv)
                    {
                        return None;
                    }
                    if d.sign_mode == Some(SignMode::Approved) {
                        let plan = first_version_plan(state, PlanKind::Plan, rv);
                        let acc = first_version_plan(state, PlanKind::Acceptance, rv);
                        match (plan, acc) {
                            (Some((s1, t1)), Some((s2, t2))) => {
                                Some(step_dispatch(s1.max(s2), t1.max(t2)))
                            }
                            _ => Some(step_plan(signed_seq, plan_label, signed_ts)),
                        }
                    } else if has_usable_plan(state) {
                        Some(step_dispatch(signed_seq, signed_ts))
                    } else {
                        Some(step_plan(signed_seq, plan_label, signed_ts))
                    }
                }
            }
        }
    }
}

/// Whether `writer` must stay live for the opening design round or post-sign plan derivation.
pub(crate) fn writer_wanted_for_design(state: &StudioState) -> bool {
    match state.design_phase() {
        DesignPhase::Idle | DesignPhase::SuggestSkip => false,
        DesignPhase::WaitingProposal
        | DesignPhase::WaitingBuildability
        | DesignPhase::WaitingSummary
        | DesignPhase::WaitingUser => true,
        DesignPhase::Signed => {
            let Some(d) = state.design.as_ref() else {
                return false;
            };
            let signed_seq = d.signed_seq.unwrap_or(0);
            if let Some(m) = last_call_after(state, signed_seq) {
                return m.purpose.as_deref() == Some("plan");
            }
            if state
                .tasks
                .values()
                .any(|t| t.created_seq > signed_seq || t.requirement_version == d.version)
            {
                return false;
            }
            match d.sign_mode {
                Some(SignMode::Approved) => true,
                Some(SignMode::Skipped) => !has_usable_plan(state),
                None => false,
            }
        }
    }
}
