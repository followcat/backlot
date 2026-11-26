//! The CLI command → request mapping for a remote server (B1b): no
//! network I/O, and body content (`--cast`, `--ask-file`, stdin) is
//! read here, on the client.

use std::path::Path;

use crate::args::{CardCmd, CastCmd, Cmd, Common, DesignCmd, HollywoodCmd, TextSource};
use crate::cli::{Env, Fail};
use backlot_model::event::TaskStateChange;
use backlot_model::hollywood::HOLLYWOOD;
use backlot_model::req_wire::ReqAction;

use super::{MappedCmd, check_user_caller, read_body, read_text_source, task_pin, with_studio};

/// Convert a CLI `Cmd` into a [`MappedCmd`] without touching network I/O.
pub(super) fn map(
    common: &Common,
    cmd: &Cmd,
    env: &Env,
    stdin: Option<&str>,
) -> Result<MappedCmd, Fail> {
    if !matches!(cmd, Cmd::Card(CardCmd::Show { .. })) {
        if matches!(cmd, Cmd::Recast { .. }) && env.get("BACKLOT_ROLE").is_some() {
            return Err(Fail::refused(
                "a recast is the user's — BACKLOT_ROLE is set",
            ));
        }
        check_user_caller(common, env)?;
    }
    let mapped = match cmd {
        Cmd::Tell { role, source, .. } => {
            let (role, text) = (role.clone(), read_text_source(source, stdin)?);
            with_studio(common, env, move |studio| ReqAction::Tell {
                studio,
                role,
                text,
            })
        }
        Cmd::TellHollywood { text } => MappedCmd::Action(ReqAction::TellHollywood {
            text: text.clone(),
            answers: Vec::new(),
        }),
        Cmd::Card(CardCmd::Show { id }) => MappedCmd::Action(ReqAction::CardShow { id: *id }),
        Cmd::Card(CardCmd::Apply { id }) => MappedCmd::Action(ReqAction::CardApply { id: *id }),
        Cmd::Card(CardCmd::Cancel { id, reason }) => MappedCmd::Action(ReqAction::CardCancel {
            id: *id,
            reason: reason.clone(),
        }),
        Cmd::AttentionClear { target } => {
            let target = *target;
            with_studio(common, env, move |studio| ReqAction::AttentionClear {
                studio,
                target,
            })
        }
        Cmd::Recast {
            person,
            runner,
            model,
            effort,
            permission_profile,
            reason,
        } => {
            let (p, r, m, e, pp, re) = (
                person.clone(),
                runner.clone(),
                model.clone(),
                effort.clone(),
                permission_profile.clone(),
                reason.clone(),
            );
            with_studio(common, env, move |studio| ReqAction::Recast {
                studio,
                person: p,
                runner: r,
                model: m,
                effort: e,
                permission_profile: pp,
                reason: re,
            })
        }
        Cmd::Restart {
            person,
            fresh,
            reason,
        } => {
            let (person, fresh, reason) = (person.clone(), *fresh, reason.clone());
            if person == HOLLYWOOD {
                MappedCmd::Action(ReqAction::Restart {
                    studio: None,
                    person,
                    fresh,
                    reason,
                })
            } else {
                with_studio(common, env, move |studio| ReqAction::Restart {
                    studio: Some(studio),
                    person,
                    fresh,
                    reason,
                })
            }
        }
        Cmd::Close { person } => {
            let person = person.clone();
            with_studio(common, env, move |studio| ReqAction::Close {
                studio,
                person,
            })
        }
        Cmd::Reopen { person } => {
            let person = person.clone();
            with_studio(common, env, move |studio| ReqAction::Reopen {
                studio,
                person,
            })
        }
        Cmd::Priority { level } => {
            let (task, level) = (task_pin(common, env)?, *level);
            with_studio(common, env, move |studio| ReqAction::TaskPriority {
                studio,
                task,
                level,
            })
        }
        Cmd::TaskState { change, reason } => map_task_state(common, env, *change, reason)?,
        Cmd::Hollywood(HollywoodCmd::Setup { cfg, force_offline }) => {
            MappedCmd::Action(ReqAction::HollywoodSetup {
                runner: cfg.runner.clone(),
                model: cfg.model.clone(),
                effort: cfg.effort.clone(),
                key: cfg.key.clone(),
                default_cast: cfg.default_cast.clone(),
                force_offline: *force_offline,
            })
        }
        Cmd::Hollywood(HollywoodCmd::Switch {
            runner,
            model,
            effort,
            timeout_s,
            force,
            ..
        }) => MappedCmd::Action(ReqAction::HollywoodSwitch {
            runner: runner.clone(),
            model: model.clone(),
            effort: effort.clone(),
            timeout_s: *timeout_s,
            force: *force,
        }),
        Cmd::Hollywood(HollywoodCmd::SwitchCancel { .. }) => {
            MappedCmd::Action(ReqAction::HollywoodSwitchCancel)
        }
        Cmd::Hollywood(HollywoodCmd::SwitchStatus { .. }) => {
            MappedCmd::Action(ReqAction::HollywoodSwitchStatus)
        }
        Cmd::StudioNew {
            title,
            repo,
            cast_file,
            dry_run,
            interactive,
            mode,
            init,
            no_git,
            ask,
        } => {
            if *interactive {
                return Err(Fail::usage(
                    "studio new --interactive runs where the terminal is; \
                     pass --title, --repo and --cast for a remote studio",
                ));
            }
            let id = crate::cli::studio_name(common, env)?;
            let title = title.clone().unwrap_or_default();
            let repo = repo.clone().ok_or_else(|| Fail::usage("missing --repo"))?;
            let cast_file = cast_file
                .clone()
                .ok_or_else(|| Fail::usage("missing --cast"))?;
            let cast = read_body(&cast_file, stdin, "--cast")?;
            let ask = match ask {
                None => None,
                Some(TextSource::Text(text)) => Some(text.clone()),
                Some(TextSource::File(path)) => Some(read_body(path, stdin, "--ask-file")?),
                Some(TextSource::Stdin) => Some(read_body(Path::new("-"), stdin, "--ask-file")?),
            };
            if ask.as_deref().is_some_and(|body| body.trim().is_empty()) {
                return Err(Fail::refused("the ask is empty"));
            }
            MappedCmd::Action(ReqAction::StudioNew {
                id,
                title,
                repo,
                cast,
                mode: *mode,
                init: *init,
                no_git: *no_git,
                ask,
                dry_run: *dry_run,
            })
        }
        Cmd::StudioFinish { reason, force } => {
            let (reason, force) = (reason.clone(), *force);
            with_studio(common, env, move |studio| ReqAction::StudioFinish {
                studio,
                reason,
                force,
            })
        }
        Cmd::StudioReopen { reason } => {
            let reason = reason.clone();
            with_studio(common, env, move |studio| ReqAction::StudioReopen {
                studio,
                reason,
            })
        }
        Cmd::StudioCast(CastCmd::Show { json }) => {
            let json = *json;
            with_studio(common, env, move |studio| ReqAction::StudioCastShow {
                studio,
                json,
            })
        }
        // The cast file is read on the client and sent as content — the
        // hub never resolves a client path.
        Cmd::StudioCast(CastCmd::Set {
            file,
            dry_run,
            json,
        }) => {
            let cast = read_body(file, stdin, "--file")?;
            let (dry_run, json) = (*dry_run, *json);
            with_studio(common, env, move |studio| ReqAction::StudioCastSet {
                studio,
                cast,
                dry_run,
                json,
            })
        }
        Cmd::StudioPrune {
            studio,
            all_finished,
            dry_run,
            force,
            json: _,
        } => {
            let (all_finished, dry_run, force) = (*all_finished, *dry_run, *force);
            match studio {
                Some(name) => MappedCmd::Action(ReqAction::StudioPrune {
                    studio: Some(name.clone()),
                    all_finished,
                    dry_run,
                    force,
                }),
                None if all_finished => MappedCmd::Action(ReqAction::StudioPrune {
                    studio: None,
                    all_finished,
                    dry_run,
                    force,
                }),
                None => with_studio(common, env, move |studio| ReqAction::StudioPrune {
                    studio: Some(studio),
                    all_finished,
                    dry_run,
                    force,
                }),
            }
        }
        Cmd::StudioDelete {
            studio,
            dry_run,
            force,
            json: _,
        } => {
            let (dry_run, force) = (*dry_run, *force);
            match studio {
                Some(name) => MappedCmd::Action(ReqAction::StudioDelete {
                    studio: Some(name.clone()),
                    dry_run,
                    force,
                }),
                None => with_studio(common, env, move |studio| ReqAction::StudioDelete {
                    studio: Some(studio),
                    dry_run,
                    force,
                }),
            }
        }
        Cmd::StudioRestore {
            archived,
            new_name,
            dry_run,
            json: _,
        } => MappedCmd::Action(ReqAction::StudioRestore {
            archived: archived.clone(),
            name: new_name.clone(),
            dry_run: *dry_run,
        }),
        Cmd::Repos { json: _, from, all } => MappedCmd::Action(ReqAction::Repos {
            from: from.clone(),
            all: *all,
        }),
        Cmd::Runners { json: _ } => MappedCmd::Action(ReqAction::Runners),
        Cmd::Design(DesignCmd::Sign { skip, reason, note }) => {
            let (skip, reason, note) = (*skip, reason.clone(), note.clone());
            with_studio(common, env, move |studio| ReqAction::DesignSign {
                studio,
                skip,
                reason,
                note,
            })
        }
        Cmd::Design(DesignCmd::Open {
            req: None,
            suggest_skip: false,
            reason: None,
        }) => with_studio(common, env, |studio| ReqAction::DesignStartRound { studio }),
        _ => MappedCmd::Unsupported,
    };
    Ok(mapped)
}

fn map_task_state(
    common: &Common,
    env: &Env,
    change: TaskStateChange,
    reason: &str,
) -> Result<MappedCmd, Fail> {
    let (task, reason) = (task_pin(common, env)?, reason.to_owned());
    Ok(match change {
        TaskStateChange::Paused => with_studio(common, env, move |s| ReqAction::TaskPause {
            studio: s,
            task,
            reason,
        }),
        TaskStateChange::Resumed => with_studio(common, env, move |s| ReqAction::TaskResume {
            studio: s,
            task,
            reason,
        }),
        TaskStateChange::Abandoned => with_studio(common, env, move |s| ReqAction::TaskAbandon {
            studio: s,
            task,
            reason,
        }),
        TaskStateChange::TakenOver | TaskStateChange::Blocked => MappedCmd::Unsupported,
    })
}
