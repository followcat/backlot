//! Staleness rules and passive ride-along outcomes (`card.done`,
//! `accepted`) for Hollywood's notice queue.

use std::collections::{BTreeMap, BTreeSet};

use backlot_model::event::Verdict;
use backlot_model::hollywood::{CardState, HMessage, HollywoodState};
use backlot_model::state::StudioState;
use serde::{Deserialize, Serialize};

use crate::hnotice::label;

/// A passive context item (`card.done` or accepted studio delivery)
/// that never wakes Hollywood on its own and instead rides once with
/// the next real delivery.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub(crate) enum PassiveKey {
    CardDone(u64),
    Accepted { studio: String, seq: u64 },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PassiveItem {
    pub key: PassiveKey,
    pub text: String,
    pub seq: u64,
}

/// Passive outcomes (`card.done`, `accepted`) owed as ride-along context.
pub(crate) fn passive_items(
    studios: &BTreeMap<String, StudioState>,
    h: &HollywoodState,
    accounted: &BTreeSet<PassiveKey>,
) -> Vec<PassiveItem> {
    let mut out = Vec::new();
    for c in h.cards.values() {
        if c.state != CardState::Done {
            continue;
        }
        let key = PassiveKey::CardDone(c.seq);
        if accounted.contains(&key) {
            continue;
        }
        let pre_handled = h.messages.iter().any(|m| {
            m.from == "stage"
                && m.to == "hollywood"
                && m.studio.is_none()
                && m.source_seq == c.closed_seq
                && label(m) == Some("card")
                && (m.delivered || m.dropped)
        });
        if pre_handled {
            continue;
        }
        out.push(PassiveItem {
            key,
            text: if c.detail.is_empty() {
                format!("c{} done", c.seq)
            } else {
                format!("c{} done — {}", c.seq, c.detail)
            },
            seq: c.closed_seq.unwrap_or(c.seq),
        });
    }
    for (name, s) in studios {
        if s.finished.is_some() {
            continue;
        }
        for (id, t) in &s.tasks {
            for r in &t.reviews {
                if !matches!(r.verdict, Verdict::Accepted | Verdict::PartiallyAccepted) {
                    continue;
                }
                let key = PassiveKey::Accepted {
                    studio: name.clone(),
                    seq: r.seq,
                };
                if accounted.contains(&key) {
                    continue;
                }
                let pre_handled = h.messages.iter().any(|m| {
                    m.from == "stage"
                        && m.to == "hollywood"
                        && m.studio.as_deref() == Some(name.as_str())
                        && m.source_seq == Some(r.seq)
                        && label(m) == Some("accepted")
                        && (m.delivered || m.dropped)
                });
                if pre_handled {
                    continue;
                }
                out.push(PassiveItem {
                    key,
                    text: format!("{name} {id} {}", verdict_str(r.verdict)),
                    seq: r.seq,
                });
            }
        }
    }
    out.sort_by(|a, b| b.seq.cmp(&a.seq).then_with(|| a.key.cmp(&b.key)));
    out
}

/// The verdict's ledger word — `Verdict` carries no `as_str`.
fn verdict_str(v: Verdict) -> &'static str {
    match v {
        Verdict::Accepted => "accepted",
        Verdict::PartiallyAccepted => "partially accepted",
        Verdict::ChangesRequested => "changes requested",
        Verdict::Blocked => "blocked",
    }
}

fn attention_role<'a>(m: &HMessage, studios: &'a BTreeMap<String, StudioState>) -> Option<&'a str> {
    let source = m.source_seq?;
    let studio = m.studio.as_deref().and_then(|s| studios.get(s))?;
    studio
        .attention
        .iter()
        .find(|a| a.seq == source)
        .map(|a| a.role.as_str())
}

fn has_newer_pending(
    m: &HMessage,
    l: &str,
    role: Option<&str>,
    studios: &BTreeMap<String, StudioState>,
    h: &HollywoodState,
) -> bool {
    h.messages.iter().any(|other| {
        other.seq > m.seq
            && other.to == "hollywood"
            && other.from == "stage"
            && !other.delivered
            && !other.dropped
            && label(other) == Some(l)
            && other.studio == m.studio
            && (role.is_none() || attention_role(other, studios) == role)
    })
}

/// Does a queued notice still stand? `Some(reason)` drops it —
/// clears obsolete, superseded, or post-finish notices before send.
pub(crate) fn stale_reason(
    m: &HMessage,
    studios: &BTreeMap<String, StudioState>,
    errors: &BTreeMap<String, String>,
    h: &HollywoodState,
) -> Option<String> {
    let l = label(m)?;
    let studio = m.studio.as_deref().and_then(|s| studios.get(s));
    if l != "finished" && studio.is_some_and(|s| s.finished.is_some()) {
        return Some("studio finished".into());
    }
    match l {
        "attention" | "prompt" | "stalled" => {
            let source = m.source_seq?;
            let attn = studio.and_then(|s| s.attention.iter().find(|a| a.seq == source));
            let Some(attn) = attn.filter(|a| a.open) else {
                return Some("attention cleared".into());
            };
            if has_newer_pending(m, l, Some(attn.role.as_str()), studios, h) {
                return Some("superseded by newer notice".into());
            }
            None
        }
        "failed" => {
            let cur = m
                .studio
                .as_deref()
                .and_then(|s| errors.get(s))
                .map(String::as_str);
            if cur != m.text.as_deref() {
                return Some("studio recovered or the error changed".into());
            }
            if has_newer_pending(m, l, None, studios, h) {
                return Some("superseded by newer notice".into());
            }
            None
        }
        "card" => {
            let source = m.source_seq?;
            let applied_closed = h
                .cards
                .values()
                .any(|c| c.applied_seq == Some(source) && c.state != CardState::Applied);
            if applied_closed {
                Some("superseded by outcome".into())
            } else {
                None
            }
        }
        "recovered" => {
            if has_newer_pending(m, l, None, studios, h) {
                return Some("superseded by newer notice".into());
            }
            if h.role.closed
                || (h
                    .role
                    .recovered_seq
                    .is_some_and(|r| m.source_seq.is_some_and(|s| s != r)))
                || (h.role.since_ms > 0 && h.role.recovered_seq.is_none())
            {
                return Some("new session".into());
            }
            None
        }
        // `finished` stands once written.
        _ => None,
    }
}
