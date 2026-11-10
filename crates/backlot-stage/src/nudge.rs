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
use backlot_model::state::StudioState;
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
    /// Raise one `owed` attention on the still-idle owner.
    Raise { role: String, detail: String },
    /// Clear an open `owed` attention that no longer stands.
    Clear { seq: u64 },
}

/// An undelivered message or report aimed at the role — the idle
/// rule's "nothing queued for it".
fn queued_for(state: &StudioState, role: &str) -> bool {
    state.messages.iter().any(|m| m.to == role && !m.delivered)
        || state.reports.iter().any(|r| r.to == role && !r.delivered)
}

/// The nudge's own message — any queued, whatever became of it.
fn nudge_for(state: &StudioState, key: u64) -> Option<&backlot_model::state::Message> {
    state
        .messages
        .iter()
        .find(|m| m.source_seq == Some(key) && m.purpose.as_deref() == Some("nudge"))
}

/// The body the nudge file carries: what is owed, the command that
/// discharges it, and where a question or a wait goes.
pub(crate) fn nudge_body(step: &Step) -> String {
    format!(
        "# {label} is owed\n\nThe ledger still owes {label} — the step is yours:\n\n    {command}\n\nA question or a wait on something the ledger cannot see goes through `backlot tell <role> --file <file>` — the stage reads the ledger, so a message there reaches whoever can unblock you.\n",
        label = step.label,
        command = step.command,
    )
}

/// The moves the state calls for. `idle` answers the role's current
/// idle-stretch start — `None` while it is not idle — so the caller
/// owns the stretch bookkeeping and this stays pure. At most one
/// nudge per owner per plan: a second owed step waits for the
/// first's delivery, which makes the owner not idle anyway.
pub fn plan(
    state: &StudioState,
    wanted: &BTreeSet<String>,
    idle: impl Fn(&str) -> Option<u64>,
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
        match nudge_for(state, s.key) {
            // The nudge landed `escalate_after_s` ago and nothing
            // answered — still owed, still idle: the user's step now.
            Some(m) if m.delivered => {
                let delivered = m.delivered_ms.unwrap_or(m.ts_ms);
                if now_ms.saturating_sub(delivered) >= escalate_ms
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
    for a in state.open_attention().filter(|a| a.reason == reasons::OWED) {
        let nudged = steps
            .iter()
            .any(|s| s.owner == a.role && nudge_for(state, s.key).is_some_and(|m| m.delivered));
        if !wanted.contains(&a.role) || !nudged {
            out.push(Move::Clear { seq: a.seq });
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
        for mv in plan(state, &wanted, |r| idle.get(r).copied(), now_ms) {
            match mv {
                Move::Nudge { step } => {
                    let dest = format!("messages/{}-nudge.md", step.key);
                    let path = dir.join(&dest);
                    if let Some(parent) = path.parent() {
                        std::fs::create_dir_all(parent)
                            .map_err(|e| Fail::refused(format!("nudge file: {e}")))?;
                    }
                    std::fs::write(&path, nudge_body(&step))
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
                Move::Raise { role, detail } => {
                    record(
                        dir,
                        EventKind::AttentionRaised {
                            role: role.clone(),
                            reason: reasons::OWED.into(),
                            detail,
                            needs_user: false,
                        },
                    )?;
                    eprintln!("stage: {role} still owes a step — escalated");
                }
                Move::Clear { seq } => {
                    record(
                        dir,
                        EventKind::AttentionCleared {
                            target: seq,
                            reason: Some("done".into()),
                        },
                    )?;
                }
            }
        }
        Ok(())
    }
}
