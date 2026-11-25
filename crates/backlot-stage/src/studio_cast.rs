//! `backlot studio cast set` — changing a studio's cast after it
//! started ("Changing a studio after it starts"). The cast file is
//! read with `studio new`'s parser (the effort, cline-model and limit
//! rules, the check declarations) but only the *runnable* checks hold
//! here: the studio exists and is not finished, a limit is positive,
//! and the caps still seat the fixed roles. Whether a runner has an
//! adapter and is installed is asked separately, because a new cast
//! may name a runner the machine has not got.
//!
//! A fixed role whose identity moved is carried out as a `role.recast`
//! for that role, in the same apply: that is what makes the stage swap
//! a live terminal. The `studio.cast` event records the whole new
//! settings.

use std::path::Path;

use backlot_model::cast::{self, CastDiff, CastSettings};
use backlot_model::event::EventKind;
use backlot_model::state::StudioState;
use serde::Serialize;

use crate::cli::{Caller, Env, Fail};
use crate::commands;
use crate::store;

/// What `studio cast set` reports: the diff it made (or would make),
/// and whether it landed.
#[derive(Debug, Clone, Serialize)]
pub struct CastSetOutput {
    pub studio: String,
    pub dry_run: bool,
    #[serde(skip)]
    pub json: bool,
    /// The `studio.cast` seq, `None` on a dry run.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seq: Option<u64>,
    pub diff: CastDiff,
    /// The `role.recast` events the set appended (role, seq), in order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub recasts: Vec<(String, u64)>,
}

/// What [`record`] returns on the write path.
pub struct CastSet {
    pub seq: u64,
    pub diff: CastDiff,
    pub recasts: Vec<(String, u64)>,
}

/// Read a cast file as this studio's new cast — `studio new`'s parser
/// (the effort, cline-model and limit rules, the check declarations)
/// and then the defaults.
pub(crate) fn read(path: &Path) -> Result<CastSettings, Fail> {
    let file = commands::read_cast(path)?;
    Ok(CastSettings {
        roles: backlot_model::event::RoleConfigs {
            producer: file.producer.clone(),
            director: file.director.clone(),
            writer: file.writer.clone(),
        },
        candidates: file.candidates.clone(),
        rework_cap: file
            .rework_cap
            .unwrap_or_else(backlot_model::event::default_rework_cap),
        stall_limit_s: file
            .stall_limit_s
            .unwrap_or_else(backlot_model::event::default_stall_limit_s),
        nudge_after_s: file
            .nudge_after_s
            .unwrap_or_else(backlot_model::event::default_nudge_after_s),
        escalate_after_s: file
            .escalate_after_s
            .unwrap_or_else(backlot_model::event::default_escalate_after_s),
        check_env: file.check_env.clone(),
        checks: crate::checks::declarations(&file.check_env, &file.checks)
            .map(|(_, checks)| checks)?,
        task_budget_s: file.task_budget_s,
        limits: file.limits.clone(),
        push: backlot_model::event::PushPolicy {
            protected_branches: file
                .push
                .protected_branches
                .clone()
                .unwrap_or_else(backlot_model::push_policy::default_protected_branches),
            protect_tags: file.push.protect_tags.unwrap_or(true),
            protect_default_branch: file.push.protect_default_branch.unwrap_or(true),
            remotes: Vec::new(),
        },
    })
}

/// The runnable checks `studio cast set` runs before anything is
/// recorded: the model's cast rules over the live state, plus the
/// runner check `studio new --dry-run` does — every named runner has an
/// adapter and launches on this machine. This is where "nothing
/// changes" is said.
pub(crate) fn check(
    base: &Path,
    env: &Env,
    state: &StudioState,
    next: &CastSettings,
) -> Result<(), Fail> {
    cast::check(state, next).map_err(|e| Fail::refused(e.to_string()))?;
    check_runners(base, env, next)
}

/// The same runnable rules without the "nothing changes" comparison —
/// what each append re-checks under the lock. The settings event lands
/// after the recasts it carries, so the effective cast already holds
/// the moved identities by then.
fn check_runnable(
    base: &Path,
    env: &Env,
    state: &StudioState,
    next: &CastSettings,
) -> Result<(), Fail> {
    cast::check_runnable(state, next).map_err(|e| Fail::refused(e.to_string()))?;
    check_runners(base, env, next)
}

fn check_runners(base: &Path, env: &Env, next: &CastSettings) -> Result<(), Fail> {
    let named: std::collections::BTreeSet<String> = next.identities().map(|a| a.runner).collect();
    crate::draft::check_runners(base, &named, env)
}

/// Append `next` as the studio's cast, `by: user`. The fixed roles the
/// file moves are recorded as `role.recast` events first — that is what
/// makes the stage swap a live terminal — and then one `studio.cast`
/// carries the whole new settings. Both are re-checked under the append
/// lock, so a change between the read and the write is refused rather
/// than recorded. Returns the settings event's seq, its diff and the
/// recasts.
pub(crate) fn record(
    dir: &Path,
    base: &Path,
    env: &Env,
    caller: &Caller,
    next: &CastSettings,
    card: Option<String>,
) -> Result<CastSet, Fail> {
    // The diff against the state as it is now; the append re-checks the
    // whole cast under the lock, so a moved state refuses the recasts
    // that follow rather than landing half a change.
    let state = store::load_state(dir).map_err(crate::hwood::store_fail)?;
    check(base, env, &state, next)?;
    let old = CastSettings::of(&state)
        .ok_or_else(|| Fail::refused("no studio recorded yet; run 'backlot studio new' first"))?;
    let diff = cast::diff(&old, next);

    let mut recasts = Vec::new();
    for (role, actor) in diff.moved_roles() {
        let role = role.to_string();
        let actor = actor.clone();
        let by = caller.by();
        let recast_role = role.clone();
        let event = store::append(dir, move |state, _| {
            backlot_model::recast::check(state, &recast_role, &actor)
                .map_err(|e| Fail::refused(e.to_string()))?;
            Ok(store::Pending {
                event: backlot_model::NewEvent::new(
                    by.clone(),
                    EventKind::RoleRecast {
                        role: recast_role.clone(),
                        actor,
                        reason: Some("studio cast".into()),
                        card: None,
                    },
                ),
                files: Vec::new(),
            })
        })
        .map_err(crate::cli::Fail::from)?;
        recasts.push((role, event.seq));
    }

    let by = caller.by();
    let next = next.clone();
    let checked = base.to_path_buf();
    let env_vars = env.vars.clone();
    let event = store::append(dir, |state, _| {
        let env = Env {
            vars: env_vars.clone(),
        };
        check_runnable(&checked, &env, state, &next)?;
        Ok(store::Pending {
            event: backlot_model::NewEvent::new(by.clone(), next.event_kind(card.clone())),
            files: Vec::new(),
        })
    })
    .map_err(crate::cli::Fail::from)?;
    Ok(CastSet {
        seq: event.seq,
        diff,
        recasts,
    })
}
