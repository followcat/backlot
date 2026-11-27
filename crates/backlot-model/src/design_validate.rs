//! Append-time validation rules for `design.opened`, `design.entry`, and `design.signed`.

use crate::design::{DesignGate, DesignPhase, DesignQuestion, DesignSlot, EntrySource, SignMode};
use crate::event::{EventKind, NewEvent};
use crate::refusal::Refusal;
use crate::state::{StudioConfig, StudioState};

const MAX_QUESTIONS: usize = 3;

/// Enforce `design_gate = required` before planning, dispatching, creating, or casting tasks.
pub(crate) fn require_signed(studio: &StudioConfig, state: &StudioState) -> Result<(), Refusal> {
    if studio.design_gate == DesignGate::Required && state.design_phase() != DesignPhase::Signed {
        return Err(Refusal::DesignNotSigned(state.requirement_version));
    }
    Ok(())
}

fn ref_exists(state: &StudioState, seq: u64) -> bool {
    state.messages.iter().any(|m| m.seq == seq)
        || state
            .design
            .iter()
            .chain(state.design_history.values())
            .flat_map(|d| d.rounds.iter())
            .flat_map(|r| r.entries.iter())
            .any(|e| e.seq == seq)
}

fn check_version(state: &StudioState, version: u32) -> Result<(), Refusal> {
    if version == 0 || version > state.requirement_version {
        return Err(Refusal::UnknownReqVersion(version));
    }
    if version != state.requirement_version {
        return Err(Refusal::DesignWrongVersion {
            current: state.requirement_version,
            got: version,
        });
    }
    Ok(())
}

fn check_questions(questions: &[DesignQuestion]) -> Result<(), Refusal> {
    if questions.len() > MAX_QUESTIONS {
        return Err(Refusal::InvalidDesign(format!(
            "design summary accepts at most {MAX_QUESTIONS} questions, got {}",
            questions.len()
        )));
    }
    for q in questions {
        if q.id.trim().is_empty()
            || q.prompt.trim().is_empty()
            || q.recommendation.trim().is_empty()
        {
            return Err(Refusal::InvalidDesign(
                "each design question requires non-empty id, prompt, and recommendation".into(),
            ));
        }
        if let Some(section_ref) = &q.section_ref
            && section_ref.trim().is_empty()
        {
            return Err(Refusal::InvalidDesign(
                "design question section_ref cannot be blank when set".into(),
            ));
        }
    }
    Ok(())
}

fn check_user_input_source(role: &str, source: Option<EntrySource>) -> Result<(), Refusal> {
    let ok = match role {
        "user" => matches!(source, None | Some(EntrySource::Tell)),
        "producer" => matches!(source, None | Some(EntrySource::ProducerNote)),
        "stage" => matches!(source, None | Some(EntrySource::Hook)),
        _ => false,
    };
    if ok {
        Ok(())
    } else {
        Err(Refusal::InvalidDesign(format!(
            "role '{role}' cannot record user_input with source {source:?}"
        )))
    }
}

pub(crate) fn validate_design(state: &StudioState, new: &NewEvent) -> Result<(), Refusal> {
    match &new.kind {
        EventKind::DesignOpened {
            version,
            round,
            suggest_skip,
            doc_file,
            ..
        } => {
            check_version(state, *version)?;
            if doc_file.trim().is_empty() {
                return Err(Refusal::InvalidDesign(
                    "design.opened requires a non-empty doc_file".into(),
                ));
            }
            match state.design_phase() {
                DesignPhase::Idle => {
                    if *suggest_skip {
                        if *round > 1 {
                            return Err(Refusal::DesignBadRound {
                                expected: 0,
                                got: *round,
                            });
                        }
                    } else if *round != 1 {
                        return Err(Refusal::DesignBadRound {
                            expected: 1,
                            got: *round,
                        });
                    }
                    Ok(())
                }
                DesignPhase::SuggestSkip => {
                    if *suggest_skip {
                        return Err(Refusal::DesignWrongPhase {
                            action: "open skip suggestion",
                            phase: DesignPhase::SuggestSkip,
                        });
                    }
                    if *round != 1 {
                        return Err(Refusal::DesignBadRound {
                            expected: 1,
                            got: *round,
                        });
                    }
                    Ok(())
                }
                DesignPhase::WaitingUser => {
                    if *suggest_skip {
                        return Err(Refusal::DesignWrongPhase {
                            action: "open skip suggestion",
                            phase: DesignPhase::WaitingUser,
                        });
                    }
                    let d = state
                        .design
                        .as_ref()
                        .ok_or(Refusal::NoActiveDesign(*version))?;
                    if !d.has_user_input_after_last_summary() {
                        return Err(Refusal::DesignNeedsUserInput);
                    }
                    let expected = d.round + 1;
                    if *round != expected {
                        return Err(Refusal::DesignBadRound {
                            expected,
                            got: *round,
                        });
                    }
                    Ok(())
                }
                DesignPhase::Signed => Err(Refusal::DesignAlreadySigned(*version)),
                phase => Err(Refusal::DesignWrongPhase {
                    action: "open design round",
                    phase,
                }),
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
            checks,
        } => {
            check_version(state, *version)?;
            let d = state
                .design
                .as_ref()
                .filter(|d| d.version == *version)
                .ok_or(Refusal::NoActiveDesign(*version))?;
            if d.phase == DesignPhase::Signed {
                return Err(Refusal::DesignAlreadySigned(*version));
            }
            if *round != d.round {
                return Err(Refusal::DesignBadRound {
                    expected: d.round,
                    got: *round,
                });
            }
            if file.trim().is_empty() {
                return Err(Refusal::InvalidDesign(
                    "design.entry requires a non-empty file".into(),
                ));
            }
            let expected_phase = match slot {
                DesignSlot::Proposal => DesignPhase::WaitingProposal,
                DesignSlot::Buildability => DesignPhase::WaitingBuildability,
                DesignSlot::Summary => DesignPhase::WaitingSummary,
                DesignSlot::UserInput | DesignSlot::Supplement => DesignPhase::WaitingUser,
            };
            if d.phase != expected_phase {
                return Err(Refusal::DesignWrongPhase {
                    action: "record design entry",
                    phase: d.phase,
                });
            }
            if !checks.is_empty() && *slot != DesignSlot::Buildability {
                return Err(Refusal::InvalidDesign(
                    "only buildability entries may propose checks".into(),
                ));
            }
            if *slot == DesignSlot::Buildability && !checks.is_empty() {
                let existing = state
                    .studio
                    .as_ref()
                    .map(|s| s.checks.as_slice())
                    .unwrap_or(&[]);
                crate::cast::validate_proposed_checks(existing, checks)?;
            }
            if *slot == DesignSlot::UserInput {
                if !questions.is_empty() {
                    return Err(Refusal::InvalidDesign(
                        "only summary entries may carry questions".into(),
                    ));
                }
                check_user_input_source(&new.by.role, *source)?;
            } else {
                if source.is_some() {
                    return Err(Refusal::InvalidDesign(
                        "only user_input entries may carry source".into(),
                    ));
                }
                if *slot == DesignSlot::Summary {
                    check_questions(questions)?;
                } else if !questions.is_empty() {
                    return Err(Refusal::InvalidDesign(
                        "only summary entries may carry questions".into(),
                    ));
                }
                if *slot == DesignSlot::Supplement {
                    if refs.is_empty() {
                        return Err(Refusal::InvalidDesign(
                            "design supplement requires at least one --refs seq".into(),
                        ));
                    }
                    for &seq in refs {
                        if !ref_exists(state, seq) {
                            return Err(Refusal::InvalidDesign(format!(
                                "design supplement ref #{seq} does not name an existing message or design entry"
                            )));
                        }
                    }
                }
            }
            Ok(())
        }
        EventKind::DesignSigned {
            version,
            round,
            mode,
            doc_file,
            ..
        } => {
            check_version(state, *version)?;
            let d = state
                .design
                .as_ref()
                .filter(|d| d.version == *version)
                .ok_or(Refusal::NoActiveDesign(*version))?;
            if d.phase == DesignPhase::Signed {
                return Err(Refusal::DesignAlreadySigned(*version));
            }
            if *round != d.round {
                return Err(Refusal::DesignBadRound {
                    expected: d.round,
                    got: *round,
                });
            }
            match (mode, d.phase) {
                (SignMode::Approved, DesignPhase::WaitingUser) => {
                    if doc_file.trim().is_empty() {
                        return Err(Refusal::InvalidDesign(
                            "approved design.signed requires a non-empty doc_file".into(),
                        ));
                    }
                    Ok(())
                }
                (SignMode::Skipped, DesignPhase::SuggestSkip | DesignPhase::WaitingUser) => Ok(()),
                (_, phase) => Err(Refusal::DesignWrongPhase {
                    action: "sign off design",
                    phase,
                }),
            }
        }
        _ => Ok(()),
    }
}
