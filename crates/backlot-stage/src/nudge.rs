//! Nudges (S9a): the pass that turns an owed step into a message.
//! [`next_steps`] says what the ledger still owes and who owes it;
//! this pass watches the owner's idleness — ledger-live, terminal
//! up, latest work sample `waiting`, nothing undelivered queued for
//! it and no send in flight — and once the idle stretch and the
//! step's own age both pass `nudge_after_s` queues one `nudge`
//! message, ever, with the step's key as `source_seq`. Still owed
//! and still idle `escalate_after_s` after that nudge was
//! delivered, the step raises one `owed` attention on the owner —
//! cleared once no owed step of the owner still carries a delivered
//! nudge (the escalated step is done, whatever it owes next) or the
//! owner is no longer wanted. A step the user owes is shown, never
//! pushed.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use backlot_model::event::EventKind;
use backlot_model::state::{StudioState, actor_task};
use backlot_model::steps::{Step, next_steps};
use backlot_model::summary::Work;

use crate::act::record;
use crate::cli::Fail;
use crate::daemon::Stage;
use crate::reasons;
use crate::reconcile::{self, LiveTerminal};

/// What the pass decided. Files are written before their
/// `message.queued` lands — the same crash order the notices keep.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Move {
    /// Write `messages/<key>-nudge.md`, then queue the nudge.
    Nudge { step: Step },
    /// Write the firmer `messages/<key>-again.md`, then queue it —
    /// the actor's delivery step's second nudge, for the same key.
    NudgeAgain { step: Step },
    /// Raise one `owed` attention on the still-idle owner.
    Raise { role: String, detail: String },
    /// Clear an open attention that no longer stands, with the
    /// `attention.cleared` reason.
    Clear { seq: u64, reason: &'static str },
}

/// An undelivered message or report aimed at the role — the idle
/// rule's "nothing queued for it". A dropped message is done, not
/// pending: it stays out.
pub(crate) fn queued_for(state: &StudioState, role: &str) -> bool {
    state
        .messages
        .iter()
        .any(|m| m.to == role && !m.delivered && !m.dropped)
        || state.reports.iter().any(|r| r.to == role && !r.delivered)
}

/// The step's latest nudge message — any queued, whatever became of
/// it. The latest, not the first, so an actor's second nudge restarts
/// the escalation clock from its own delivery.
fn nudge_for(state: &StudioState, key: u64) -> Option<&backlot_model::state::Message> {
    state
        .messages
        .iter()
        .rev()
        .find(|m| m.source_seq == Some(key) && m.purpose.as_deref() == Some("nudge"))
}

/// `attention.cleared` reason: the owed step's owner wrote to the
/// ledger after the escalation.
pub(crate) const ROLE_ACTIVE: &str = "role active again";
/// `attention.cleared` reason: the refused role has since been handed
/// new work.
pub(crate) const ROLE_MOVED_ON: &str = "role moved on";

/// Was a message or report delivered to `role` after `ts_ms`?
fn delivered_since(state: &StudioState, role: &str, ts_ms: u64) -> bool {
    let after = |d: Option<u64>| d.is_some_and(|d| d > ts_ms);
    state
        .messages
        .iter()
        .any(|m| m.to == role && m.delivered && after(m.delivered_ms))
        || state
            .reports
            .iter()
            .any(|r| r.to == role && r.delivered && after(r.delivered_ms))
}

/// The delivery step of a running task — the one obligation an actor
/// can sit at its prompt and never discharge. `None` for every other
/// step, which keeps today's body and one-nudge rule.
fn delivery_task(step: &Step) -> Option<&str> {
    let task = actor_task(&step.owner)?;
    step.command
        .starts_with(&format!("backlot deliver --task {task} "))
        .then_some(task)
}

/// The body the nudge file carries: what is owed, the command that
/// discharges it, and where a question or a wait goes. An actor's
/// delivery step gets the directive form — name the task, keep working
/// until `backlot deliver`, do not end the turn on a progress line,
/// and a block goes to the task's reviewer.
pub(crate) fn nudge_body(state: &StudioState, step: &Step) -> String {
    if let Some(task) = delivery_task(step) {
        let reviewer = reviewer_of(state, task);
        return format!(
            "# {label} is owed\n\nTask `{task}` is running and its delivery is yours:\n\n    {command}\n\nKeep working until that command lands; do not end the turn with a `backlot progress` line. If something blocks you, say why with `backlot tell {reviewer} --file <file>` — the stage reads the ledger, so a message there reaches whoever can unblock you.\n",
            label = step.label,
            task = task,
            command = step.command,
        );
    }
    format!(
        "# {label} is owed\n\nThe ledger still owes {label} — the step is yours:\n\n    {command}\n\nA question or a wait on something the ledger cannot see goes through `backlot tell <role> --file <file>` — the stage reads the ledger, so a message there reaches whoever can unblock you.\n",
        label = step.label,
        command = step.command,
    )
}

/// Who a blocked actor tells: the task's reviewer, else the producer.
fn reviewer_of<'a>(state: &'a StudioState, task: &str) -> &'a str {
    state
        .tasks
        .get(task)
        .map(|t| t.reviewer())
        .unwrap_or("producer")
}

/// The second nudge body: the same step, said as a directive — keep
/// working until the delivery lands, do not end the turn on a
/// progress line, and a block goes to the task's reviewer by name.
pub(crate) fn nudge_body_second(state: &StudioState, step: &Step) -> String {
    let reviewer = delivery_task(step).map_or("producer", |task| reviewer_of(state, task));
    format!(
        "# {label} is still owed\n\nYou answered the last nudge and went back to your prompt, but the ledger still owes {label} — the step is yours:\n\n    {command}\n\nKeep working until that command lands; do not end the turn with a `backlot progress` line. If something blocks you, say why with `backlot tell {reviewer} --file <file>` — the stage reads the ledger, so a message there reaches whoever can unblock you.\n",
        label = step.label,
        command = step.command,
    )
}

/// Whether the actor's delivery step already earned its second nudge:
/// two `nudge` messages share the step's `source_seq`.
fn second_sent(state: &StudioState, key: u64) -> bool {
    state
        .messages
        .iter()
        .filter(|m| m.source_seq == Some(key) && m.purpose.as_deref() == Some("nudge"))
        .count()
        > 1
}

/// The ledger activity that counts for `role`: a bare `role.progress`
/// never does when `ignore_progress` — an actor's delivery step — so
/// a progress-only reply cannot postpone the escalation.
fn act_ts(state: &StudioState, role: &str, ignore_progress: bool) -> u64 {
    let map = if ignore_progress {
        &state.last_act_by_role
    } else {
        &state.last_by_role
    };
    map.get(role).map_or(0, |last| last.ts_ms)
}

/// Did `role` record any ledger event after `seq` that counts — the
/// same `ignore_progress` rule as [`act_ts`]?
fn acted_since(state: &StudioState, role: &str, seq: u64, ignore_progress: bool) -> bool {
    let map = if ignore_progress {
        &state.last_act_by_role
    } else {
        &state.last_by_role
    };
    map.get(role).is_some_and(|last| last.seq > seq)
}

/// The moves the state calls for. `idle` answers the role's current
/// idle-stretch start — `None` while it is not idle — and `moved` the
/// last time its work-area probe saw a change, so the caller owns the
/// stretch and probe bookkeeping and this stays pure. At most one
/// nudge per owner per plan: a second owed step waits for the first's
/// delivery, which makes the owner not idle anyway.
pub fn plan(
    state: &StudioState,
    wanted: &BTreeSet<String>,
    idle: impl Fn(&str) -> Option<u64>,
    moved: impl Fn(&str) -> Option<u64>,
    now_ms: u64,
) -> Vec<Move> {
    let steps = next_steps(state);
    let nudge_ms = u64::from(state.nudge_after_s()) * 1000;
    let escalate_ms = u64::from(state.escalate_after_s()) * 1000;
    let mut out = Vec::new();
    let mut nudged_now: BTreeSet<String> = BTreeSet::new();
    for s in &steps {
        if s.owner == "user" {
            continue;
        }
        // An actor's delivery step ignores a bare `role.progress` and
        // reads the work-area probe instead: a progress-only reply or
        // a `git status` with nothing touched is not activity there, so
        // neither postpones the second nudge or the escalation.
        let ignore_progress = delivery_task(s).is_some();
        // For the delivery step, the last thing that counted as work:
        // any non-progress ledger write, else the work area's last move.
        let worked_ms = |role: &str| {
            if ignore_progress {
                act_ts(state, role, true).max(moved(role).unwrap_or(0))
            } else {
                act_ts(state, role, false)
            }
        };
        match nudge_for(state, s.key) {
            Some(m) if m.delivered => {
                let delivered = m.delivered_ms.unwrap_or(m.ts_ms);
                // An actor's delivery step earns one firmer nudge: the
                // first was delivered, nothing but progress landed since
                // — no deliver, no commit, the work area unchanged — and
                // it sat idle past `nudge_after_s` again. Real work
                // since the nudge cancels the second one outright.
                let worked_since = ignore_progress && worked_ms(&s.owner) >= delivered;
                if ignore_progress
                    && !worked_since
                    && !second_sent(state, s.key)
                    && idle(&s.owner).is_some_and(|stretch| {
                        now_ms.saturating_sub(delivered.max(stretch)) >= nudge_ms
                    })
                {
                    out.push(Move::NudgeAgain { step: s.clone() });
                    continue;
                }
                // The nudge landed `escalate_after_s` ago, and the
                // owner has done nothing that counts for as long —
                // still owed, still idle: the user's step now. For a
                // delivery step the clock reads the ledger and the
                // work-area probe, never a bare progress write.
                let quiet_since = delivered.max(worked_ms(&s.owner));
                if now_ms.saturating_sub(quiet_since) >= escalate_ms
                    && idle(&s.owner).is_some()
                    && !state
                        .open_attention()
                        .any(|a| a.role == s.owner && a.reason == reasons::OWED)
                {
                    out.push(Move::Raise {
                        role: s.owner.clone(),
                        detail: format!("{} still owed: {}", s.label, s.command),
                    });
                }
            }
            // Queued but not delivered — its clock starts on delivery.
            Some(_) => {}
            None => {
                if nudged_now.contains(&s.owner) {
                    continue;
                }
                let Some(stretch) = idle(&s.owner) else {
                    continue;
                };
                if now_ms.saturating_sub(s.since_ms.max(stretch)) >= nudge_ms {
                    out.push(Move::Nudge { step: s.clone() });
                    nudged_now.insert(s.owner.clone());
                }
            }
        }
    }
    // An `owed` attention stands only while its role is wanted and
    // some owed step still carries a delivered nudge — the step it
    // escalated. A newer step, not yet nudged, does not hold it up.
    // An owner that wrote to the ledger after the raise is working
    // again: the attention clears unless the owner has since sat idle
    // a whole escalation window, when it stands as the re-escalation.
    for a in state.open_attention().filter(|a| a.reason == reasons::OWED) {
        // The escalated step decides whether progress counts here: for
        // an actor's delivery step it does not.
        let ignore_progress = steps.iter().any(|s| {
            s.owner == a.role
                && nudge_for(state, s.key).is_some_and(|m| m.delivered)
                && delivery_task(s).is_some()
        });
        let nudged = steps
            .iter()
            .any(|s| s.owner == a.role && nudge_for(state, s.key).is_some_and(|m| m.delivered));
        // A work-area move after the raise is work too, wherever the
        // ledger is silent.
        let moved_after = ignore_progress && moved(&a.role).is_some_and(|t| t > a.ts_ms);
        if !wanted.contains(&a.role) || !nudged {
            out.push(Move::Clear {
                seq: a.seq,
                reason: "done",
            });
        } else if (acted_since(state, &a.role, a.seq, ignore_progress) || moved_after)
            && (idle(&a.role).is_none()
                || now_ms.saturating_sub(act_ts(state, &a.role, ignore_progress)) < escalate_ms)
        {
            out.push(Move::Clear {
                seq: a.seq,
                reason: ROLE_ACTIVE,
            });
        }
    }
    // An `outward action` refusal is the role's own record of what the
    // guard stopped; the role was told to ask if it still needs the
    // command. It clears as `role moved on` once the role writes to the
    // ledger again after it — its turn's next `progress`, a `tell`, a
    // publish, or the `role.refused` of a later attempt — and also once
    // the stage hands it new work, since deliveries only reach an idle
    // role and the refused turn is over by then.
    for a in state
        .open_attention()
        .filter(|a| a.reason == backlot_model::event::OUTWARD_ACTION)
    {
        if acted_since(state, &a.role, a.seq, false) || delivered_since(state, &a.role, a.ts_ms) {
            out.push(Move::Clear {
                seq: a.seq,
                reason: ROLE_MOVED_ON,
            });
        }
    }
    out
}

impl Stage {
    /// The nudge pass ahead of the outbox: refresh every recorded
    /// role's idle stretch, then carry out the moves `plan` calls
    /// for. The stretch map is in-memory — a restart starts every
    /// role's idle over, while the nudge's ledger `source_seq` keeps
    /// the once-ever rule across it.
    pub(crate) fn nudge(
        &self,
        studio: &str,
        dir: &Path,
        state: &StudioState,
        live: &[LiveTerminal],
        now_ms: u64,
    ) -> Result<(), Fail> {
        let mut idle: BTreeMap<String, u64> = BTreeMap::new();
        for (key, role) in &state.roles {
            let pair = (studio.to_string(), key.clone());
            let terminal_live = reconcile::terminal_name(studio, key)
                .is_some_and(|name| live.iter().any(|t| t.name == name));
            let is_idle = role.live
                && terminal_live
                && self.work.borrow().get(&pair).copied() == Some(Work::Waiting)
                && !self.sending.borrow().contains_key(&pair)
                && !queued_for(state, key);
            let mut map = self.idle_since.borrow_mut();
            if is_idle {
                idle.insert(key.clone(), *map.entry(pair).or_insert(now_ms));
            } else {
                map.remove(&pair);
            }
        }
        let wanted = reconcile::wanted(studio, state);
        let moved = |role: &str| {
            self.liveness
                .borrow()
                .get(&(studio.to_string(), role.to_string()))
                .and_then(|ev| ev.work_area_moved_ms())
        };
        for mv in plan(state, &wanted, |r| idle.get(r).copied(), moved, now_ms) {
            match mv {
                Move::Nudge { step } => {
                    let dest = format!("messages/{}-nudge.md", step.key);
                    let path = dir.join(&dest);
                    if let Some(parent) = path.parent() {
                        std::fs::create_dir_all(parent)
                            .map_err(|e| Fail::refused(format!("nudge file: {e}")))?;
                    }
                    std::fs::write(&path, nudge_body(state, &step))
                        .map_err(|e| Fail::refused(format!("nudge file: {e}")))?;
                    record(
                        dir,
                        EventKind::MessageQueued {
                            to: step.owner.clone(),
                            file: Some(dest),
                            purpose: Some("nudge".into()),
                            source_seq: Some(step.key),
                        },
                    )?;
                    eprintln!(
                        "stage: nudge {} ({}) queued for {}",
                        step.key, step.label, step.owner
                    );
                }
                Move::NudgeAgain { step } => {
                    let dest = format!("messages/{}-again.md", step.key);
                    let path = dir.join(&dest);
                    if let Some(parent) = path.parent() {
                        std::fs::create_dir_all(parent)
                            .map_err(|e| Fail::refused(format!("nudge file: {e}")))?;
                    }
                    std::fs::write(&path, nudge_body_second(state, &step))
                        .map_err(|e| Fail::refused(format!("nudge file: {e}")))?;
                    record(
                        dir,
                        EventKind::MessageQueued {
                            to: step.owner.clone(),
                            file: Some(dest),
                            purpose: Some("nudge".into()),
                            source_seq: Some(step.key),
                        },
                    )?;
                    eprintln!(
                        "stage: second nudge {} ({}) queued for {}",
                        step.key, step.label, step.owner
                    );
                }
                Move::Raise { role, detail } => {
                    record(
                        dir,
                        EventKind::AttentionRaised {
                            role: role.clone(),
                            reason: reasons::OWED.into(),
                            detail,
                            needs_user: false,
                            account: None,
                            reset_at_ms: None,
                        },
                    )?;
                    crate::stagelog::stage_log!("{role} still owes a step — escalated");
                }
                Move::Clear { seq, reason } => {
                    record(
                        dir,
                        EventKind::AttentionCleared {
                            target: seq,
                            reason: Some(reason.into()),
                        },
                    )?;
                }
            }
        }
        Ok(())
    }
}
