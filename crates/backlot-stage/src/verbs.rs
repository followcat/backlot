//! Shared person-verb handlers for studio operations (B1a).
//!
//! Both CLI commands (`cli::exec_event`) and core-document requests
//! (`doc_req`) execute through these handlers and receive typed result
//! structs so validation, ledger events, and formatting agree.

use std::path::{Path, PathBuf};

use backlot_model::attention::is_prompt_reason;
use backlot_model::event::{ActorIdentity, EventKind, Priority, TaskStateChange};
use backlot_model::req_results::{
    AttentionClearResult, CloseResult, RecastResult, ReopenResult, RestartResult,
    TaskPriorityResult, TaskStateResult, TellResult,
};

use crate::args::{Cmd, TextSource};
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
    env: &Env,
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
    tell_file(base, env, caller, role, stage_path)
}

/// `tell <role> --file <path>` shared between CLI and `tell_text`.
pub(crate) fn tell_file(
    base: &Path,
    env: &Env,
    caller: &Caller,
    role: String,
    file: PathBuf,
) -> Result<TellResult, Fail> {
    let dir = store::studio_dir(base, &caller.studio);
    let built = commands::build(
        caller,
        Cmd::Tell {
            role: role.clone(),
            source: TextSource::File(file),
        },
        env,
        base,
    )?;
    let event = store::append(&dir, built)?;
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
    Ok(AttentionClearResult {
        studio: caller.studio.clone(),
        target,
        seq: event.seq,
        cleared_role: cleared_role.into_inner(),
        cleared_reason: cleared_reason.into_inner(),
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
        let current = state
            .fixed_identity(&person)
            .cloned()
            .ok_or_else(|| Fail::refused(format!("no fixed role '{person}' recorded")))?;
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
