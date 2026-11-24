//! The summary document's `hollywood` section (R27, D20): what the
//! floating layer draws — the role's state and runner, its newest
//! headline and its open cards — each field capped so the document
//! stays under its target at the limits.

use serde::{Deserialize, Serialize};

use crate::event::PERMISSION_PROMPT;
use crate::hollywood_state::HollywoodState;
use crate::summary::{Work, cap, default_true, is_true, steady};

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
    /// Screen/signal-classified work state (`working`, `waiting`,
    /// `blocked`) while `state == "live"`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub work: Option<Work>,
    /// True while an undelivered, non-dropped `message.sent` (`to:
    /// hollywood`) sits in Hollywood's outbox behind the send gate.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub pending: bool,
    /// What `bl-hollywood` is doing now — the terminal's reported
    /// status, the same read a role's `doing` makes. Published only
    /// while live.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub doing: Option<String>,
    /// A permission prompt on Hollywood's screen: the reason a role's
    /// open attention carries. Only the user may answer this one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attention: Option<String>,
    /// True while `attention` is that user-owned prompt.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub attention_user: bool,
    /// The prompt's one-line detail — the command its picker asks
    /// about.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attention_detail: Option<String>,
    /// Whether Hollywood's runner can answer its own prompt — false
    /// for a runner with no approval key, where the client offers
    /// `answer in its terminal (Enter)`. Absent on documents an older
    /// stage wrote, which keeps today's `y`/`d` behaviour.
    #[serde(default = "default_true", skip_serializing_if = "is_true")]
    pub can_answer_prompt: bool,
    /// The newest `hollywood.reported` headline (≤ [`HEADLINE_CHARS`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub headline: Option<String>,
    /// When the stage last restarted `bl-hollywood` in place
    /// (`role.restart`) — the bar reads `restarted`. Documents an
    /// older stage wrote have none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub restarted_ms: Option<u64>,
    /// Ledger `seq` of the newest `hollywood.reported` event so the
    /// Call Sheet's headline row can open its full report body.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub headline_seq: Option<u64>,
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
    /// Whether this entry carries a file body on disk (`--body` on a
    /// report, or `messages/…` on a message).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub has_body: bool,
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
    hollywood_section_with_work(state, role_state, runner, model, effort, None)
}

/// [`hollywood_section`] carrying the stage's latest sampled [`Work`]
/// for `bl-hollywood` (published only while `role_state == "live"`).
pub fn hollywood_section_with_work(
    state: &HollywoodState,
    role_state: &str,
    runner: Option<&str>,
    model: Option<&str>,
    effort: Option<&str>,
    work: Option<Work>,
) -> HollywoodSummary {
    hollywood_section_with_activity(state, role_state, runner, model, effort, work, None, None)
}

/// [`hollywood_section_with_work`] plus the live activity the band
/// shows under the newest `user ›` line: what `bl-hollywood` is doing
/// (`doing`, read and capped like a role's) and a permission prompt on
/// its screen (`prompt`, the picker's one-line ask). Both publish only
/// while `role_state == "live"`.
#[allow(clippy::too_many_arguments)]
pub fn hollywood_section_with_activity(
    state: &HollywoodState,
    role_state: &str,
    runner: Option<&str>,
    model: Option<&str>,
    effort: Option<&str>,
    work: Option<Work>,
    doing: Option<&str>,
    prompt: Option<&str>,
) -> HollywoodSummary {
    let live = role_state == "live";
    let doing = live
        .then(|| doing.map(steady))
        .flatten()
        .filter(|d| !d.is_empty())
        .map(|d| cap(&d));
    let prompt = live.then_some(prompt).flatten().map(cap);
    HollywoodSummary {
        state: cap(role_state),
        runner: runner.map(cap),
        model: model.map(cap),
        effort: effort.map(cap),
        work: live.then_some(work).flatten(),
        pending: state
            .messages
            .iter()
            .any(|m| m.to == "hollywood" && !m.delivered && !m.dropped && !m.seen),
        doing,
        attention: prompt
            .is_some()
            .then(|| format!("{PERMISSION_PROMPT} — needs the user")),
        attention_user: prompt.is_some(),
        attention_detail: prompt,
        can_answer_prompt: runner.is_none_or(crate::caps::can_answer_prompt),
        headline: state.last_headline().map(cap_headline),
        restarted_ms: state.role.restarted_ms,
        headline_seq: state.reports.last().map(|r| r.seq),
        cards: state
            .open_cards()
            .take(MAX_OPEN_CARDS)
            .map(|c| {
                let action = match (&c.action, &c.base) {
                    (
                        crate::hollywood::CardAction::Setup {
                            runner,
                            model,
                            effort,
                            ..
                        },
                        crate::hollywood::CardBase::HollywoodIdentity { actor },
                    ) => crate::hollywood::format_setup_transition(
                        actor.as_ref(),
                        runner,
                        model,
                        effort.as_deref(),
                    ),
                    _ => c.action.label(),
                };
                CardSummary {
                    id: format!("c{}", c.seq),
                    studio: cap(c.action.studio()),
                    action: cap(&action),
                    why: cap(&c.why),
                }
            })
            .collect(),
        exchange: exchange(state),
    }
}

fn cap_headline(s: &str) -> String {
    s.chars().take(HEADLINE_CHARS).collect()
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
                text: cap_headline(text),
                has_body: m.file.is_some(),
            })
        })
        .chain(state.reports.iter().map(|r| Exchange {
            seq: r.seq,
            who: "hollywood".into(),
            text: cap_headline(&r.headline),
            has_body: r.body.is_some(),
        }))
        .collect();
    lines.sort_by_key(|e| e.seq);
    lines.split_off(lines.len().saturating_sub(MAX_EXCHANGE))
}
