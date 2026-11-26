//! The overseer commands: `hollywood propose`, `hollywood report`,
//! `tell hollywood`, `card list|apply|cancel`, and the cross-studio
//! reads `status --all` and `records --all`/`--studio`. Durable
//! output lands in Hollywood's own ledger at
//! `<base>/hollywood/events.jsonl` or in the studios' ledgers —
//! nothing here reaches a terminal (that is the stage's job, H3).

use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use backlot_model::EventBy;
use backlot_model::event::ApprovalDecision;
use backlot_model::hollywood::{CardAction, HollywoodKind, NewHollywoodEvent};
use backlot_model::state::StudioState;

use crate::args::{CardCmd, Cmd, Common, HollywoodCmd};
use crate::cli::{Caller, Env, Fail};
use crate::commands;
use crate::hrender;
use crate::hwood_answers::{card_apply, card_cancel, check_answers, hstore_fail, mark_answers};
use crate::link;
use crate::store::{self, HPending, PendingFile, StoreError, sanitize};

/// The caller role for an overseer command: `--as`, then
/// `BACKLOT_ROLE`, else the plain user.
pub(crate) fn role(common: &Common, env: &Env) -> String {
    common
        .role
        .clone()
        .or_else(|| env.get("BACKLOT_ROLE").map(String::from))
        .unwrap_or_else(|| "user".to_string())
}

pub(crate) fn by_for(common: &Common, env: &Env) -> EventBy {
    EventBy {
        role: role(common, env),
        session: env
            .get("HC_TERMINAL_ID")
            .or_else(|| env.get("BACKLOT_TERMINAL"))
            .map(String::from),
        via: env
            .get("BACKLOT_HANDOFF_ID")
            .filter(|id| !id.is_empty())
            .map(|id| format!("handoff:{id}")),
    }
}

/// The state base — same resolution as the studio commands'.
pub(crate) fn base_of(common: &Common, env: &Env) -> Result<PathBuf, Fail> {
    link::state_dir_sync(
        env,
        common.session.as_deref().map(OsStr::new),
        common.remote.as_deref(),
    )
}

pub(crate) fn store_fail<E, V: ToString>(e: StoreError<E, V>) -> Fail {
    match e {
        StoreError::Io(e) => Fail::refused(e.to_string()),
        StoreError::Corrupt(c) => Fail::refused(c.to_string()),
        StoreError::Refused(v) => Fail::refused(v.to_string()),
        StoreError::Build(_) => Fail::refused("unexpected error"),
    }
}

/// The studio's reduced state, `None` when it has no ledger — a
/// `studio-new` card's base, or a stale reason at apply time. The
/// name — which a card action carries in from the ledger — is
/// validated before any path is built, so a hostile identifier can
/// never walk out of `studios/` (D17a).
pub(crate) fn studio_state(base: &Path, name: &str) -> Result<Option<StudioState>, Fail> {
    crate::cli::check_studio_name(name)
        .map_err(|_| Fail::refused(format!("bad studio name '{name}'")))?;
    let dir = store::studio_dir(base, name);
    if !dir.join("events.jsonl").is_file() {
        return Ok(None);
    }
    store::load_state(&dir).map(Some).map_err(store_fail)
}

/// The studio must exist — `propose` on anything but `studio-new`
/// talks about a real one.
pub(crate) fn require_studio(base: &Path, name: &str) -> Result<StudioState, Fail> {
    studio_state(base, name)?.ok_or_else(|| Fail::refused(format!("no studio '{name}'")))
}

/// `hollywood propose` — Hollywood only, checked before the
/// filesystem is touched.
fn propose(
    common: &Common,
    env: &Env,
    action: &CardAction,
    why: &str,
    answers: &[u64],
) -> Result<String, Fail> {
    let r = role(common, env);
    if r != "hollywood" {
        return Err(Fail::refused(format!(
            "role '{r}' cannot do this; the hollywood role can"
        )));
    }
    let base = base_of(common, env)?;
    // A recast's identity is completed and checked before the card
    // exists; every other action passes through unchanged.
    let action = crate::hwood_base::resolve_action(&base, action, env)?;
    let card_base = crate::hwood_base::card_base(&base, &action, env)?;
    let hdir = store::hollywood_dir(&base);
    let by = by_for(common, env);
    let why = why.to_string();
    let event = store::append_hollywood(&hdir, |state, _| {
        check_answers(state, answers)?;
        Ok::<HPending, Fail>(HPending {
            event: NewHollywoodEvent::new(
                by.clone(),
                HollywoodKind::CardProposed {
                    action,
                    why,
                    base: card_base,
                },
            ),
            files: Vec::new(),
        })
    })
    .map_err(hstore_fail)?;
    mark_answers(&hdir, &by, event.seq, answers)?;
    Ok(format!(
        "{} — the user applies or cancels it",
        hrender::hconfirm(&event)
    ))
}

/// `hollywood report "<headline>" [--body <file>] [--answers <seqs>]` —
/// the layer's headline lands in the ledger; the TUI reads it there (H4).
fn hreport(
    common: &Common,
    env: &Env,
    headline: &str,
    body: Option<PathBuf>,
    answers: &[u64],
) -> Result<String, Fail> {
    let r = role(common, env);
    if r != "hollywood" {
        return Err(Fail::refused(format!(
            "role '{r}' cannot do this; the hollywood role can"
        )));
    }
    if let Some(f) = &body {
        commands::must_exist(f, "--body")?;
    }
    let base = base_of(common, env)?;
    let hdir = store::hollywood_dir(&base);
    let by = by_for(common, env);
    let headline = headline.to_string();
    let event = store::append_hollywood(&hdir, |state, seq| {
        check_answers(state, answers)?;
        let (body_path, files) = match &body {
            Some(src) => {
                let pf = copy_to("reports", seq, src, None);
                (Some(pf.dest.clone()), vec![pf])
            }
            None => (None, Vec::new()),
        };
        Ok::<HPending, Fail>(HPending {
            event: NewHollywoodEvent::new(
                by.clone(),
                HollywoodKind::Reported {
                    headline,
                    body: body_path,
                },
            ),
            files,
        })
    })
    .map_err(hstore_fail)?;
    mark_answers(&hdir, &by, event.seq, answers)?;
    Ok(format!(
        "{} — the layer shows it when the TUI lands (H4)",
        hrender::hconfirm(&event)
    ))
}

/// A `seq`-prefixed copy name under a Hollywood content dir — the
/// same shape the studio ledger's `messages/` and `reports/` use.
fn copy_to(subdir: &str, seq: u64, src: &Path, label: Option<&str>) -> PendingFile {
    let base = src
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "file".into());
    let leaf = match label {
        Some(l) => format!("{seq}-{l}-{}", sanitize(&base)),
        None => format!("{seq}-{}", sanitize(&base)),
    };
    PendingFile {
        src: src.to_path_buf(),
        dest: format!("{subdir}/{leaf}"),
        append: None,
    }
}

/// `tell hollywood "<text>"` — recorded; delivery is H3.
fn tell_hollywood(common: &Common, env: &Env, text: &str) -> Result<String, Fail> {
    let base = base_of(common, env)?;
    let by = by_for(common, env);
    let res = crate::verbs_hwood::tell_hollywood(&base, by, text, &[])?;
    Ok(crate::verbs_hwood::format_tell_hollywood(&res))
}

/// `restart --person hollywood [--fresh] [--reason <r>]`: the user
/// stops `bl-hollywood` and the stage launches it again — resuming the
/// recorded session unless `--fresh`. Recorded in Hollywood's own
/// ledger (`role.restart`); the stage's `hplan` carries it out. The
/// `[hollywood]` section must exist — there is nothing to restart
/// without one.
pub(crate) fn restart(
    common: &Common,
    env: &Env,
    fresh: bool,
    reason: &str,
) -> Result<String, Fail> {
    let base = base_of(common, env)?;
    let by = by_for(common, env);
    let res = crate::verbs_hwood::restart_hollywood(&base, env, by, fresh, reason)?;
    Ok(crate::verbs::format_restart(&res))
}

/// `approve --person hollywood --decision allow|deny`: the user's
/// answer to a permission prompt on Hollywood's own screen, recorded
/// in its ledger. The stage sends the runner's one-time key (H3); no
/// card ever answers a prompt (D18).
pub(crate) fn approve(
    common: &Common,
    env: &Env,
    decision: ApprovalDecision,
) -> Result<String, Fail> {
    let r = role(common, env);
    if r != "user" {
        return Err(Fail::refused(format!(
            "role '{r}' cannot do this; the user can"
        )));
    }
    let base = base_of(common, env)?;
    let hdir = store::hollywood_dir(&base);
    let by = by_for(common, env);
    let event = store::append_hollywood(&hdir, |_, _| {
        Ok::<HPending, Fail>(HPending {
            event: NewHollywoodEvent::new(by, HollywoodKind::RoleApproval { decision }),
            files: Vec::new(),
        })
    })
    .map_err(store_fail)?;
    Ok(format!(
        "{} — the stage answers the prompt",
        hrender::hconfirm(&event)
    ))
}

/// The message file a Hollywood caller just sent a studio — its own
/// ledger keeps a copy (R26). `tell producer` only — Hollywood's one
/// write into a studio; captured before `commands::build` consumes
/// the cmd.
pub(crate) fn mirror_for(caller: &Caller, cmd: &Cmd) -> Option<PathBuf> {
    match cmd {
        Cmd::Tell {
            role,
            source: crate::args::TextSource::File(file),
            ..
        } if caller.role == "hollywood" && role == "producer" => Some(file.clone()),
        _ => None,
    }
}

/// Hollywood's own copy of a `tell producer` it recorded in a
/// studio's ledger (R26): `message.sent` with the studio's name and
/// the same file under its `messages/` — the studio's
/// `message.queued` remains the delivery record.
pub fn mirror_tell(base: &Path, by: &EventBy, studio: &str, file: &Path) -> Result<String, Fail> {
    let hdir = store::hollywood_dir(base);
    let studio = studio.to_string();
    let by = by.clone();
    let event = store::append_hollywood(&hdir, |_, seq| {
        let pf = copy_to("messages", seq, file, None);
        Ok::<HPending, Fail>(HPending {
            event: NewHollywoodEvent::new(
                by,
                HollywoodKind::MessageSent {
                    to: "producer".into(),
                    studio: Some(studio),
                    text: None,
                    file: Some(pf.dest.clone()),
                    source_seq: None,
                },
            ),
            files: vec![pf],
        })
    })
    .map_err(store_fail)?;
    Ok(hrender::hconfirm(&event))
}

/// `hollywood`/`card`/`tell hollywood` — the overseer-ledger commands.
pub fn run(common: Common, cmd: Cmd, env: &Env) -> Result<String, Fail> {
    match cmd {
        Cmd::TellHollywood { ref text } => tell_hollywood(&common, env, text),
        Cmd::Hollywood(HollywoodCmd::Propose {
            ref action,
            ref why,
            ref answers,
        }) => propose(&common, env, action, why, answers),
        Cmd::Hollywood(HollywoodCmd::Report {
            ref headline,
            ref body,
            ref answers,
        }) => hreport(&common, env, headline, body.clone(), answers),
        Cmd::Hollywood(HollywoodCmd::Show { seq }) => crate::hwood_show::show(&common, env, seq),
        Cmd::Hollywood(HollywoodCmd::Setup {
            ref cfg,
            force_offline,
        }) => crate::hsetup::hsetup(&common, env, cfg, force_offline),
        Cmd::Hollywood(HollywoodCmd::Switch {
            ref runner,
            ref model,
            ref effort,
            timeout_s,
            force,
            no_wait,
            json,
        }) => crate::hswitch_verbs::switch_local(
            &common,
            env,
            runner,
            model,
            effort.as_deref(),
            timeout_s,
            force,
            no_wait,
            json,
        ),
        Cmd::Hollywood(HollywoodCmd::SwitchCancel { json }) => {
            crate::hswitch_verbs::cancel_local(&common, env, json)
        }
        Cmd::Hollywood(HollywoodCmd::SwitchStatus { json }) => {
            crate::hswitch_verbs::status_local(&common, env, json)
        }
        Cmd::Card(CardCmd::List) => crate::hwood_read::card_list(&common, env),
        Cmd::Card(CardCmd::Show { id }) => crate::hwood_read::card_show(&common, env, id),
        Cmd::Card(CardCmd::Apply { id }) => card_apply(&common, env, id),
        Cmd::Card(CardCmd::Cancel { id, ref reason }) => card_cancel(&common, env, id, reason),
        _ => unreachable!("hwood::run handles only overseer commands"),
    }
}
