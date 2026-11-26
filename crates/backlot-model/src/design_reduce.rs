//! Reducer fold functions for `design.*` events and `req.version` transitions.

use crate::design::{DesignEntryRecord, DesignPhase, DesignRound, DesignSlot, DesignState};
use crate::event::{Event, EventKind};
use crate::state::StudioState;

/// Retain any signed design in `design_history` and reset `state.design` for the new version.
pub(crate) fn apply_req_version(state: &mut StudioState) {
    if let Some(prev) = state.design.take()
        && prev.phase == DesignPhase::Signed
    {
        state.design_history.insert(prev.version, prev);
    }
}

fn ensure_design(state: &mut StudioState, version: u32) -> &mut DesignState {
    if state.design.as_ref().is_some_and(|d| d.version == version) {
        state.design.as_mut().expect("checked Some above")
    } else {
        state.design.insert(DesignState {
            version,
            ..DesignState::default()
        })
    }
}

/// Fold `design.*` events into `StudioState.design`.
pub(crate) fn apply_event(state: &mut StudioState, event: &Event) {
    let seq = event.seq;
    let ts = event.ts_ms;
    match &event.kind {
        EventKind::DesignOpened {
            version,
            round,
            suggest_skip,
            reason,
            doc_file,
        } => {
            let d = ensure_design(state, *version);
            d.round = *round;
            d.suggest_skip = *suggest_skip;
            if !doc_file.is_empty() {
                d.doc_file = doc_file.clone();
            }
            if *suggest_skip {
                d.phase = DesignPhase::SuggestSkip;
                d.skip_reason = reason.clone();
                d.waiting_since_ms = Some(ts);
            } else {
                d.phase = DesignPhase::WaitingProposal;
                d.skip_reason = None;
                d.waiting_since_ms = None;
                if !d.rounds.iter().any(|r| r.round == *round) {
                    d.rounds.push(DesignRound {
                        round: *round,
                        opened_seq: seq,
                        opened_ts_ms: ts,
                        ..DesignRound::default()
                    });
                }
            }
        }
        EventKind::DesignEntry {
            version,
            round,
            slot,
            file,
            source,
            refs,
            questions,
        } => {
            let d = ensure_design(state, *version);
            let round_idx = match d.rounds.iter().position(|r| r.round == *round) {
                Some(idx) => idx,
                None => {
                    d.rounds.push(DesignRound {
                        round: *round,
                        opened_seq: seq,
                        opened_ts_ms: ts,
                        ..DesignRound::default()
                    });
                    d.rounds.len() - 1
                }
            };
            d.rounds[round_idx].entries.push(DesignEntryRecord {
                seq,
                ts_ms: ts,
                by: event.by.role.clone(),
                slot: *slot,
                file: file.clone(),
                source: *source,
                refs: refs.clone(),
                questions: questions.clone(),
            });
            match slot {
                DesignSlot::Proposal => {
                    d.phase = DesignPhase::WaitingBuildability;
                    d.waiting_since_ms = None;
                }
                DesignSlot::Buildability => {
                    d.phase = DesignPhase::WaitingSummary;
                    d.waiting_since_ms = None;
                }
                DesignSlot::Summary => {
                    d.phase = DesignPhase::WaitingUser;
                    d.waiting_since_ms = Some(ts);
                    d.rounds[round_idx].summary_seq = Some(seq);
                    d.rounds[round_idx].waiting_since_ms = Some(ts);
                    d.rounds[round_idx].questions = questions.clone();
                }
                DesignSlot::UserInput | DesignSlot::Supplement => {}
            }
        }
        EventKind::DesignSigned {
            version,
            round,
            mode,
            doc_file,
            note,
        } => {
            let d = ensure_design(state, *version);
            d.round = *round;
            d.phase = DesignPhase::Signed;
            d.waiting_since_ms = None;
            d.sign_mode = Some(*mode);
            d.sign_note = note.clone();
            d.signed_seq = Some(seq);
            d.signed_ts_ms = Some(ts);
            if !doc_file.is_empty() {
                d.doc_file = doc_file.clone();
            }
        }
        _ => {}
    }
}
