//! Dispatcher mapping validated [`ReqAction`] values to shared B1a handlers
//! under `Caller::doc_user` (`by.via = "doc"`).

use std::path::Path;

use backlot_model::EventBy;
use backlot_model::event::TaskStateChange;
use backlot_model::req_wire::ReqAction;
use serde_json::Value;

use crate::cli::{self, Caller, Env, Fail};
use crate::config::HollywoodCfg;
use crate::verbs;
use crate::verbs_design;
use crate::verbs_hwood;

/// Dispatch one validated [`ReqAction`] as `Caller::doc_user` through the
/// shared verb handlers, returning serialized typed JSON result.
pub(crate) fn dispatch_action(base: &Path, env: &Env, action: ReqAction) -> Result<Value, Fail> {
    match action {
        ReqAction::Tell { studio, role, text } => {
            let studio = cli::check_studio_name(&studio)?;
            let caller = Caller::doc_user(studio);
            to_json(verbs::tell_text(base, env, &caller, role, &text)?)
        }
        ReqAction::TellHollywood { text, answers } => to_json(verbs_hwood::tell_hollywood(
            base,
            EventBy::doc("user"),
            &text,
            &answers,
        )?),
        ReqAction::CardApply { id } => to_json(verbs_hwood::card_apply(
            base,
            env,
            EventBy::doc("user"),
            id,
        )?),
        ReqAction::CardCancel { id, reason } => to_json(verbs_hwood::card_cancel(
            base,
            EventBy::doc("user"),
            id,
            &reason,
        )?),
        ReqAction::CardShow { id } => to_json(verbs_hwood::card_show(base, env, id)?),
        ReqAction::AskAcknowledge {
            studio,
            role,
            target,
        } => {
            let studio = cli::check_studio_name(&studio)?;
            let caller = Caller::doc_user(studio);
            to_json(verbs::ask_acknowledge(base, &caller, role, target)?)
        }
        ReqAction::AttentionClear { studio, target } => {
            let studio = cli::check_studio_name(&studio)?;
            let caller = Caller::doc_user(studio);
            to_json(verbs::attention_clear(base, &caller, target, false)?)
        }
        ReqAction::Recast {
            studio,
            person,
            runner,
            model,
            effort,
            permission_profile,
            reason,
        } => {
            let studio = cli::check_studio_name(&studio)?;
            let caller = Caller::doc_user(studio);
            let (res, _) = verbs::recast(
                base,
                env,
                &caller,
                person,
                runner,
                model,
                effort,
                permission_profile,
                reason,
            )?;
            to_json(res)
        }
        ReqAction::Restart {
            studio,
            person,
            fresh,
            reason,
        } => {
            if person == backlot_model::hollywood::HOLLYWOOD {
                to_json(verbs_hwood::restart_hollywood(
                    base,
                    env,
                    EventBy::doc("user"),
                    fresh,
                    &reason,
                )?)
            } else {
                let s = studio.ok_or_else(|| Fail::usage("missing studio for role restart"))?;
                let s = cli::check_studio_name(&s)?;
                let caller = Caller::doc_user(s);
                to_json(verbs::restart_role(
                    base, env, &caller, person, fresh, reason,
                )?)
            }
        }
        ReqAction::Close { studio, person } => {
            let studio = cli::check_studio_name(&studio)?;
            let caller = Caller::doc_user(studio);
            to_json(verbs::close(base, env, &caller, person)?)
        }
        ReqAction::Reopen { studio, person } => {
            let studio = cli::check_studio_name(&studio)?;
            let caller = Caller::doc_user(studio);
            to_json(verbs::reopen(base, env, &caller, person)?)
        }
        ReqAction::TaskPriority {
            studio,
            task,
            level,
        } => {
            let studio = cli::check_studio_name(&studio)?;
            let mut caller = Caller::doc_user(studio);
            caller.task = Some(task);
            to_json(verbs::task_priority(base, env, &caller, level)?)
        }
        ReqAction::TaskPause {
            studio,
            task,
            reason,
        } => task_state_doc(base, env, studio, task, TaskStateChange::Paused, reason),
        ReqAction::TaskResume {
            studio,
            task,
            reason,
        } => task_state_doc(base, env, studio, task, TaskStateChange::Resumed, reason),
        ReqAction::TaskAbandon {
            studio,
            task,
            reason,
        } => task_state_doc(base, env, studio, task, TaskStateChange::Abandoned, reason),
        ReqAction::HollywoodSetup {
            runner,
            model,
            effort,
            key,
            default_cast,
            force_offline,
        } => {
            let cfg = HollywoodCfg {
                runner,
                model,
                effort,
                key,
                default_cast,
            };
            let (res, _) =
                verbs_hwood::hollywood_setup_checked(Some(base), env, &cfg, force_offline)?;
            to_json(res)
        }
        ReqAction::HollywoodSwitch {
            runner,
            model,
            effort,
            timeout_s,
            force,
        } => to_json(crate::hswitch_verbs::rpc_switch(
            base,
            env,
            &runner,
            &model,
            effort.as_deref(),
            timeout_s,
            force,
        )?),
        ReqAction::HollywoodSwitchCancel => to_json(crate::hswitch_verbs::rpc_cancel(base, env)?),
        ReqAction::HollywoodSwitchStatus => to_json(crate::hswitch_verbs::rpc_status(base, env)?),
        // `studio cast show|set`: the cast file `set` carries is content,
        // staged into a temp file the hub parses with `studio new`'s own
        // rules — the same `verbs::cast_set` the local CLI runs.
        ReqAction::StudioCastShow { studio, json } => {
            let studio = cli::check_studio_name(&studio)?;
            to_json(verbs::cast_show(base, &studio, json)?)
        }
        ReqAction::StudioCastSet {
            studio,
            cast,
            dry_run,
            json,
        } => {
            let studio = cli::check_studio_name(&studio)?;
            let file = crate::doc_dispatch_studio::stage_content("cast", &cast)?;
            let caller = Caller::doc_user(&studio);
            to_json(verbs::cast_set_result(
                base,
                env,
                &caller,
                file.path(),
                dry_run,
                json,
            )?)
        }
        ReqAction::DesignSign {
            studio,
            skip,
            reason,
            note,
        } => {
            let studio = cli::check_studio_name(&studio)?;
            let caller = Caller::doc_user(studio);
            to_json(verbs_design::design_sign(
                base, &caller, skip, reason, note,
            )?)
        }
        ReqAction::DesignStartRound { studio } => {
            let studio = cli::check_studio_name(&studio)?;
            let caller = Caller::doc_user(studio);
            to_json(verbs_design::design_start_round(base, &caller)?)
        }
        ReqAction::Version => {
            unreachable!("the version probe is dispatched by doc_dispatch_studio")
        }
        other => unreachable!("studio actions are dispatched by doc_dispatch_studio: {other:?}"),
    }
}

fn task_state_doc(
    base: &Path,
    env: &Env,
    studio: String,
    task: String,
    change: TaskStateChange,
    reason: String,
) -> Result<Value, Fail> {
    let studio = cli::check_studio_name(&studio)?;
    let mut caller = Caller::doc_user(studio);
    caller.task = Some(task);
    to_json(verbs::task_state(base, env, &caller, change, reason)?)
}

fn to_json<T: serde::Serialize>(val: T) -> Result<Value, Fail> {
    serde_json::to_value(val).map_err(|e| Fail::refused(format!("result serialize: {e}")))
}
