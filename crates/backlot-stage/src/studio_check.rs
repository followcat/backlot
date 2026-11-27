//! User-only `backlot studio check add|remove` handlers: validate the check
//! declaration (and optionally preflight it), then record an updated
//! `studio.cast` event with `CheckOrigin::User` preserved independently of
//! `cast.toml` and design-proposed checks.

use std::path::Path;

use backlot_model::cast::{self, CastSettings};
use backlot_model::event::CheckOrigin;
use backlot_model::NewEvent;

use crate::args_studio_check::CheckCmd;
use crate::checks::{self, CastCheck};
use crate::checks_preflight;
use crate::cli::{Caller, Env, Fail};
use crate::store::{self, Pending};
use crate::verbs::require_studio_dir;

pub(crate) fn run(
    base: &Path,
    env: &Env,
    caller: &Caller,
    cmd: CheckCmd,
) -> Result<String, Fail> {
    if caller.role != "user" {
        return Err(Fail::refused(format!(
            "role '{}' cannot do this; only the user can",
            caller.role
        )));
    }
    let dir = store::studio_dir(base, &caller.studio);
    require_studio_dir(&dir, &caller.studio)?;
    match cmd {
        CheckCmd::Add {
            name,
            run,
            timeout_s,
            expect,
            min_matches,
            preflight,
        } => add(
            &dir,
            env,
            caller,
            CastCheck {
                name,
                run,
                timeout_s: timeout_s.unwrap_or(600),
                expect,
                min_matches,
            },
            preflight,
        ),
        CheckCmd::Remove { name } => remove(&dir, caller, &name),
    }
}

fn add(
    dir: &Path,
    env: &Env,
    caller: &Caller,
    raw: CastCheck,
    preflight: bool,
) -> Result<String, Fail> {
    let decl = checks::validate_one_check(&raw, CheckOrigin::User)?;
    let state = store::load_state(dir).map_err(crate::hwood::store_fail)?;
    let studio = state
        .studio
        .as_ref()
        .ok_or_else(|| Fail::refused("no studio recorded yet"))?;
    if studio.checks.iter().any(|c| c.name == decl.name) {
        return Err(Fail::refused(format!(
            "check '{}' is already declared",
            decl.name
        )));
    }
    if preflight {
        checks_preflight::run_user_preflight(dir, studio, env, std::slice::from_ref(&decl))?;
    }
    let by = caller.by();
    let check_name = decl.name.clone();
    let event = store::append(dir, move |state, _| {
        let mut next = CastSettings::of(state)
            .ok_or_else(|| Fail::refused("no studio recorded yet"))?;
        if next.checks.iter().any(|c| c.name == decl.name) {
            return Err(Fail::refused(format!(
                "check '{}' is already declared",
                decl.name
            )));
        }
        next.checks.push(decl.clone());
        cast::check_runnable(state, &next).map_err(|e| Fail::refused(e.to_string()))?;
        Ok::<Pending, Fail>(Pending {
            event: NewEvent::new(by, next.event_kind(None)),
            files: Vec::new(),
        })
    })?;
    Ok(format!("event {}: studio.cast +check {check_name}", event.seq))
}

fn remove(dir: &Path, caller: &Caller, name: &str) -> Result<String, Fail> {
    if name.trim().is_empty() {
        return Err(Fail::usage("--name must not be empty"));
    }
    let by = caller.by();
    let target = name.to_string();
    let event = store::append(dir, move |state, _| {
        let mut next = CastSettings::of(state)
            .ok_or_else(|| Fail::refused("no studio recorded yet"))?;
        let Some(pos) = next.checks.iter().position(|c| c.name == target) else {
            return Err(Fail::refused(format!("unknown check '{target}'")));
        };
        next.checks.remove(pos);
        cast::check_runnable(state, &next).map_err(|e| Fail::refused(e.to_string()))?;
        Ok::<Pending, Fail>(Pending {
            event: NewEvent::new(by, next.event_kind(None)),
            files: Vec::new(),
        })
    })?;
    Ok(format!("event {}: studio.cast -check {name}", event.seq))
}
