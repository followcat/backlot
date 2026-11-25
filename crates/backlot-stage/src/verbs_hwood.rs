//! Shared overseer & Hollywood person-verb handlers (B1a).
//!
//! Used identically by the CLI (`hwood`, `hwood_answers`, `hwood_read`,
//! `hsetup`) and `doc_req`, returning typed result structs defined in
//! `backlot_model::req_results`.

use std::path::{Path, PathBuf};

use backlot_model::EventBy;
use backlot_model::hollywood::{self, CardAction, HollywoodKind, NewHollywoodEvent};
use backlot_model::req_results::{
    CardApplyResult, CardCancelResult, HollywoodSetupResult, RestartResult, TellHollywoodResult,
};

use crate::cli::{Env, Fail};
use crate::config::{self, HollywoodCfg};
use crate::hwood::{store_fail, studio_state};
use crate::hwood_answers::{check_answers, hstore_fail, mark_answers};
use crate::store::{self, HPending};
pub(crate) use crate::verbs_card_show::{card_show, format_card_show};

fn require_user_role(role: &str) -> Result<(), Fail> {
    if role != "user" {
        return Err(Fail::refused(format!(
            "role '{role}' cannot do this; the user can"
        )));
    }
    Ok(())
}

/// `tell hollywood "<text>"` with optional `--answers <seqs>`.
pub(crate) fn tell_hollywood(
    base: &Path,
    by: EventBy,
    text: &str,
    answers: &[u64],
) -> Result<TellHollywoodResult, Fail> {
    if text.trim().is_empty() {
        return Err(Fail::usage("an empty message says nothing"));
    }
    if text.contains('\n') {
        return Err(Fail::usage("tell hollywood sends one line"));
    }
    let hdir = store::hollywood_dir(base);
    let body = text.to_string();
    let answers_vec = answers.to_vec();
    let marked_seen = if !answers_vec.is_empty() {
        let state = store::load_hstate(&hdir).map_err(store_fail)?;
        check_answers(&state, &answers_vec)?;
        answers_vec
            .iter()
            .filter(|&&seq| {
                state
                    .messages
                    .iter()
                    .find(|m| m.seq == seq)
                    .is_some_and(|m| !m.delivered && !m.dropped && !m.seen)
            })
            .copied()
            .collect()
    } else {
        Vec::new()
    };
    let stage_by = EventBy {
        role: "stage".into(),
        session: None,
        via: by.via.clone(),
    };
    let event = store::append_hollywood(&hdir, |state, _| {
        if !answers_vec.is_empty() {
            check_answers(state, &answers_vec)?;
        }
        Ok::<HPending, Fail>(HPending {
            event: NewHollywoodEvent::new(
                by,
                HollywoodKind::MessageSent {
                    to: "hollywood".into(),
                    studio: None,
                    text: Some(body),
                    file: None,
                    source_seq: None,
                },
            ),
            files: Vec::new(),
        })
    })
    .map_err(hstore_fail)?;
    if !answers_vec.is_empty() {
        mark_answers(&hdir, &stage_by, event.seq, &answers_vec)?;
    }
    Ok(TellHollywoodResult {
        seq: event.seq,
        to: "hollywood".into(),
        marked_seen,
    })
}

pub(crate) fn format_tell_hollywood(res: &TellHollywoodResult) -> String {
    format!(
        "event {}: message.sent → {} — delivery to its terminal is the stage's job (H3)",
        res.seq, res.to
    )
}

/// `card apply <id>` — checks staleness under current studio / `[hollywood]`
/// state and records `card.applied` or `card.stale`.
pub(crate) fn card_apply(
    base: &Path,
    env: &Env,
    by: EventBy,
    id: u64,
) -> Result<CardApplyResult, Fail> {
    require_user_role(&by.role)?;
    let hdir = store::hollywood_dir(base);
    let hstate = store::load_hstate(&hdir).map_err(store_fail)?;
    let card = hstate
        .cards
        .get(&id)
        .ok_or_else(|| Fail::refused(format!("no card 'c{id}'")))?;
    let stale = match &card.action {
        CardAction::Setup { .. } => {
            let current = config::hollywood(&env.vars)
                .map_err(Fail::refused)?
                .map(|c| crate::hstage::identity(&c));
            hollywood::stale_hollywood(&card.base, current.as_ref())
        }
        _ => {
            let sstate = studio_state(base, card.action.studio())?;
            hollywood::stale(&card.action, &card.base, sstate.as_ref()).or_else(|| {
                crate::hcard_outward::outward_stale(&card.action, &card.base, sstate.as_ref())
            })
        }
    };
    let kind = match &stale {
        Some(r) => HollywoodKind::CardStale {
            card: id,
            reason: r.clone(),
        },
        None => HollywoodKind::CardApplied { card: id },
    };
    let event = store::append_hollywood(&hdir, |_, _| {
        Ok::<HPending, Fail>(HPending {
            event: NewHollywoodEvent::new(by, kind),
            files: Vec::new(),
        })
    })
    .map_err(store_fail)?;
    let summary = match &stale {
        Some(r) => format!("event {}: card.stale c{id} — {r}", event.seq),
        None => format!(
            "event {}: card.applied c{id} — carrying the action out is the stage's job (H3)",
            event.seq
        ),
    };
    Ok(CardApplyResult {
        id: format!("c{id}"),
        seq: id,
        outcome_seq: event.seq,
        stale,
        summary,
    })
}

/// `card cancel <id> [--reason <r>]`.
pub(crate) fn card_cancel(
    base: &Path,
    by: EventBy,
    id: u64,
    reason: &str,
) -> Result<CardCancelResult, Fail> {
    require_user_role(&by.role)?;
    let hdir = store::hollywood_dir(base);
    let reason_owned = reason.to_string();
    let event = store::append_hollywood(&hdir, |_, _| {
        Ok::<HPending, Fail>(HPending {
            event: NewHollywoodEvent::new(
                by,
                HollywoodKind::CardCancelled {
                    card: id,
                    reason: reason_owned,
                },
            ),
            files: Vec::new(),
        })
    })
    .map_err(store_fail)?;
    Ok(CardCancelResult {
        id: format!("c{id}"),
        seq: id,
        outcome_seq: event.seq,
        reason: reason.to_string(),
    })
}

pub(crate) fn format_card_cancel(res: &CardCancelResult) -> String {
    format!("event {}: card.cancelled {}", res.outcome_seq, res.id)
}

/// `restart --person hollywood [--fresh] [--reason <r>]`.
pub(crate) fn restart_hollywood(
    base: &Path,
    env: &Env,
    by: EventBy,
    fresh: bool,
    reason: &str,
) -> Result<RestartResult, Fail> {
    require_user_role(&by.role)?;
    if config::hollywood(&env.vars).ok().flatten().is_none() {
        return Err(Fail::refused(
            "hollywood is not set up — run 'backlot hollywood setup' first",
        ));
    }
    let hdir = store::hollywood_dir(base);
    let hstate = store::load_hstate(&hdir).map_err(store_fail)?;
    if !hstate.role.recorded() {
        return Err(Fail::refused(
            "hollywood has not started yet — the stage starts it on its next tick",
        ));
    }
    let reason_opt = (!reason.is_empty()).then(|| reason.to_string());
    let event = store::append_hollywood(&hdir, |_, _| {
        Ok::<HPending, Fail>(HPending {
            event: NewHollywoodEvent::new(
                by,
                HollywoodKind::RoleRestart {
                    fresh,
                    reason: reason_opt,
                },
            ),
            files: Vec::new(),
        })
    })
    .map_err(store_fail)?;
    Ok(RestartResult {
        studio: None,
        person: "hollywood".into(),
        seq: event.seq,
        fresh,
        reason: reason.to_string(),
        is_hollywood: true,
    })
}

/// `hollywood setup`: validates, writes `[hollywood]` to `config.toml`, and returns
/// `(HollywoodSetupResult, written_path)`.
pub(crate) fn hollywood_setup(
    env: &Env,
    cfg: &HollywoodCfg,
) -> Result<(HollywoodSetupResult, PathBuf), Fail> {
    if env.get("BACKLOT_ROLE").is_some() {
        return Err(Fail::refused(
            "hollywood setup is the user's — BACKLOT_ROLE is set",
        ));
    }
    let was_configured = config::hollywood(&env.vars).ok().flatten().is_some();
    let path = crate::hsetup::write_config(env, cfg)?;
    let written = config::hollywood(&env.vars).ok().flatten();
    let res = HollywoodSetupResult {
        runner: cfg.runner.clone(),
        model: cfg.model.clone(),
        effort: cfg.effort.clone(),
        key: written.as_ref().and_then(|w| w.key.clone()),
        default_cast: written.and_then(|w| w.default_cast),
        action: if was_configured {
            "updated".into()
        } else {
            "configured".into()
        },
        restarted: false,
    };
    Ok((res, path))
}
