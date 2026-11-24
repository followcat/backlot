//! Hollywood's reduced state: the cards with their place in the
//! gate's life, the capped last headlines and the messages —
//! [`apply`] folds one event in, [`replay`] folds the log.
//! Permissive like the studio's reducer: whatever the ledger holds,
//! replay reproduces it.

use std::collections::BTreeMap;

use crate::event::{ActorIdentity, ApprovalDecision};
use crate::hollywood::{
    CardAction, CardBase, HEADLINE_CAP, HollywoodEvent, HollywoodKind, RoleMotion,
};
use crate::state::{RestartRequest, RoleLifecycle};

/// A card's point in its life.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CardState {
    Open,
    Applied,
    Cancelled,
    Done,
    Failed,
    Stale,
}

impl CardState {
    pub fn as_str(self) -> &'static str {
        match self {
            CardState::Open => "open",
            CardState::Applied => "applied",
            CardState::Cancelled => "cancelled",
            CardState::Done => "done",
            CardState::Failed => "failed",
            CardState::Stale => "stale",
        }
    }
}

/// A proposed change and where it stands.
#[derive(Debug, Clone, PartialEq)]
pub struct Card {
    /// The `card.proposed` seq — the card's `c<seq>` id.
    pub seq: u64,
    pub action: CardAction,
    /// Hollywood's one-line why.
    pub why: String,
    /// What must still hold for the apply to proceed.
    pub base: CardBase,
    pub by: String,
    pub ts_ms: u64,
    pub state: CardState,
    /// The closing event's reason or note (`card.failed`'s is required).
    pub detail: String,
    /// The seq that closed the card, if it did.
    pub closed_seq: Option<u64>,
    /// The `card.applied` seq — the applied notice's dedup key. The
    /// card's close keeps `closed_seq` instead of overwriting it.
    pub applied_seq: Option<u64>,
}

/// One `hollywood.reported` in the capped last-headlines list.
#[derive(Debug, Clone, PartialEq)]
pub struct HReport {
    pub seq: u64,
    pub ts_ms: u64,
    pub headline: String,
    pub body: Option<String>,
}

/// One `message.sent` to or from Hollywood.
#[derive(Debug, Clone, PartialEq)]
pub struct HMessage {
    pub seq: u64,
    pub ts_ms: u64,
    pub from: String,
    pub to: String,
    pub studio: Option<String>,
    pub text: Option<String>,
    pub file: Option<String>,
    /// For a stage notice, the source event's seq — in `studio`'s
    /// ledger when set, in Hollywood's own when absent. The dedup
    /// key that keeps a restart from telling it twice.
    pub source_seq: Option<u64>,
    pub delivered: bool,
    /// `message.dropped` landed: a queued notice skipped at send
    /// time because its source no longer stood.
    pub dropped: bool,
}

/// Hollywood's own terminal record, folded from `role.state`
/// events — the same lifecycle a studio's `RoleState` tracks.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct HWRole {
    /// True while the role should have a live terminal.
    pub live: bool,
    /// The stage closed it on purpose — it stays down until a start
    /// records it live again.
    pub closed: bool,
    /// The `closed` record's reason — `"cannot resume"` marks the
    /// crash guard's stop, which no config return revives.
    pub closed_reason: Option<String>,
    /// When the current lifecycle state began (the `role.state` ts).
    pub since_ms: u64,
    pub terminal: Option<String>,
    pub actor: Option<ActorIdentity>,
    pub session_id: Option<String>,
    /// Every `lost` timestamp — the resume guard's crash-loop
    /// history, kept so a stage restart does not lose the count.
    pub losses: Vec<u64>,
    /// The seq of a `resumed` whose reason was `recovered`, while
    /// its notice is owed — a start or plain resume clears it.
    pub recovered_seq: Option<u64>,
    /// A `role.restart` the stage still owes: stop the terminal and
    /// launch again (resumed unless `fresh`). Cleared by the close it
    /// records.
    pub restart: Option<RestartRequest>,
    /// The newest time the stage restarted `bl-hollywood` in place —
    /// the `restarted` word the bar shows.
    pub restarted_ms: Option<u64>,
}

impl HWRole {
    /// Live, lost or closed — never more than one.
    pub fn lifecycle(&self) -> RoleLifecycle {
        if self.live {
            RoleLifecycle::Live
        } else if self.closed {
            RoleLifecycle::Closed
        } else {
            RoleLifecycle::Lost
        }
    }

    /// Anything a resume could use — a recorded actor, terminal or
    /// loss. A bare struct means Hollywood never started.
    pub fn recorded(&self) -> bool {
        self.actor.is_some() || self.terminal.is_some() || !self.losses.is_empty()
    }
}

/// The user's answer to a permission prompt on Hollywood's screen.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HApproval {
    pub seq: u64,
    pub ts_ms: u64,
    pub decision: ApprovalDecision,
}

/// The reduced view of Hollywood's ledger.
#[derive(Debug, Default, PartialEq)]
pub struct HollywoodState {
    /// Every card by id (`card.proposed` seq), open and closed.
    pub cards: BTreeMap<u64, Card>,
    /// The last [`HEADLINE_CAP`] headlines.
    pub reports: Vec<HReport>,
    /// Every `message.sent`, in seq order.
    pub messages: Vec<HMessage>,
    /// Every `role.approval`, in seq order — the user's answers to
    /// Hollywood's own prompts.
    pub approvals: Vec<HApproval>,
    /// Hollywood's own terminal record — nothing before the first
    /// `role.state`.
    pub role: HWRole,
    /// Hollywood's latest `role.progress` note.
    pub progress: Option<crate::progress::RoleProgress>,
    /// Last applied seq — torn-tail replay keeps the maximum.
    pub last_seq: u64,
}

impl HollywoodState {
    /// Open cards in proposal order.
    pub fn open_cards(&self) -> impl Iterator<Item = &Card> {
        self.cards.values().filter(|c| c.state == CardState::Open)
    }

    /// The newest headline, if any.
    pub fn last_headline(&self) -> Option<&str> {
        self.reports.last().map(|r| r.headline.as_str())
    }

    /// The newest approval recorded after `seq` — the answer to a
    /// prompt that appeared when the ledger was at `seq`.
    pub fn approval_after(&self, seq: u64) -> Option<&HApproval> {
        self.approvals.iter().rev().find(|a| a.seq > seq)
    }
}

/// Fold one stamped event into the state — permissive: whatever the
/// log holds, replay reproduces it.
pub fn apply(state: &mut HollywoodState, e: &HollywoodEvent) {
    state.last_seq = state.last_seq.max(e.seq);
    match &e.kind {
        HollywoodKind::CardProposed { action, why, base } => {
            state.cards.insert(
                e.seq,
                Card {
                    seq: e.seq,
                    action: action.clone(),
                    why: why.clone(),
                    base: base.clone(),
                    by: e.by.role.clone(),
                    ts_ms: e.ts_ms,
                    state: CardState::Open,
                    detail: String::new(),
                    closed_seq: None,
                    applied_seq: None,
                },
            );
        }
        HollywoodKind::CardApplied { card } => {
            close(state, *card, CardState::Applied, "", e.seq);
            if let Some(c) = state.cards.get_mut(card) {
                c.applied_seq = Some(e.seq);
            }
        }
        HollywoodKind::CardCancelled { card, reason } => {
            close(state, *card, CardState::Cancelled, reason, e.seq)
        }
        HollywoodKind::CardDone { card, note } => close(state, *card, CardState::Done, note, e.seq),
        HollywoodKind::CardFailed { card, reason } => {
            close(state, *card, CardState::Failed, reason, e.seq)
        }
        HollywoodKind::CardStale { card, reason } => {
            close(state, *card, CardState::Stale, reason, e.seq)
        }
        HollywoodKind::Reported { headline, body } => {
            state.reports.push(HReport {
                seq: e.seq,
                ts_ms: e.ts_ms,
                headline: headline.clone(),
                body: body.clone(),
            });
            if state.reports.len() > HEADLINE_CAP {
                state.reports.drain(..state.reports.len() - HEADLINE_CAP);
            }
        }
        HollywoodKind::MessageSent {
            to,
            studio,
            text,
            file,
            source_seq,
        } => state.messages.push(HMessage {
            seq: e.seq,
            ts_ms: e.ts_ms,
            from: e.by.role.clone(),
            to: to.clone(),
            studio: studio.clone(),
            text: text.clone(),
            file: file.clone(),
            source_seq: *source_seq,
            delivered: false,
            dropped: false,
        }),
        HollywoodKind::RoleState {
            state: motion,
            terminal,
            actor,
            session_id,
            reason,
        } => match motion {
            // A start begins a fresh conversation: the session id it
            // carries is the whole truth, so a start without one —
            // a new runner's first start, or an adoption — clears the
            // id the previous runner recorded. Otherwise a resume
            // would hand that id to a runner that never minted it.
            RoleMotion::Started => {
                let r = &mut state.role;
                r.live = true;
                r.closed = false;
                r.closed_reason = None;
                r.since_ms = e.ts_ms;
                if terminal.is_some() {
                    r.terminal = terminal.clone();
                }
                if actor.is_some() {
                    r.actor = actor.clone();
                }
                r.session_id = session_id.clone();
                r.recovered_seq = None;
                // A fresh restart's start — or any other start — is
                // the moment the pending request is done.
                r.restart = None;
                r.restarted_ms =
                    (reason.as_deref() == Some(crate::event::RESTARTED)).then_some(e.ts_ms);
            }
            // A resume continues the recorded conversation: keep the
            // old id when the record carries none.
            RoleMotion::Resumed => {
                let r = &mut state.role;
                r.live = true;
                r.closed = false;
                r.closed_reason = None;
                r.since_ms = e.ts_ms;
                if terminal.is_some() {
                    r.terminal = terminal.clone();
                }
                if actor.is_some() {
                    r.actor = actor.clone();
                }
                if session_id.is_some() {
                    r.session_id = session_id.clone();
                }
                r.recovered_seq = (reason.as_deref() == Some("recovered")).then_some(e.seq);
                // The start a restart asked for is done.
                r.restart = None;
                r.restarted_ms =
                    (reason.as_deref() == Some(crate::event::RESTARTED)).then_some(e.ts_ms);
            }
            RoleMotion::Lost => {
                state.role.live = false;
                state.role.closed = false;
                state.role.losses.push(e.ts_ms);
            }
            RoleMotion::Closed => {
                let r = &mut state.role;
                r.live = false;
                r.closed = true;
                r.closed_reason = reason.clone();
                // A close the restart asked for keeps the request and
                // the session — the launch that follows resumes it.
                // Any other close ends both.
                if reason.as_deref() != Some(crate::event::RESTARTED) {
                    r.restart = None;
                    // A close ends the conversation: no later resume
                    // may hand this id to whichever runner starts next.
                    r.session_id = None;
                }
                // The close's own ts: a stage process born after it
                // may revive a `cannot resume` give-up (R1-4), and the
                // crash history ends with the lifecycle it counted.
                r.since_ms = e.ts_ms;
                r.losses.clear();
            }
        },
        HollywoodKind::RoleApproval { decision } => state.approvals.push(HApproval {
            seq: e.seq,
            ts_ms: e.ts_ms,
            decision: *decision,
        }),
        HollywoodKind::MessageDelivered { target } => {
            if let Some(m) = state.messages.iter_mut().find(|m| m.seq == *target) {
                m.delivered = true;
            }
        }
        HollywoodKind::MessageDropped { target, .. } => {
            if let Some(m) = state.messages.iter_mut().find(|m| m.seq == *target) {
                m.dropped = true;
            }
        }
        HollywoodKind::RoleRestart { fresh, reason } => {
            state.role.restart = Some(RestartRequest {
                seq: e.seq,
                fresh: *fresh,
                reason: reason.clone(),
            });
        }
        HollywoodKind::RoleProgress { text } => {
            state.progress = Some(crate::progress::RoleProgress {
                text: text.clone(),
                ts_ms: e.ts_ms,
            });
        }
    }
}

fn close(state: &mut HollywoodState, card: u64, to: CardState, detail: &str, seq: u64) {
    if let Some(c) = state.cards.get_mut(&card) {
        c.state = to;
        c.detail = detail.to_string();
        c.closed_seq = Some(seq);
    }
}

/// The whole log folded — equals applying the events one by one.
pub fn replay(events: &[HollywoodEvent]) -> HollywoodState {
    let mut state = HollywoodState::default();
    for e in events {
        apply(&mut state, e);
    }
    state
}
