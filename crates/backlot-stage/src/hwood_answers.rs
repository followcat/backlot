//! `--answers` validation and `message.seen` recording, plus `card`
//! apply/cancel commands extracted from `hwood.rs`.

use std::path::Path;

use backlot_model::EventBy;
use backlot_model::hollywood::{HRefusal, HollywoodKind, HollywoodState, NewHollywoodEvent};

use crate::args::Common;
use crate::cli::{Env, Fail};
use crate::hwood::{base_of, by_for, role, store_fail};
use crate::store::{self, HPending, StoreError};

pub(crate) fn hstore_fail(e: StoreError<Fail, HRefusal>) -> Fail {
    match e {
        StoreError::Build(f) => f,
        other => store_fail(other),
    }
}

/// Validate `--answers` seqs before appending `propose`/`report` so an
/// unknown or non-user seq errors cleanly with nothing appended.
pub(crate) fn check_answers(state: &HollywoodState, answers: &[u64]) -> Result<(), Fail> {
    let mut checked = Vec::new();
    for &seq in answers {
        if checked.contains(&seq) {
            continue;
        }
        let Some(m) = state.messages.iter().find(|m| m.seq == seq) else {
            return Err(Fail::usage(format!("unknown user tell seq #{seq}")));
        };
        if m.from != "user" || m.to != "hollywood" {
            return Err(Fail::usage(format!(
                "message #{seq} is not a user tell to hollywood"
            )));
        }
        checked.push(seq);
    }
    Ok(())
}

/// Append `message.seen` for each still-queued user tell listed in
/// `--answers`, silently skipping any seq already delivered/dropped/seen.
pub(crate) fn mark_answers(
    hdir: &Path,
    by: &EventBy,
    by_seq: u64,
    answers: &[u64],
) -> Result<(), Fail> {
    enum Skip {
        Done,
    }
    let mut unique = Vec::new();
    for &seq in answers {
        if !unique.contains(&seq) {
            unique.push(seq);
        }
    }
    for target in unique {
        match store::append_hollywood(hdir, |state, _| {
            let queued = state
                .messages
                .iter()
                .find(|m| m.seq == target)
                .is_some_and(|m| !m.delivered && !m.dropped && !m.seen);
            if !queued {
                return Err(Skip::Done);
            }
            Ok(HPending {
                event: NewHollywoodEvent::new(
                    by.clone(),
                    HollywoodKind::MessageSeen { target, by_seq },
                ),
                files: Vec::new(),
            })
        }) {
            Ok(_) | Err(StoreError::Build(Skip::Done)) => {}
            Err(e) => return Err(store_fail(e)),
        }
    }
    Ok(())
}

/// `card apply|cancel` is the user's (D17a): a shell carrying
/// `BACKLOT_ROLE` is a role's terminal, refused before the ledger
/// is even read.
fn card_gate(env: &Env) -> Result<(), Fail> {
    if env.get("BACKLOT_ROLE").is_some() {
        return Err(Fail::refused(
            "a card is applied or cancelled by the user — BACKLOT_ROLE is set",
        ));
    }
    Ok(())
}

pub(crate) fn card_apply(common: &Common, env: &Env, id: u64) -> Result<String, Fail> {
    card_gate(env)?;
    let base = base_of(common, env)?;
    let by = by_for(common, env);
    let res = crate::verbs_hwood::card_apply(&base, env, by, id)?;
    Ok(res.summary)
}

pub(crate) fn card_cancel(
    common: &Common,
    env: &Env,
    id: u64,
    reason: &str,
) -> Result<String, Fail> {
    card_gate(env)?;
    let base = base_of(common, env)?;
    let _ = role(common, env);
    let by = by_for(common, env);
    let res = crate::verbs_hwood::card_cancel(&base, by, id, reason)?;
    Ok(crate::verbs_hwood::format_card_cancel(&res))
}
