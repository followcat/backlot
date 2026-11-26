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
    /// studio this card finished (its own brief message is the
    /// marker) is `Done`; any other brief means a studio-new ran
    /// here without this card — the base moved (R1-3). A
    /// `studio.created` newer than the card's own `card.applied`
    /// with this card's title is the crash window: land just the
    /// missing brief. Anything else stands as a moved base.
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
            no_git,
            ..
        } = &card.action
        else {
            return Step::Fail("not a studio-new card".into());
        };
        if let Some(reason) = crate::hcard_snapshot::stale(&card.action) {
            return Step::Stale(reason);
        }
        let cast_path = resolve_draft(hdir, cast);
        let brief_path = resolve_draft(hdir, brief);
        if let Some(s) = sstate {
            if studio_new_done(&s, card.seq) {
                return Step::Done;
            }
            // A brief that isn't this card's — another studio-new
            // completed here: ours would double the producer's
            // brief, so the base moved (R1-3).
            if any_brief(&s) {
                return Step::Stale(format!("studio '{studio}' has its brief already"));
            }
            // Created after this card was applied, under this card's
            // title: this card's own earlier pass got it that far —
            // land the brief that completes it. Created before, or
            // another title — someone else's studio: the base moved.
            let applied_ms = applied_ts(hdir, card);
            let created_ms = studio_created_ts(sdir);
            let ours = s.studio.as_ref().is_some_and(|c| c.title == *title);
            return match (applied_ms, created_ms) {
                (Some(a), Some(c)) if c >= a && ours => {
                    match queue_brief(sdir, &brief_path, card.seq) {
                        Ok(()) => Step::Done,
                        Err(e) => Step::Fail(e),
                    }
                }
                _ => Step::Stale(format!("studio '{studio}' exists now")),
            };
        }
        let env = crate::cli::Env {
            vars: self.env_vars.clone(),
        };
        let binding =
            match crate::draft::check(&self.base, studio, repo, &cast_path, &env, false, *no_git) {
                Ok(b) => b,
                Err(e) => return Step::Fail(e.message().to_string()),
            };
        // The launch-time init the draft check promised: the card's
        // repo or member repositories may still hold no commit (D4).
        if let Err(e) = crate::paths::initialize(repo, &binding.members, binding.no_git, false) {
            return Step::Fail(e.message().to_string());
        }
        let cast_data = match commands::read_cast(&cast_path) {
            Ok(c) => c,
            Err(e) => return Step::Fail(e.message().to_string()),
        };
        let kind = match studio_created_kind(&cast_data, title, repo, &binding) {
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
            Ok(()) => match queue_brief(sdir, &brief_path, card.seq) {
                Ok(()) => Step::Done,
                Err(e) => Step::Fail(e),
            },
        }
    }
}

/// This card's studio-new marker: its brief became the producer's
/// first message. `purpose: "brief"` alone belongs to whatever card
/// ran first — the file name carries the card id so a second
/// studio-new for the same name sees the studio moved, not done
/// (R1-3).
pub(crate) fn studio_new_done(s: &StudioState, card_seq: u64) -> bool {
    let tag = format!("-brief-c{card_seq}.");
    s.messages.iter().any(|m| {
        m.to == "producer"
            && m.purpose.as_deref() == Some("brief")
            && m.file.as_deref().is_some_and(|f| f.contains(&tag))
    })
}

/// Any brief at all — the "someone else's studio" check: whoever
/// landed it, this card is not the studio's maker.
fn any_brief(s: &StudioState) -> bool {
    s.messages
        .iter()
        .any(|m| m.to == "producer" && m.purpose.as_deref() == Some("brief"))
}

/// A draft path on the card: relative paths resolve against
/// Hollywood's own directory — where `hollywood propose` ran —
/// absolute ones stay. A relative path that climbs out (`..`) is
/// not resolved: the caller's file open simply misses inside `hdir`
/// rather than serving a host file the card named (D17a).
pub(crate) fn resolve_draft(hdir: &Path, p: &str) -> std::path::PathBuf {
    let path = Path::new(p);
    if path.is_absolute() {
        path.to_path_buf()
    } else if path.components().all(|c| {
        matches!(
            c,
            std::path::Component::Normal(_) | std::path::Component::CurDir
        )
    }) {
        hdir.join(path)
    } else {
        // Any `..` (or other non-normal component): resolve inside
        // `hdir` under a leaf that can't exist — the draft read
        // fails closed instead of escaping.
        hdir.join(".refused")
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
    binding: &crate::draft::Binding,
) -> Result<EventKind, String> {
    let (check_env, checks) = crate::checks::declarations(&cast.check_env, &cast.checks)
        .map_err(|e| e.message().to_string())?;
    let push = commands::build_push_policy(&cast.push, repo, &binding.members, binding.no_git);
    Ok(EventKind::StudioCreated {
        title: title.to_string(),
        repo: repo.to_string(),
        members: binding.members.clone(),
        no_git: binding.no_git,
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
        max_actors: cast
            .max_actors
            .unwrap_or_else(backlot_model::event::default_max_actors),
        push,
    })
}

/// The brief lands as the producer's first message — `purpose:
/// "brief"`, the file under `messages/` like a `tell`'s. A
/// purpose-bearing message is the stage's or the producer's word
/// (the role gate's rule), so the stage records this one: the
/// card's own action stays `by: user`, the brief is delivery
/// plumbing.
fn queue_brief(sdir: &Path, brief: &Path, card_seq: u64) -> Result<(), String> {
    let src = brief.to_path_buf();
    store::append(sdir, move |_, seq| {
        // The file name carries the card id — it is `studio_new_done`'s
        // marker, the "which card made me" answer a restart reads.
        let dest = format!("messages/{seq}-brief-c{card_seq}.md");
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
