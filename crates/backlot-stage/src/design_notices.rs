//! Opening design notices (`design_gate == required`).

use std::collections::BTreeSet;
use std::path::Path;

use backlot_model::design::{DesignPhase, DesignSlot, SignMode};
use backlot_model::design_steps::{has_usable_plan, has_version_plan, last_call_after};
use backlot_model::state::StudioState;

use crate::notices::Notice;

/// Append stage notices owed by `req.version` and `design.*` when `design_gate == required`.
pub(crate) fn append(state: &StudioState, dir: &Path, sent: &BTreeSet<u64>, out: &mut Vec<Notice>) {
    let phase = state.design_phase();
    if let Some(req) = state
        .requirements
        .iter()
        .rfind(|r| r.version == state.requirement_version)
        && !sent.contains(&req.seq)
    {
        if phase == DesignPhase::Idle {
            out.push(Notice {
                source_seq: req.seq,
                to: "producer".into(),
                label: "design-open",
                body: format!(
                    "# requirement v{} — open design\n\nRequirement v{} was recorded (`{}` — {}).\n\n`design_gate` is `required`: start Round 1 with `backlot design open` before any plan, task, or dispatch (or use `backlot design open --suggest-skip --reason \"<why>\"` only for a tiny fix or an already-finalized external plan).\n",
                    req.version,
                    req.version,
                    dir.join(&req.file).display(),
                    req.summary,
                ),
            });
        }
        if req.version > 1 || phase != DesignPhase::Signed {
            let behind: Vec<&str> = if req.version > 1 {
                state
                    .tasks
                    .values()
                    .filter(|t| t.is_behind(state.requirement_version))
                    .map(|t| t.id.as_str())
                    .collect()
            } else {
                Vec::new()
            };
            let behind_line = if req.version <= 1 {
                String::new()
            } else if behind.is_empty() {
                "\nNo task is behind.\n".to_string()
            } else {
                format!(
                    "\nTasks now behind: {} (dispatched under an older version).\n",
                    behind.join(", ")
                )
            };
            let gate_note = if phase != DesignPhase::Signed {
                format!(
                    "\nDesign gate is required for v{}: wait for the writer to submit `proposal` before evaluating buildability (`backlot design write --slot buildability --file <file>`), and do not create or cast tasks (`backlot task new` / `backlot cast`) until the design is signed off.\n",
                    req.version
                )
            } else {
                String::new()
            };
            out.push(Notice {
                source_seq: req.seq,
                to: "director".into(),
                label: "req",
                body: format!(
                    "# requirement v{}\n\nThe requirement was set to v{}: `{}` — {}\n{behind_line}{gate_note}",
                    req.version,
                    req.version,
                    dir.join(&req.file).display(),
                    req.summary,
                ),
            });
        }
    }

    let Some(d) = state
        .design
        .as_ref()
        .filter(|d| d.version == state.requirement_version)
    else {
        return;
    };
    match d.phase {
        DesignPhase::WaitingProposal => {
            if let Some(round) = d.current_round()
                && !sent.contains(&round.opened_seq)
            {
                out.push(Notice {
                    source_seq: round.opened_seq,
                    to: "writer".into(),
                    label: "design-proposal",
                    body: format!(
                        "# design v{} round {} — proposal owed\n\nRound {} is open against `{}` (`{}`).\n\nGround claims in verified repository facts (`file:line`), present 2–3 distinct options with a comparison and Why-not notes plus one recommendation, and submit with:\n\n    backlot design write --slot proposal --file <file>\n",
                        d.version,
                        round.round,
                        round.round,
                        dir.join(format!("req/v{}.md", d.version)).display(),
                        dir.join(&d.doc_file).display(),
                    ),
                });
            }
        }
        DesignPhase::WaitingBuildability => {
            if let Some(round) = d.current_round()
                && let Some(prop) = round
                    .entries
                    .iter()
                    .rev()
                    .find(|e| e.slot == DesignSlot::Proposal)
                && !sent.contains(&prop.seq)
            {
                out.push(Notice {
                    source_seq: prop.seq,
                    to: "director".into(),
                    label: "design-buildability",
                    body: format!(
                        "# design v{} round {} — buildability review owed\n\nThe writer submitted the proposal (`{}`, assembled in `{}`).\n\nVerify runnable build/test commands, worktree/guard boundaries, and a file-disjoint task split, then submit:\n\n    backlot design write --slot buildability --file <file>\n\nDo not create or cast tasks until the design is signed off.\n",
                        d.version,
                        round.round,
                        dir.join(&prop.file).display(),
                        dir.join(&d.doc_file).display(),
                    ),
                });
            }
        }
        DesignPhase::WaitingSummary => {
            if let Some(round) = d.current_round()
                && let Some(build) = round
                    .entries
                    .iter()
                    .rev()
                    .find(|e| e.slot == DesignSlot::Buildability)
                && !sent.contains(&build.seq)
            {
                out.push(Notice {
                    source_seq: build.seq,
                    to: "producer".into(),
                    label: "design-summary",
                    body: format!(
                        "# design v{} round {} — summary owed\n\nThe director submitted the buildability review (`{}`, assembled in `{}`).\n\nWrite a concise convergence summary citing `#<seq>` (never paraphrasing the person's own words) with 0–3 structured questions and submit:\n\n    backlot design write --slot summary --file <file> [--question ...]\n",
                        d.version,
                        round.round,
                        dir.join(&build.file).display(),
                        dir.join(&d.doc_file).display(),
                    ),
                });
            }
        }
        DesignPhase::WaitingUser => {
            if let Some(round) = d.current_round() {
                for supp in round
                    .entries
                    .iter()
                    .filter(|e| e.slot == DesignSlot::Supplement && !sent.contains(&e.seq))
                {
                    let refs = if supp.refs.is_empty() {
                        "none".to_string()
                    } else {
                        supp.refs
                            .iter()
                            .map(|s| format!("#{s}"))
                            .collect::<Vec<_>>()
                            .join(", ")
                    };
                    out.push(Notice {
                        source_seq: supp.seq,
                        to: "producer".into(),
                        label: "design-supplement",
                        body: format!(
                            "# design v{} round {} — supplement by {}\n\n`{}` posted a targeted supplement (`{}`, refs: {refs}) in `{}`.\n\nReview it and refresh the summary or decision card if needed.\n",
                            d.version,
                            round.round,
                            supp.by,
                            supp.by,
                            dir.join(&supp.file).display(),
                            dir.join(&d.doc_file).display(),
                        ),
                    });
                }
            }
        }
        DesignPhase::Signed => {
            let Some(signed_seq) = d.signed_seq else {
                return;
            };
            if sent.contains(&signed_seq)
                || last_call_after(state, signed_seq).is_some()
                || state
                    .tasks
                    .values()
                    .any(|t| t.created_seq > signed_seq || t.requirement_version == d.version)
            {
                return;
            }
            let note_line = d
                .sign_note
                .as_deref()
                .filter(|s| !s.is_empty())
                .map(|n| format!(" (note: {n})"))
                .unwrap_or_default();
            match d.sign_mode {
                Some(SignMode::Approved) if !has_version_plan(state, d.version) => {
                    let doc = dir.join(&d.doc_file);
                    out.push(Notice {
                        source_seq: signed_seq,
                        to: "writer".into(),
                        label: "design-signed",
                        body: format!(
                            "# design v{} signed off (approved)\n\nThe person approved the opening design at round {}: `{}`{note_line}.\n\nDerive the formal `plan` and `acceptance` list from `{}` — every acceptance item must cite the decided section (e.g. `design v{} §R{}.W` or `design v{} §R{}.D`) — and publish both:\n\n    backlot plan publish --kind plan --file <file>\n    backlot plan publish --kind acceptance --file <file>\n",
                            d.version,
                            d.round,
                            doc.display(),
                            doc.display(),
                            d.version,
                            d.round,
                            d.version,
                            d.round,
                        ),
                    });
                }
                Some(SignMode::Skipped) => {
                    let reason_line = d
                        .skip_reason
                        .as_deref()
                        .filter(|s| !s.is_empty())
                        .map(|r| format!(" — {r}"))
                        .unwrap_or_default();
                    if has_usable_plan(state) {
                        out.push(Notice {
                            source_seq: signed_seq,
                            to: "producer".into(),
                            label: "design-signed",
                            body: format!(
                                "# design v{} signed off (skipped)\n\nThe person signed off to skip opening design for requirement v{}{reason_line}{note_line}.\n\nAn existing plan and acceptance list are already recorded in the ledger — you can `backlot dispatch --file <file>` directly (or run `backlot plan request` if a revised plan is needed).\n",
                                d.version, d.version,
                            ),
                        });
                    } else {
                        out.push(Notice {
                            source_seq: signed_seq,
                            to: "writer".into(),
                            label: "design-signed",
                            body: format!(
                                "# design v{} signed off (skipped)\n\nThe person signed off to skip opening design for requirement v{}{reason_line}{note_line}, and no recorded plan pair exists yet.\n\nWrite the formal `plan` and `acceptance` list against `{}` and publish both:\n\n    backlot plan publish --kind plan --file <file>\n    backlot plan publish --kind acceptance --file <file>\n",
                                d.version,
                                d.version,
                                dir.join(format!("req/v{}.md", d.version)).display(),
                            ),
                        });
                    }
                }
                _ => {}
            }
        }
        DesignPhase::Idle | DesignPhase::SuggestSkip => {}
    }
}
