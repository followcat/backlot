//! Carrying applied cards out (Part B, D17-D19): the stage checks
//! the card's base against the studio again — a moved base records
//! `card.stale` and nothing else — then records the action in the
//! studio's ledger exactly as the user's own command would, `by:
//! user` through the same validation. Success is `card.done`, a
//! refusal `card.failed` with the reason.
//!
//! Never twice: every action is idempotent. Before the base check
//! each action asks whether the studio already carries this card's
//! effect — a `task.state`/`role.*` marker, the priority it wanted,
//! or the `studio-new` brief message — and a restart that finds it
//! records `card.done` without re-appending. `studio-new`'s own
//! machinery is `hcard_new.rs`.

use std::path::Path;

use backlot_model::event::{EventKind, TaskStateChange};
use backlot_model::hollywood::{self, Card, CardAction, CardState, HollywoodKind};
use backlot_model::state::StudioState;

use crate::cli::Fail;
use crate::daemon::Stage;
use crate::hcard_new::studio_new_done;
use crate::store::{self, PendingFile};

/// The `task.state` reason a card's change carries — also the
/// restart's "already done" marker.
fn marker(card: u64, why: &str) -> String {
    format!("card c{card}: {why}")
}

/// What one applied card does next.
pub(crate) enum Step {
    /// The effect is already in the studio — record `card.done`.
    Done,
    /// The base moved — record `card.stale`.
    Stale(String),
    /// Record this kind in the studio `by: user`, then `card.done`.
    Act(EventKind),
    /// `card.failed` with the reason.
    Fail(String),
}

/// The task-state change an action maps to.
fn state_change(action: &CardAction) -> Option<TaskStateChange> {
    match action {
        CardAction::Pause { .. } => Some(TaskStateChange::Paused),
        CardAction::Resume { .. } => Some(TaskStateChange::Resumed),
        CardAction::Abandon { .. } => Some(TaskStateChange::Abandoned),
        _ => None,
    }
}

/// Did this card's effect already land — the restart check.
fn already_done(card: &Card, s: &StudioState) -> bool {
    match &card.action {
        CardAction::Pause { task, .. }
        | CardAction::Resume { task, .. }
        | CardAction::Abandon { task, .. } => {
            let Some(change) = state_change(&card.action) else {
                return false;
            };
            s.tasks.get(task).is_some_and(|t| {
                t.state_changes.iter().any(|c| {
                    // The colon is part of the tag: `card c1:` must
                    // not match card c12's marker (R1-2).
                    c.state == change && c.reason.starts_with(&format!("card c{}:", card.seq))
                })
            })
        }
        CardAction::Priority { task, level, .. } => {
            s.tasks.get(task).is_some_and(|t| t.priority == *level)
        }
        CardAction::Close { person, .. } => s.roles.get(person).is_some_and(|r| !r.held),
        CardAction::Reopen { person, .. } => s.roles.get(person).is_some_and(|r| r.held),
        CardAction::StudioNew { .. } => studio_new_done(s, card.seq),
    }
}

impl Stage {
    /// Every applied card, oldest first. Cards execute whether or
    /// not `[hollywood]` is configured — the config gates the agent,
    /// never the gate.
    pub(crate) fn hcards(&self, hdir: &Path) -> Result<(), Fail> {
        self.hrefresh(hdir);
        let applied: Vec<Card> = self
            .hstate
            .borrow()
            .cards
            .values()
            .filter(|c| c.state == CardState::Applied)
            .cloned()
            .collect();
        for card in applied {
            self.hcard(hdir, &card)?;
        }
        Ok(())
    }

    /// One applied card: already-done, stale-check, then the action
    /// — and its outcome in Hollywood's ledger.
    fn hcard(&self, hdir: &Path, card: &Card) -> Result<(), Fail> {
        let sdir = store::studio_dir(&self.base, card.action.studio());
        let sstate = hwood_state(&sdir)?;
        let step = match &card.action {
            CardAction::StudioNew { .. } => self.studio_new_step(hdir, &sdir, card, sstate),
            _ => match &sstate {
                None => Step::Fail(format!("no studio '{}'", card.action.studio())),
                Some(s) if already_done(card, s) => Step::Done,
                Some(s) => match hollywood::stale(&card.action, &card.base, Some(s)) {
                    Some(r) => Step::Stale(r),
                    None => match action_kind(card) {
                        Ok(kind) => Step::Act(kind),
                        Err(e) => Step::Fail(e),
                    },
                },
            },
        };
        match step {
            Step::Done => {
                self.happend(
                    hdir,
                    HollywoodKind::CardDone {
                        card: card.seq,
                        note: String::new(),
                    },
                )?;
                eprintln!("stage: card c{} done", card.seq);
            }
            Step::Stale(reason) => {
                self.happend(
                    hdir,
                    HollywoodKind::CardStale {
                        card: card.seq,
                        reason,
                    },
                )?;
                eprintln!("stage: card c{} stale", card.seq);
            }
            Step::Fail(reason) => {
                self.happend(
                    hdir,
                    HollywoodKind::CardFailed {
                        card: card.seq,
                        reason,
                    },
                )?;
                eprintln!("stage: card c{} failed", card.seq);
            }
            Step::Act(kind) => match append_studio(&sdir, kind) {
                Ok(()) => {
                    self.happend(
                        hdir,
                        HollywoodKind::CardDone {
                            card: card.seq,
                            note: String::new(),
                        },
                    )?;
                    eprintln!("stage: card c{} done", card.seq);
                }
                Err(e) => {
                    self.happend(
                        hdir,
                        HollywoodKind::CardFailed {
                            card: card.seq,
                            reason: e.clone(),
                        },
                    )?;
                    eprintln!("stage: card c{} failed: {e}", card.seq);
                }
            },
        }
        Ok(())
    }
}

/// The studio's reduced state, `None` without a ledger.
fn hwood_state(sdir: &Path) -> Result<Option<StudioState>, Fail> {
    if !sdir.join("events.jsonl").is_file() {
        return Ok(None);
    }
    store::load_state(sdir)
        .map(Some)
        .map_err(crate::hwood::store_fail)
}

/// The `EventKind` a card action records — `by: user` rides the
/// event envelope, not the kind.
fn action_kind(card: &Card) -> Result<EventKind, String> {
    match &card.action {
        CardAction::Close { person, .. } => Ok(EventKind::RoleRelease {
            role: person.clone(),
        }),
        CardAction::Reopen { person, .. } => Ok(EventKind::RoleReopen {
            role: person.clone(),
        }),
        CardAction::Priority { task, level, .. } => Ok(EventKind::TaskPriority {
            task: task.clone(),
            priority: *level,
        }),
        CardAction::Pause { task, .. }
        | CardAction::Resume { task, .. }
        | CardAction::Abandon { task, .. } => Ok(EventKind::TaskState {
            task: task.clone(),
            state: state_change(&card.action).expect("a task-state action"),
            reason: marker(card.seq, &card.why),
        }),
        CardAction::StudioNew { .. } => Err("studio-new is handled apart".into()),
    }
}

/// Append a `by: user` event to the studio — the same validation a
/// user's own command runs.
fn append_studio(dir: &Path, kind: EventKind) -> Result<(), String> {
    append_studio_files(dir, kind, Vec::new())
}

/// Append with carried files — `by: user` through the ledger's own
/// validation, the same boundary a user command crosses.
pub(crate) fn append_studio_files(
    dir: &Path,
    kind: EventKind,
    files: Vec<PendingFile>,
) -> Result<(), String> {
    store::append(dir, move |_, _| {
        Ok::<_, std::convert::Infallible>(store::Pending {
            event: backlot_model::NewEvent::new(backlot_model::EventBy::role("user"), kind),
            files,
        })
    })
    .map(|_| ())
    .map_err(|e| match e {
        store::StoreError::Io(e) => format!("append: {e}"),
        store::StoreError::Corrupt(c) => c.to_string(),
        store::StoreError::Refused(r) => r.to_string(),
        store::StoreError::Build(_) => "unexpected error".into(),
    })
}
