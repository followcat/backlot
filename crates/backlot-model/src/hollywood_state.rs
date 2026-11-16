//! Hollywood's reduced state: the cards with their place in the
//! gate's life, the capped last headlines and the messages —
//! [`apply`] folds one event in, [`replay`] folds the log.
//! Permissive like the studio's reducer: whatever the ledger holds,
//! replay reproduces it.

use std::collections::BTreeMap;

use crate::hollywood::{CardAction, CardBase, HEADLINE_CAP, HollywoodEvent, HollywoodKind};

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
                },
            );
        }
        HollywoodKind::CardApplied { card } => close(state, *card, CardState::Applied, "", e.seq),
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
        } => state.messages.push(HMessage {
            seq: e.seq,
            ts_ms: e.ts_ms,
            from: e.by.role.clone(),
            to: to.clone(),
            studio: studio.clone(),
            text: text.clone(),
            file: file.clone(),
        }),
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
