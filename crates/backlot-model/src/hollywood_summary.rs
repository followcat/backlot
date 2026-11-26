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
/// Maximum open outward cards per studio in `StudioSummary`.
pub const MAX_OUTWARD_CARDS: usize = 10;
/// Maximum commit preview lines per outward card in `StudioSummary`.
pub const MAX_OUTWARD_COMMITS: usize = 5;
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
    /// Live active terminal address recorded in `role.state started|resumed`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub terminal: Option<String>,
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
    /// Additive queue waiting status for Hollywood's outbox.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub queue: Option<crate::hollywood_switch::QueueStatus>,
    /// Active or most recent make-before-break switch handoff.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub handoff: Option<crate::hollywood_switch::HandoffSummary>,
}

pub use crate::hollywood_switch::{HandoffSummary, QueueReason, QueueStatus};

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
    /// The recipient (`hollywood`, `user`, …).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub to: Option<String>,
    /// Turn kind: `"tell"` for `message.sent`, `"report"` for
    /// `hollywood.reported`, and `"card"` for a `card.proposed`
    /// Hollywood wrote itself (additive on `backlot/1`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    /// Reserved for schema parity with studio exchange entries; omitted on wire.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task: Option<String>,
    /// Ledger timestamp in milliseconds, omitted when `0`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ts_ms: Option<u64>,
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
    /// Proposal timestamp in milliseconds, omitted when `0`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ts_ms: Option<u64>,
}

/// One open outward card projected onto [`crate::summary::StudioSummary`]
/// so the Lot Call Sheet, Studio dock, remote hosts, and `wherdr` see it
/// with or without `[hollywood]` (push-perms spec §2.2).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OutwardCardSummary {
    pub id: String,
    pub role: String,
    pub command: String,
    pub rule: String,
    pub why: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub repo: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub remote_url: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub commits: Vec<String>,
    #[serde(default, skip_serializing_if = "crate::summary::is_zero")]
    pub commits_total: u32,
}

/// Project open outward cards for `studio` from `HollywoodState`.
pub fn studio_outward_cards(state: &HollywoodState, studio: &str) -> Vec<OutwardCardSummary> {
    state
        .open_cards()
        .filter_map(|c| {
            let crate::hollywood::CardAction::Outward {
                studio: card_studio,
                role,
                argv,
                repo,
                remote_url,
                rule,
                commits,
                commits_total,
                ..
            } = &c.action
            else {
                return None;
            };
            if card_studio != studio {
                return None;
            }
            Some(OutwardCardSummary {
                id: format!("c{}", c.seq),
                role: cap(role),
                command: cap_headline(&argv.join(" ")),
                rule: cap(rule),
                why: cap_headline(&c.why),
                repo: cap_headline(repo),
                remote_url: cap_headline(remote_url),
                commits: commits
                    .iter()
                    .take(MAX_OUTWARD_COMMITS)
                    .map(|l| cap(l))
                    .collect(),
                commits_total: *commits_total,
            })
        })
        .take(MAX_OUTWARD_CARDS)
        .collect()
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
        terminal: live.then(|| state.role.terminal.clone()).flatten(),
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
            .map(|c| CardSummary {
                id: format!("c{}", c.seq),
                studio: cap(c.action.studio()),
                action: cap(&card_action_label(c)),
                why: cap(&c.why),
                ts_ms: (c.ts_ms > 0).then_some(c.ts_ms),
            })
            .collect(),
        exchange: exchange(state),
        queue: None,
        handoff: None,
    }
}

fn cap_headline(s: &str) -> String {
    s.chars().take(HEADLINE_CHARS).collect()
}

/// A card's one-line action as the summary shows it: a `Setup` card
/// names the identity transition, every other card its own label.
fn card_action_label(c: &crate::hollywood::Card) -> String {
    match (&c.action, &c.base) {
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
    }
}

/// A `card.proposed` line's text — the same words the Call Sheet's
/// CARD row uses (`proposed card c3 · acme-shop · pause t1`).
fn card_line(c: &crate::hollywood::Card) -> String {
    format!(
        "proposed card c{} · {} · {}",
        c.seq,
        c.action.studio(),
        card_action_label(c)
    )
}

/// The conversation lines: each `message.sent` `to` Hollywood that
/// carries text, each `reported` headline, and each `card.proposed`
/// Hollywood wrote itself, merged on their seq and cut to the newest
/// [`MAX_EXCHANGE`]. A card the stage proposed (an `Outward` card) is
/// not Hollywood's word and never enters the talk.
fn exchange(state: &HollywoodState) -> Vec<Exchange> {
    let mut lines: Vec<Exchange> = state
        .messages
        .iter()
        .filter(|m| m.to == "hollywood")
        .filter_map(|m| {
            m.text.as_ref().map(|text| Exchange {
                seq: m.seq,
                who: cap(&m.from),
                to: Some(cap(&m.to)),
                kind: Some("tell".into()),
                task: None,
                ts_ms: (m.ts_ms > 0).then_some(m.ts_ms),
                text: cap_headline(text),
                has_body: m.file.is_some(),
            })
        })
        .chain(state.reports.iter().map(|r| Exchange {
            seq: r.seq,
            who: "hollywood".into(),
            to: Some("user".into()),
            kind: Some("report".into()),
            task: None,
            ts_ms: (r.ts_ms > 0).then_some(r.ts_ms),
            text: cap_headline(&r.headline),
            has_body: r.body.is_some(),
        }))
        .chain(
            state
                .cards
                .values()
                .filter(|c| c.by == crate::hollywood::HOLLYWOOD)
                .map(|c| Exchange {
                    seq: c.seq,
                    who: "hollywood".into(),
                    to: Some("user".into()),
                    kind: Some("card".into()),
                    task: None,
                    ts_ms: (c.ts_ms > 0).then_some(c.ts_ms),
                    text: cap_headline(&card_line(c)),
                    has_body: false,
                }),
        )
        .collect();
    lines.sort_by_key(|e| e.seq);
    lines.split_off(lines.len().saturating_sub(MAX_EXCHANGE))
}
