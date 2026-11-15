//! The `studio-new` card's execution half (D18): the draft
//! re-validated at apply time, `studio.created` recorded with the
//! cast file copied in, and the brief queued as the producer's
//! first message — `purpose: "brief"` is this path's marker, so a
//! restart that finds it records `card.done` and a crash between
//! `studio.created` and the brief lands just the brief. The card
//! pipeline itself is `hcard.rs`.

use std::path::Path;

use backlot_model::event::EventKind;
use backlot_model::hollywood::{Card, CardAction};
use backlot_model::state::StudioState;

use crate::commands;
use crate::daemon::Stage;
use crate::hcard::{Step, append_studio_files};
use crate::store::{self, PendingFile};

impl Stage {
    /// The studio-new card's step: the draft re-validated now (the
    /// dry run again at apply time), then `studio.created` — cast
    /// file copied in — and the brief queued to the producer. A
    /// studio the card already made (its brief message is the
    /// marker) is `Done`; one made any other way is `Stale` — a
    /// `studio.created` newer than the card's own `card.applied`
    /// lands just the missing brief (the crash window), anything
    /// else stands as a moved base.
    pub(crate) fn studio_new_step(
        &self,
        hdir: &Path,
        sdir: &Path,
        card: &Card,
        sstate: Option<StudioState>,
    ) -> Step {
        let CardAction::StudioNew {
            studio,
            title,
            repo,
            cast,
            brief,
        } = &card.action
        else {
            return Step::Fail("not a studio-new card".into());
        };
        let cast_path = resolve_draft(hdir, cast);
        let brief_path = resolve_draft(hdir, brief);
        if let Some(s) = sstate {
            if studio_new_done(&s) {
                return Step::Done;
            }
            // Created after this card was applied: this card's own
            // earlier pass got it that far — land the brief that
            // completes it. Created before, or with no marker —
            // someone else's studio: the base moved.
            let applied_ms = applied_ts(hdir, card);
            let created_ms = studio_created_ts(sdir);
            return match (applied_ms, created_ms) {
                (Some(a), Some(c)) if c >= a => match queue_brief(sdir, &brief_path) {
                    Ok(()) => Step::Done,
                    Err(e) => Step::Fail(e),
                },
                _ => Step::Stale(format!("studio '{studio}' exists now")),
            };
        }
        let env = crate::cli::Env {
            vars: self.env_vars.clone(),
        };
        if let Err(e) = crate::draft::check(&self.base, studio, repo, &cast_path, &env) {
            return Step::Fail(e.message().to_string());
        }
        let cast_data = match commands::read_cast(&cast_path) {
            Ok(c) => c,
            Err(e) => return Step::Fail(e.message().to_string()),
        };
        let kind = match studio_created_kind(&cast_data, title, repo) {
            Ok(k) => k,
            Err(e) => return Step::Fail(e),
        };
        match append_studio_files(
            sdir,
            kind,
            vec![PendingFile {
                src: cast_path,
                dest: "cast.toml".into(),
                append: None,
            }],
        ) {
            Err(e) => Step::Fail(e),
            Ok(()) => match queue_brief(sdir, &brief_path) {
                Ok(()) => Step::Done,
                Err(e) => Step::Fail(e),
            },
        }
    }
}

/// The studio-new marker: the brief became the producer's first
/// message — `purpose: "brief"` is the card path's alone.
pub(crate) fn studio_new_done(s: &StudioState) -> bool {
    s.messages
        .iter()
        .any(|m| m.to == "producer" && m.purpose.as_deref() == Some("brief"))
}

/// A draft path on the card: relative paths resolve against
/// Hollywood's own directory — where `hollywood propose` ran —
/// absolute ones stay.
fn resolve_draft(hdir: &Path, p: &str) -> std::path::PathBuf {
    let path = Path::new(p);
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        hdir.join(path)
    }
}

/// The `card.applied` event's ts — a `studio.created` at or after
/// it belongs to this card's execution, not to anyone else.
fn applied_ts(hdir: &Path, card: &Card) -> Option<u64> {
    let applied = card.applied_seq?;
    store::load_hevents(hdir)
        .ok()?
        .0
        .iter()
        .find(|e| e.seq == applied)
        .map(|e| e.ts_ms)
}

/// The `studio.created` event's ts — the "did this card make it"
/// check against the apply's own ts.
fn studio_created_ts(sdir: &Path) -> Option<u64> {
    store::load_events(sdir)
        .ok()?
        .0
        .iter()
        .find(|e| matches!(e.kind, EventKind::StudioCreated { .. }))
        .map(|e| e.ts_ms)
}

/// The `studio.created` kind from a parsed cast — the same fields
/// `studio new` builds.
fn studio_created_kind(
    cast: &commands::CastFile,
    title: &str,
    repo: &str,
) -> Result<EventKind, String> {
    let (check_env, checks) = crate::checks::declarations(&cast.check_env, &cast.checks)
        .map_err(|e| e.message().to_string())?;
    Ok(EventKind::StudioCreated {
        title: title.to_string(),
        repo: repo.to_string(),
        mode: cast.mode.unwrap_or_default(),
        roles: Box::new(backlot_model::event::RoleConfigs {
            producer: cast.producer.clone(),
            director: cast.director.clone(),
            writer: cast.writer.clone(),
        }),
        candidates: cast.candidates.clone(),
        rework_cap: cast
            .rework_cap
            .unwrap_or_else(backlot_model::event::default_rework_cap),
        stall_limit_s: cast
            .stall_limit_s
            .unwrap_or_else(backlot_model::event::default_stall_limit_s),
        nudge_after_s: cast
            .nudge_after_s
            .unwrap_or_else(backlot_model::event::default_nudge_after_s),
        escalate_after_s: cast
            .escalate_after_s
            .unwrap_or_else(backlot_model::event::default_escalate_after_s),
        check_env,
        checks,
        task_budget_s: cast.task_budget_s,
        limits: cast.limits.clone(),
    })
}

/// The brief lands as the producer's first message — `purpose:
/// "brief"`, the file under `messages/` like a `tell`'s. A
/// purpose-bearing message is the stage's or the producer's word
/// (the role gate's rule), so the stage records this one: the
/// card's own action stays `by: user`, the brief is delivery
/// plumbing.
fn queue_brief(sdir: &Path, brief: &Path) -> Result<(), String> {
    let src = brief.to_path_buf();
    store::append(sdir, move |_, seq| {
        let dest = format!("messages/{seq}-brief.md");
        Ok::<_, std::convert::Infallible>(store::Pending {
            event: backlot_model::NewEvent::new(
                backlot_model::EventBy::role("stage"),
                EventKind::MessageQueued {
                    to: "producer".into(),
                    file: Some(dest.clone()),
                    purpose: Some("brief".into()),
                    source_seq: None,
                },
            ),
            files: vec![PendingFile {
                src: src.clone(),
                dest,
                append: None,
            }],
        })
    })
    .map(|_| ())
    .map_err(|e| match e {
        store::StoreError::Io(e) => format!("brief: {e}"),
        store::StoreError::Corrupt(c) => c.to_string(),
        store::StoreError::Refused(r) => r.to_string(),
        store::StoreError::Build(_) => "unexpected error".into(),
    })
}
