//! Shared person-verb handlers for studio operations (B1a).
//!
//! Both CLI commands (`cli::exec_event`) and core-document requests
//! (`doc_req`) execute through these handlers and receive typed result
//! structs so validation, ledger events, and formatting agree.

use std::path::{Path, PathBuf};

use backlot_model::attention::is_prompt_reason;
use backlot_model::event::{ActorIdentity, EventKind, Priority, TaskStateChange};
use backlot_model::req_results::{
    AskAcknowledgeResult, AttentionClearResult, CloseResult, RecastResult, ReopenResult,
    RestartResult, TaskPriorityResult, TaskStateResult, TellResult,
};

use crate::args::Cmd;
use crate::cli::{Caller, Env, Fail};
use crate::commands;
use crate::store;
pub(crate) use crate::verbs_fmt::*;

/// Ensure a studio ledger exists under `dir` before staging any files.
pub(crate) fn require_studio_dir(dir: &Path, studio: &str) -> Result<(), Fail> {
    if !dir.join("events.jsonl").is_file() {
        return Err(Fail::refused(format!("no studio '{studio}'")));
    }
    Ok(())
}

/// Remove a transient staging file even if building/appending returns early.
struct TempStageFile(PathBuf);

impl Drop for TempStageFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

/// `tell <role> "<text>"` from an in-memory string (used by `plugin.backlot.req`).
pub(crate) fn tell_text(
    base: &Path,
    _env: &Env,
    caller: &Caller,
    role: String,
    text: &str,
) -> Result<TellResult, Fail> {
    if text.trim().is_empty() {
        return Err(Fail::refused("the message body is empty"));
    }
    let dir = store::studio_dir(base, &caller.studio);
    require_studio_dir(&dir, &caller.studio)?;
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let stage_path = dir.join(format!("doc-tell-{}-{}.txt", std::process::id(), stamp));
    std::fs::write(&stage_path, text)
        .map_err(|e| Fail::refused(format!("cannot stage tell body: {e}")))?;
    let _guard = TempStageFile(stage_path.clone());
    // `tell_text` serves `plugin.backlot.req` (UI/doc bridge) and is exempt from the CLI question guard.
    tell_file_inner(base, caller, role, stage_path, false, false, false)
}

/// `tell <role> --file <path>` shared between CLI and doc verbs.
pub(crate) fn tell_file(
    base: &Path,
    _env: &Env,
    caller: &Caller,
    role: String,
    file: PathBuf,
    ask: bool,
    note: bool,
) -> Result<TellResult, Fail> {
    tell_file_inner(base, caller, role, file, ask, note, true)
}

fn tell_file_inner(
    base: &Path,
    caller: &Caller,
    role: String,
    file: PathBuf,
    ask: bool,
    note: bool,
    guard: bool,
) -> Result<TellResult, Fail> {
    let dir = store::studio_dir(base, &caller.studio);
    let msg_file = file.clone();
    let is_user_to_producer = caller.role == "user" && role == "producer";
    let caller_by = caller.by();
    // Keep identity/lock rules aligned with `commands::build` while letting `tell_text` skip the guard.
    let built = commands::tell_cmd(caller, &role, file, ask, note, guard)?;
    let events = store::append_many(&dir, move |state, seq| {
        let first = built(state, seq)?;
        let mut out = vec![first];
        if is_user_to_producer
            && state.design_phase() == backlot_model::design::DesignPhase::WaitingUser
            && let Some(d) = state.design.as_ref()
        {
            let entry_seq = seq + 1;
            let dest = format!(
                "design/v{}/r{}-{entry_seq}-user_input.md",
                d.version, d.round
            );
            out.push(store::Pending {
                event: backlot_model::NewEvent::new(
                    caller_by,
                    EventKind::DesignEntry {
                        version: d.version,
                        round: d.round,
                        slot: backlot_model::design::DesignSlot::UserInput,
                        file: dest.clone(),
                        source: Some(backlot_model::design::EntrySource::Tell),
                        refs: vec![seq],
                        questions: Vec::new(),
                        checks: Vec::new(),
                    },
                ),
                files: vec![store::PendingFile {
                    src: msg_file,
                    dest,
                    append: None,
                }],
            });
        }
        Ok(out)
    })?;
    let event = events
        .into_iter()
        .next()
        .expect("tell always appends MessageQueued");
    let dest = match event.kind {
        EventKind::MessageQueued {
            file: Some(dest), ..
        } => dest,
        _ => String::new(),
    };
    Ok(TellResult {
        studio: caller.studio.clone(),
        role,
        seq: event.seq,
        file: dest,
    })
}

/// `ask-acknowledge`: the person marked an explicit role question seen without answering it.
pub(crate) fn ask_acknowledge(
    base: &Path,
    caller: &Caller,
    role: String,
    target: u64,
) -> Result<AskAcknowledgeResult, Fail> {
    if caller.role != "user" {
        return Err(Fail::refused("acknowledging a question is the user's"));
    }
    let dir = store::studio_dir(base, &caller.studio);
    require_studio_dir(&dir, &caller.studio)?;
    let by = caller.by();
    let event = store::append(&dir, move |_, _| {
        Ok(crate::store::Pending {
            event: backlot_model::NewEvent::new(
                by,
                EventKind::AskAcknowledged {
                    role: role.clone(),
                    target,
                },
            ),
            files: Vec::new(),
        })
    })?;
    let EventKind::AskAcknowledged { role, .. } = event.kind else {
        unreachable!()
    };
    Ok(AskAcknowledgeResult {
        studio: caller.studio.clone(),
        role,
        target,
        seq: event.seq,
    })
}

/// `attention clear <seq>`: clears any open attention for CLI (`allow_prompt = true`),
/// or non-prompt attentions only for document requests (`allow_prompt = false`).
pub(crate) fn attention_clear(
    base: &Path,
    caller: &Caller,
    target: u64,
    allow_prompt: bool,
) -> Result<AttentionClearResult, Fail> {
    commands::person_gate(caller)?;
    let dir = store::studio_dir(base, &caller.studio);
    require_studio_dir(&dir, &caller.studio)?;
    let by = caller.by();
    let who = caller.role.clone();
    let cleared_role = std::cell::RefCell::new(String::new());
    let cleared_reason = std::cell::RefCell::new(String::new());
    let event = store::append(&dir, |state, _| {
        let Some(attn) = state.open_attention().find(|a| a.seq == target) else {
            return Err(Fail::refused(format!("no open attention {target}")));
        };
        if !allow_prompt && is_prompt_reason(&attn.reason) {
            return Err(Fail::refused(format!(
                "attention {target} ('{}') is an interactive prompt or dialog; open the role terminal to answer it",
                attn.reason
            )));
        }
        *cleared_role.borrow_mut() = attn.role.clone();
        *cleared_reason.borrow_mut() = attn.reason.clone();
        Ok(crate::store::Pending {
            event: backlot_model::NewEvent::new(
                by,
                EventKind::AttentionCleared {
                    target,
                    reason: Some(format!("cleared by {who}")),
                },
            ),
            files: Vec::new(),
        })
    })?;
    let role_cleared = cleared_role.into_inner();
    let reason_cleared = cleared_reason.into_inner();
    if reason_cleared == crate::ratelimit::RATE_LIMITED {
        crate::accounts_store::clear_role_in_file(
            base,
            &crate::accounts_store::role_ref(&caller.studio, &role_cleared),
        );
    }
    Ok(AttentionClearResult {
        studio: caller.studio.clone(),
        target,
        seq: event.seq,
        cleared_role: role_cleared,
        cleared_reason: reason_cleared,
    })
}

/// `recast`: records `role.recast` and returns `(RecastResult, unstarted_note)`.
#[allow(clippy::too_many_arguments)]
pub(crate) fn recast(
    base: &Path,
    env: &Env,
    caller: &Caller,
    person: String,
    runner: String,
    model: String,
    effort: Option<String>,
    permission_profile: Option<String>,
    reason: String,
) -> Result<(RecastResult, Option<&'static str>), Fail> {
    if caller.role != "user" {
        return Err(Fail::refused(format!(
            "role '{}' cannot do this; the user can",
            caller.role
        )));
    }
    let dir = store::studio_dir(base, &caller.studio);
    require_studio_dir(&dir, &caller.studio)?;
    let named = std::collections::BTreeSet::from([runner.clone()]);
    crate::draft::check_runners(base, &named, env)?;

    let by = caller.by();
    let old_holder: std::cell::RefCell<Option<ActorIdentity>> = std::cell::RefCell::new(None);
    let new_holder: std::cell::RefCell<Option<ActorIdentity>> = std::cell::RefCell::new(None);
    let note_holder: std::cell::Cell<Option<&'static str>> = std::cell::Cell::new(None);
    let reason_copy = reason.clone();

    let event = store::append(&dir, |state, _| {
        let current = backlot_model::recast::current_identity(state, &person)
            .map_err(|e| Fail::refused(e.to_string()))?
            .ok_or_else(|| {
                Fail::refused(format!(
                    "'{person}' is not a fixed role or an actor of this studio"
                ))
            })?;
        let actor = ActorIdentity {
            runner,
            runner_version: None,
            model,
            effort,
            permission_profile: permission_profile
                .unwrap_or_else(|| current.permission_profile.clone()),
        };
        backlot_model::recast::check(state, &person, &actor)
            .map_err(|e| Fail::refused(e.to_string()))?;
        *old_holder.borrow_mut() = Some(current);
        *new_holder.borrow_mut() = Some(actor.clone());
        note_holder.set(backlot_model::recast::unstarted_note(state, &person));
        Ok(store::Pending {
            event: backlot_model::NewEvent::new(
                by,
                EventKind::RoleRecast {
                    role: person.clone(),
                    actor,
                    reason: (!reason_copy.is_empty()).then_some(reason_copy),
                    card: None,
                },
            ),
            files: Vec::new(),
        })
    })?;

    let old = old_holder
        .into_inner()
        .expect("append closure sets old identity");
    let new = new_holder
        .into_inner()
        .expect("append closure sets new identity");
    Ok((
        RecastResult {
            studio: caller.studio.clone(),
            person,
            seq: event.seq,
            old,
            new,
            reason,
        },
        note_holder.into_inner(),
    ))
}

/// `restart --person <role>`: restarts a studio role (resume or fresh).
pub(crate) fn restart_role(
    base: &Path,
    env: &Env,
    caller: &Caller,
    person: String,
    fresh: bool,
    reason: String,
) -> Result<RestartResult, Fail> {
    let dir = store::studio_dir(base, &caller.studio);
    let built = commands::build(
        caller,
        Cmd::Restart {
            person: person.clone(),
            fresh,
            reason: reason.clone(),
        },
        env,
        base,
    )?;
    let event = store::append(&dir, built)?;
    Ok(RestartResult {
        studio: Some(caller.studio.clone()),
        person,
        seq: event.seq,
        fresh,
        reason,
        is_hollywood: false,
    })
}

/// `close <person>`: releases a held fixed role.
pub(crate) fn close(
    base: &Path,
    env: &Env,
    caller: &Caller,
    person: String,
) -> Result<CloseResult, Fail> {
    let dir = store::studio_dir(base, &caller.studio);
    let built = commands::build(
        caller,
        Cmd::Close {
            person: person.clone(),
        },
        env,
        base,
    )?;
    let event = store::append(&dir, built)?;
    Ok(CloseResult {
        studio: caller.studio.clone(),
        person,
        seq: event.seq,
    })
}

/// `reopen <person>`: asks a closed role back.
pub(crate) fn reopen(
    base: &Path,
    env: &Env,
    caller: &Caller,
    person: String,
) -> Result<ReopenResult, Fail> {
    let dir = store::studio_dir(base, &caller.studio);
    let built = commands::build(
        caller,
        Cmd::Reopen {
            person: person.clone(),
        },
        env,
        base,
    )?;
    let event = store::append(&dir, built)?;
    Ok(ReopenResult {
        studio: caller.studio.clone(),
        person,
        seq: event.seq,
    })
}

/// `studio cast set --file <toml> [--dry-run] [--json]`: validate the
/// file as the studio's cast, print the diff, and — unless `--dry-run`
/// — record the recasts and the `studio.cast`. The user's alone.
pub(crate) fn cast_set(
    base: &Path,
    env: &Env,
    caller: &Caller,
    file: &Path,
    dry_run: bool,
    json: bool,
) -> Result<crate::studio_cast::CastSetOutput, Fail> {
    if caller.role != "user" {
        return Err(Fail::refused(format!(
            "role '{}' cannot do this; the user can",
            caller.role
        )));
    }
    let dir = store::studio_dir(base, &caller.studio);
    require_studio_dir(&dir, &caller.studio)?;
    let state = store::load_state(&dir).map_err(crate::hwood::store_fail)?;
    let existing_checks = state
        .studio
        .as_ref()
        .map(|s| s.checks.as_slice())
        .unwrap_or(&[]);
    let next = crate::studio_cast::read_preserving(file, existing_checks)?;
    crate::studio_cast::check(base, env, &state, &next)?;
    let old = backlot_model::cast::CastSettings::of(&state)
        .ok_or_else(|| Fail::refused(format!("no studio '{}'", caller.studio)))?;
    let diff = backlot_model::cast::diff(&old, &next);
    if diff.is_empty() {
        return Err(Fail::refused(
            backlot_model::Refusal::CastUnchanged.to_string(),
        ));
    }
    if dry_run {
        return Ok(crate::studio_cast::CastSetOutput {
            studio: caller.studio.clone(),
            dry_run,
            json,
            seq: None,
            diff,
            recasts: Vec::new(),
        });
    }
    let set = crate::studio_cast::record(&dir, base, env, caller, &next, None)?;
    Ok(crate::studio_cast::CastSetOutput {
        studio: caller.studio.clone(),
        dry_run,
        json,
        seq: Some(set.seq),
        diff: set.diff,
        recasts: set.recasts,
    })
}

/// `studio cast set` as the server queue's typed result: the same
/// `cast_set`, flattened into [`backlot_model::req_results::StudioCastSetResult`].
pub(crate) fn cast_set_result(
    base: &Path,
    env: &Env,
    caller: &Caller,
    file: &Path,
    dry_run: bool,
    json: bool,
) -> Result<backlot_model::req_results::StudioCastSetResult, Fail> {
    let out = cast_set(base, env, caller, file, dry_run, json)?;
    Ok(backlot_model::req_results::StudioCastSetResult {
        studio: out.studio,
        dry_run: out.dry_run,
        seq: out.seq,
        diff: out.diff,
        recasts: out.recasts,
    })
}

/// `studio cast show`: the effective cast of `studio`, as a cast file
/// (`json == false`) or the settings structured. Read-only — the CLI and
/// the server queue both go through here.
pub(crate) fn cast_show(
    base: &Path,
    studio: &str,
    json: bool,
) -> Result<backlot_model::req_results::StudioCastShowResult, Fail> {
    let dir = store::studio_dir(base, studio);
    let state = store::load_state(&dir).map_err(crate::hwood::store_fail)?;
    let cast = backlot_model::cast::CastSettings::of(&state)
        .ok_or_else(|| Fail::refused(format!("no studio '{studio}'")))?;
    Ok(backlot_model::req_results::StudioCastShowResult {
        studio: studio.to_string(),
        cast: backlot_model::cast::to_toml(&cast),
        settings: json.then_some(cast),
    })
}

/// `priority --task <t> --level low|normal|high`.
pub(crate) fn task_priority(
    base: &Path,
    env: &Env,
    caller: &Caller,
    level: Priority,
) -> Result<TaskPriorityResult, Fail> {
    let task = caller
        .task
        .clone()
        .ok_or_else(|| Fail::usage("missing task"))?;
    let dir = store::studio_dir(base, &caller.studio);
    let built = commands::build(caller, Cmd::Priority { level }, env, base)?;
    let event = store::append(&dir, built)?;
    Ok(TaskPriorityResult {
        studio: caller.studio.clone(),
        task,
        seq: event.seq,
        level,
    })
}

/// `task pause|resume|abandon`.
pub(crate) fn task_state(
    base: &Path,
    env: &Env,
    caller: &Caller,
    change: TaskStateChange,
    reason: String,
) -> Result<TaskStateResult, Fail> {
    match change {
        TaskStateChange::Paused | TaskStateChange::Resumed | TaskStateChange::Abandoned => {}
        other => {
            return Err(Fail::refused(format!(
                "task state change '{other:?}' is not a person task verb"
            )));
        }
    }
    let task = caller
        .task
        .clone()
        .ok_or_else(|| Fail::usage("missing task"))?;
    let dir = store::studio_dir(base, &caller.studio);
    let built = commands::build(
        caller,
        Cmd::TaskState {
            change,
            reason: reason.clone(),
        },
        env,
        base,
    )?;
    let event = store::append(&dir, built)?;
    Ok(TaskStateResult {
        studio: caller.studio.clone(),
        task,
        seq: event.seq,
        state: change,
        reason,
    })
}
