//! The summary document's `hollywood` section (R27, D20): what the
//! floating layer draws — the role's state and runner, its newest
//! headline and its open cards — each field capped so the document
//! stays under its target at the limits.

use serde::{Deserialize, Serialize};

use crate::hollywood_state::HollywoodState;
use crate::summary::cap;

/// The headline's character cap in the document.
pub const HEADLINE_CHARS: usize = 200;
/// At most this many open cards reach the document.
pub const MAX_OPEN_CARDS: usize = 10;
/// At most this many exchange lines reach the document — the
/// conversation the expanded layer draws above its input (D21).
pub const MAX_EXCHANGE: usize = 8;

/// Hollywood's section of the document: the layer reads the role's
/// state and runner, its newest headline and its open cards. `None`
/// while no `[hollywood]` is configured — a document written before
/// the field existed still parses.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HollywoodSummary {
    /// `down` while only configured, `live`, `lost` — the stage's
    /// read of the layer (H3), not a ledger state.
    pub state: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runner: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effort: Option<String>,
    /// The newest `hollywood.reported` headline (≤ [`HEADLINE_CHARS`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub headline: Option<String>,
    /// Up to [`MAX_OPEN_CARDS`] open cards, each string capped.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub cards: Vec<CardSummary>,
    /// The newest [`MAX_EXCHANGE`] exchange lines — the user's
    /// `tell`s and Hollywood's `reported` headlines, oldest first.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub exchange: Vec<Exchange>,
}

/// One line of the user's exchange with Hollywood: the user half is
/// a `message.sent` `to` it (`who` is the sender), the Hollywood half
/// a `reported` headline (`who` is `hollywood`). `seq` orders the
/// halves into one conversation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Exchange {
    /// The ledger seq — orders messages and reports on one timeline.
    pub seq: u64,
    /// The sender: `user`, `hollywood`, or a stage-written notice's `by`.
    pub who: String,
    /// The line's text (≤ [`HEADLINE_CHARS`]).
    pub text: String,
}

/// One open card as the layer shows it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CardSummary {
    /// The card's `c<seq>` id.
    pub id: String,
    /// The studio the action touches.
    pub studio: String,
    /// The one-line action (`pause t1`).
    pub action: String,
    /// Hollywood's one-line why.
    pub why: String,
}

/// The optional `hollywood` object: `state` is the stage's read of
/// the layer and `runner`/`model`/`effort` come from `[hollywood]`
/// config; the headline (≤200 chars) and at most [`MAX_OPEN_CARDS`]
/// open cards come from Hollywood's ledger — each capped so the
/// document stays under its target at the limits.
pub fn hollywood_section(
    state: &HollywoodState,
    role_state: &str,
    runner: Option<&str>,
    model: Option<&str>,
    effort: Option<&str>,
) -> HollywoodSummary {
    HollywoodSummary {
        state: cap(role_state),
        runner: runner.map(cap),
        model: model.map(cap),
        effort: effort.map(cap),
        headline: state
            .last_headline()
            .map(|h| h.chars().take(HEADLINE_CHARS).collect()),
        cards: state
            .open_cards()
            .take(MAX_OPEN_CARDS)
            .map(|c| CardSummary {
                id: format!("c{}", c.seq),
                studio: cap(c.action.studio()),
                action: cap(&c.action.label()),
                why: cap(&c.why),
            })
            .collect(),
        exchange: exchange(state),
    }
}

/// The conversation lines: each `message.sent` `to` Hollywood that
/// carries text and each `reported` headline, merged on their seq
/// and cut to the newest [`MAX_EXCHANGE`].
fn exchange(state: &HollywoodState) -> Vec<Exchange> {
    let mut lines: Vec<Exchange> = state
        .messages
        .iter()
        .filter(|m| m.to == "hollywood")
        .filter_map(|m| {
            m.text.as_ref().map(|text| Exchange {
                seq: m.seq,
                who: cap(&m.from),
                text: cap(text),
            })
        })
        .chain(state.reports.iter().map(|r| Exchange {
            seq: r.seq,
            who: "hollywood".into(),
            text: cap(&r.headline),
        }))
        .collect();
    lines.sort_by_key(|e| e.seq);
    lines.split_off(lines.len().saturating_sub(MAX_EXCHANGE))
}
