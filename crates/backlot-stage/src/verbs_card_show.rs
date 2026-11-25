//! Shared `card show` builder and CLI formatter returning [`CardShowResult`].

use std::fmt::Write as _;
use std::path::Path;

use backlot_model::hollywood::{self, CardAction, CardBase};
use backlot_model::req_results::{CardShowResult, IdentityChange, StudioNewDraft};

use crate::cli::{Env, Fail};
use crate::commands;
use crate::config;
use crate::hwood::{store_fail, studio_state};
use crate::store;

/// `card show <id>` — structured card read with base, identity transitions,
/// staleness check, and `studio-new` draft payloads.
pub(crate) fn card_show(base: &Path, env: &Env, id: u64) -> Result<CardShowResult, Fail> {
    let hdir = store::hollywood_dir(base);
    let hstate = store::load_hstate(&hdir).map_err(store_fail)?;
    let card = hstate
        .cards
        .get(&id)
        .ok_or_else(|| Fail::refused(format!("no card 'c{id}'")))?;

    let stale = match &card.action {
        CardAction::Setup { .. } => {
            let current = config::hollywood(&env.vars)
                .ok()
                .flatten()
                .map(|c| crate::hstage::identity(&c));
            hollywood::stale_hollywood(&card.base, current.as_ref())
        }
        _ => studio_state(base, card.action.studio()).ok().and_then(|s| {
            hollywood::stale(&card.action, &card.base, s.as_ref()).or_else(|| {
                crate::hcard_outward::outward_stale(&card.action, &card.base, s.as_ref())
            })
        }),
    };

    let identity_change = match (&card.action, &card.base) {
        (CardAction::Recast { actor, .. }, CardBase::RoleIdentity { actor: old }) => {
            Some(IdentityChange {
                old: Some(old.clone()),
                new: actor.clone(),
                old_label: format!(
                    "{} ({})",
                    backlot_model::recast::identity_text(old),
                    old.permission_profile
                ),
                new_label: format!(
                    "{} ({})",
                    backlot_model::recast::identity_text(actor),
                    actor.permission_profile
                ),
            })
        }
        (CardAction::Setup { .. }, CardBase::HollywoodIdentity { actor: old }) => {
            let new = card.action.setup_actor().expect("Setup yields actor");
            let old_label = match old {
                Some(a) => format!(
                    "{} ({})",
                    backlot_model::recast::identity_text(a),
                    a.permission_profile
                ),
                None => "(unconfigured)".into(),
            };
            let new_label = format!(
                "{} ({})",
                backlot_model::recast::identity_text(&new),
                new.permission_profile
            );
            Some(IdentityChange {
                old: old.clone(),
                new,
                old_label,
                new_label,
            })
        }
        _ => None,
    };

    let studio_new = if let CardAction::StudioNew {
        studio,
        title,
        repo,
        cast,
        brief,
        no_git,
    } = &card.action
    {
        let cast_path = crate::hcard_new::resolve_draft(&hdir, cast);
        let brief_path = crate::hcard_new::resolve_draft(&hdir, brief);
        let (cast_text, draft_error) = match std::fs::read_to_string(&cast_path) {
            Ok(text) => match commands::read_cast(&cast_path) {
                Ok(_) => (Some(text), None),
                Err(e) => (None, Some(e.message().to_string())),
            },
            Err(e) => (None, Some(e.to_string())),
        };
        let brief_text = std::fs::read_to_string(&brief_path).ok();
        Some(StudioNewDraft {
            studio: studio.clone(),
            title: title.clone(),
            repo: repo.clone(),
            no_git: *no_git,
            cast_path: cast_path.display().to_string(),
            brief_path: brief_path.display().to_string(),
            cast_text,
            brief_text,
            draft_error,
        })
    } else {
        None
    };

    let cast_diff = if let CardAction::Cast { studio, cast } = &card.action {
        let path = crate::hcard_new::resolve_draft(&hdir, cast);
        match crate::studio_cast::read(&path) {
            Ok(next) => match crate::hwood::studio_state(base, studio) {
                Ok(Some(s)) => backlot_model::cast::CastSettings::of(&s)
                    .map(|old| backlot_model::cast::diff(&old, &next)),
                _ => None,
            },
            Err(_) => None,
        }
    } else {
        None
    };

    Ok(CardShowResult {
        id: format!("c{}", card.seq),
        seq: card.seq,
        state: card.state.as_str().into(),
        action_label: card.action.label(),
        action: card.action.clone(),
        base: card.base.clone(),
        why: card.why.clone(),
        by: card.by.clone(),
        ts_ms: card.ts_ms,
        detail: (!card.detail.is_empty()).then(|| card.detail.clone()),
        stale,
        identity_change,
        studio_new,
        cast_diff,
    })
}

pub(crate) fn format_card_show(res: &CardShowResult) -> String {
    let mut out = format!(
        "{} {} · {} — {}",
        res.id, res.state, res.action_label, res.why
    );
    if let Some(detail) = &res.detail {
        let _ = write!(out, " ({detail})");
    }
    let _ = write!(out, "\nproposed by {}", res.by);
    if let Some(draft) = &res.studio_new {
        let _ = write!(
            out,
            "\nstudio: {}\ntitle: {}\nrepo: {}",
            draft.studio, draft.title, draft.repo
        );
        match draft
            .cast_text
            .as_deref()
            .and_then(|t| toml::from_str::<commands::CastFile>(t).ok())
        {
            Some(cast) => {
                let role = |name: &str, a: &backlot_model::event::ActorIdentity| {
                    format!(
                        "{name}: {}/{}{}",
                        a.runner,
                        a.model,
                        a.effort
                            .as_deref()
                            .map(|e| format!("/{e}"))
                            .unwrap_or_default()
                    )
                };
                out.push_str("\ncast:");
                for (name, a) in [
                    ("producer", &cast.producer),
                    ("director", &cast.director),
                    ("writer", &cast.writer),
                ] {
                    let _ = write!(out, "\n  {}", role(name, a));
                }
                if !cast.candidates.is_empty() {
                    let names: Vec<String> = cast
                        .candidates
                        .iter()
                        .map(|c| {
                            format!(
                                "{} ({}/{}{})",
                                c.name,
                                c.runner,
                                c.model,
                                c.effort
                                    .as_deref()
                                    .map(|e| format!("/{e}"))
                                    .unwrap_or_default()
                            )
                        })
                        .collect();
                    let _ = write!(out, "\ncandidates: {}", names.join(", "));
                }
                let mut limits = Vec::new();
                if let Some(v) = cast.rework_cap {
                    limits.push(format!("rework_cap {v}"));
                }
                if let Some(v) = cast.stall_limit_s {
                    limits.push(format!("stall_limit_s {v}"));
                }
                if let Some(v) = cast.task_budget_s {
                    limits.push(format!("task_budget_s {v}"));
                }
                for (runner, cap) in &cast.limits {
                    limits.push(format!("{runner} ≤{cap} live"));
                }
                if !limits.is_empty() {
                    let _ = write!(out, "\nlimits: {}", limits.join(" · "));
                }
            }
            None => {
                let err = draft.draft_error.as_deref().unwrap_or("unreadable cast");
                let _ = write!(out, "\ncast {}: {}", draft.cast_path, err);
            }
        }
        match &draft.brief_text {
            Some(text) => {
                let _ = write!(out, "\nbrief {}:", draft.brief_path);
                let mut lines = text.lines();
                for _ in 0..50 {
                    match lines.next() {
                        Some(line) => {
                            let _ = write!(out, "\n  {line}");
                        }
                        None => break,
                    }
                }
                if lines.next().is_some() {
                    out.push_str("\n  …");
                }
            }
            None => match std::fs::read_to_string(&draft.brief_path) {
                Ok(_) => {}
                Err(e) => {
                    let _ = write!(out, "\nbrief {}: {e}", draft.brief_path);
                }
            },
        }
    }
    if let (CardAction::Recast { person, actor, .. }, CardBase::RoleIdentity { actor: old }) =
        (&res.action, &res.base)
    {
        let _ = write!(
            out,
            "\n{person}: {} ({}) → {} ({})",
            backlot_model::recast::identity_text(old),
            old.permission_profile,
            backlot_model::recast::identity_text(actor),
            actor.permission_profile,
        );
    }
    if let (
        CardAction::Setup {
            runner,
            model,
            effort,
            ..
        },
        CardBase::HollywoodIdentity { actor: old },
    ) = (&res.action, &res.base)
    {
        out.push('\n');
        out.push_str(&hollywood::format_setup_transition(
            old.as_ref(),
            runner,
            model,
            effort.as_deref(),
        ));
    }
    crate::hcard_outward::append_outward_preview(&mut out, &res.action);
    // A `cast` card shows the dry-run diff its file would make, the same
    // text `studio cast set --dry-run` prints.
    if let Some(diff) = &res.cast_diff {
        out.push_str("\ncast change:");
        for line in diff.lines() {
            let _ = write!(out, "\n  {line}");
        }
    }
    out
}
