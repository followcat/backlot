//! The base a proposed card holds (D17): the fact about its studio
//! or task that must still be true when the user applies it.
//! `card_base` computes it from the live state — and refuses a
//! proposal whose fact does not hold today, since a card born stale
//! fools nobody.

use std::path::Path;

use backlot_model::hollywood::{CardAction, CardBase, status_str};
use backlot_model::state::{RoleLifecycle, RoleState, StudioState, Task, TaskStatus};

use crate::cli::{Env, Fail};
use crate::commands;
use crate::hwood::require_studio;

fn lifecycle_str(r: &RoleState) -> &'static str {
    match r.lifecycle() {
        RoleLifecycle::Live => "live",
        RoleLifecycle::Lost => "lost",
        RoleLifecycle::Closed => "closed",
    }
}

fn require_task<'a>(s: &'a StudioState, studio: &str, task: &str) -> Result<&'a Task, Fail> {
    s.tasks
        .get(task)
        .ok_or_else(|| Fail::refused(format!("no task '{task}' in studio '{studio}'")))
}

/// The action as the card will store it. A recast arrives with an
/// empty permission profile (the propose flags do not take one); it is
/// filled from the role's current identity, and the whole identity is
/// checked before the card exists.
pub(crate) fn resolve_action(
    base: &Path,
    action: &CardAction,
    env: &Env,
) -> Result<CardAction, Fail> {
    match action {
        CardAction::Recast {
            studio,
            person,
            actor,
        } => {
            let s = require_studio(base, studio)?;
            let current = backlot_model::recast::current_identity(&s, person)
                .map_err(|e| Fail::refused(e.to_string()))?
                .ok_or_else(|| {
                    Fail::refused(format!("no person '{person}' in studio '{studio}'"))
                })?;
            let mut actor = actor.clone();
            if actor.permission_profile.trim().is_empty() {
                actor.permission_profile = current.permission_profile.clone();
            }
            backlot_model::recast::check(&s, person, &actor)
                .map_err(|e| Fail::refused(e.to_string()))?;
            let named = std::collections::BTreeSet::from([actor.runner.clone()]);
            crate::draft::check_runners(base, &named, env)?;
            Ok(CardAction::Recast {
                studio: studio.clone(),
                person: person.clone(),
                actor,
            })
        }
        CardAction::Setup {
            runner,
            model,
            effort,
            ..
        } => {
            crate::hsetup::validate_hollywood_target(env, runner, model, effort.as_deref())?;
            Ok(action.clone())
        }
        _ => Ok(action.clone()),
    }
}

/// The card's base, computed from the live studio state — or the
/// refusal when the fact it would hold is already false.
pub(crate) fn card_base(base: &Path, action: &CardAction, env: &Env) -> Result<CardBase, Fail> {
    match action {
        CardAction::StudioNew {
            studio,
            repo,
            cast,
            brief,
            no_git,
            ..
        } => {
            // The same validation `studio new --dry-run` runs (D19).
            crate::draft::check(base, studio, repo, Path::new(cast), env, false, *no_git)?;
            commands::must_exist(Path::new(brief), "--brief")?;
            Ok(CardBase::StudioAbsent)
        }
        CardAction::Close { studio, person } => {
            let s = require_studio(base, studio)?;
            match s.roles.get(person) {
                None => Err(Fail::refused(format!(
                    "no role '{person}' in studio '{studio}'"
                ))),
                Some(r) if r.held => Ok(CardBase::PersonHeld),
                Some(r) => Err(Fail::refused(format!(
                    "role '{person}' in '{studio}' is {}, not held — nothing to close",
                    lifecycle_str(r)
                ))),
            }
        }
        CardAction::Reopen { studio, person } => {
            let s = require_studio(base, studio)?;
            match s.roles.get(person) {
                None => Err(Fail::refused(format!(
                    "no role '{person}' in studio '{studio}'"
                ))),
                Some(r) if r.held => Err(Fail::refused(format!(
                    "role '{person}' in '{studio}' is already held"
                ))),
                Some(r) if r.lifecycle() != RoleLifecycle::Closed => Err(Fail::refused(format!(
                    "role '{person}' in '{studio}' is {}, not closed",
                    lifecycle_str(r)
                ))),
                Some(r) if r.session_id.is_none() => Err(Fail::refused(format!(
                    "role '{person}' in '{studio}' has no session to resume"
                ))),
                Some(_) => Ok(CardBase::PersonClosed),
            }
        }
        CardAction::Priority { studio, task, .. } => {
            let s = require_studio(base, studio)?;
            Ok(CardBase::TaskPriority {
                was: require_task(&s, studio, task)?.priority,
            })
        }
        CardAction::Pause { studio, task } => {
            let s = require_studio(base, studio)?;
            let t = require_task(&s, studio, task)?;
            if t.status != TaskStatus::Running {
                return Err(Fail::refused(format!(
                    "task '{task}' in '{studio}' is {}, not running — nothing to pause",
                    status_str(t.status)
                )));
            }
            Ok(CardBase::TaskStatus {
                was: TaskStatus::Running,
            })
        }
        CardAction::Resume { studio, task } => {
            let s = require_studio(base, studio)?;
            let t = require_task(&s, studio, task)?;
            if !matches!(t.status, TaskStatus::Paused | TaskStatus::Blocked) {
                return Err(Fail::refused(format!(
                    "task '{task}' in '{studio}' is {}, not paused or blocked — nothing to resume",
                    status_str(t.status)
                )));
            }
            Ok(CardBase::TaskStatus { was: t.status })
        }
        CardAction::Abandon { studio, task } => {
            let s = require_studio(base, studio)?;
            let t = require_task(&s, studio, task)?;
            if t.status.is_terminal() {
                return Err(Fail::refused(format!(
                    "task '{task}' in '{studio}' is {} — already terminal",
                    status_str(t.status)
                )));
            }
            Ok(CardBase::TaskStatus { was: t.status })
        }
        // A recast is born stale the moment the role's identity moves;
        // the proposal runs the same rule `backlot recast` does, so an
        // invalid identity never becomes a card.
        CardAction::Recast {
            studio,
            person,
            actor,
        } => {
            let s = require_studio(base, studio)?;
            backlot_model::recast::check(&s, person, actor)
                .map_err(|e| Fail::refused(e.to_string()))?;
            let current = s.fixed_identity(person).cloned().ok_or_else(|| {
                Fail::refused(format!("no fixed role '{person}' in studio '{studio}'"))
            })?;
            Ok(CardBase::RoleIdentity { actor: current })
        }
        CardAction::Setup { .. } => {
            let current = crate::config::hollywood(&env.vars)
                .map_err(Fail::refused)?
                .map(|c| crate::hstage::identity(&c));
            Ok(CardBase::HollywoodIdentity { actor: current })
        }
        CardAction::Restart { studio, person, .. } => {
            let s = require_studio(base, studio)?;
            if s.finished.is_some() {
                return Err(Fail::refused(format!("studio '{studio}' is finished")));
            }
            match s.roles.get(person) {
                None => Err(Fail::refused(format!(
                    "no role '{person}' in studio '{studio}'"
                ))),
                Some(r) if r.lifecycle() == RoleLifecycle::Closed => Err(Fail::refused(format!(
                    "role '{person}' in '{studio}' is closed — reopen it instead"
                ))),
                Some(_) => Ok(CardBase::PersonRestartable),
            }
        }
        // A cast card base is the seq of the settings event it edits;
        // the file is validated up front so a card born stale never
        // exists.
        CardAction::Cast { studio, cast } => {
            let s = require_studio(base, studio)?;
            if s.finished.is_some() {
                return Err(Fail::refused(format!("studio '{studio}' is finished")));
            }
            let path = crate::hcard_new::resolve_draft(&crate::store::hollywood_dir(base), cast);
            let next = crate::studio_cast::read(&path)?;
            crate::studio_cast::check(base, env, &s, &next)?;
            Ok(CardBase::CastSeq {
                was: s.cast_seq.unwrap_or(0),
            })
        }
        CardAction::Outward { .. } => Err(Fail::refused("outward cards are proposed by the stage")),
    }
}
