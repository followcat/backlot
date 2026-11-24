//! The notices Hollywood is owed (R23): each once, with the
//! studio's name — an attention opened, a prompt only the user may
//! answer, a studio finished or failed, a delivery accepted, a
//! stall the stage could not route — plus every card outcome and
//! the recovered-terminal word. The same rules as the studios'
//! notice path: a notice whose source moved on is dropped, a
//! dropped one counts as told, and the source seq dedups across
//! restarts.
//!
//! Every notice is a `message.sent` by the stage to Hollywood with
//! the body under `messages/`; a ledger-backed one carries
//! `source_seq` — in the named studio's ledger, or in Hollywood's
//! own for card outcomes and the recovered word. A `failed` notice
//! has no event to point at: the stage's error text goes in `text`
//! as its dedup key, and it goes stale the moment the error clears
//! or changes.

use std::collections::BTreeMap;

use backlot_model::attention::AttentionClass;
use backlot_model::event::{CANNOT_RESUME, PERMISSION_PROMPT, STALLED};
use backlot_model::hollywood::{CardState, HMessage, HollywoodState};
use backlot_model::state::{StudioState, actor_task};

pub(crate) use crate::hnotice_stale::{PassiveItem, PassiveKey, passive_items, stale_reason};
use crate::stall;

/// One owed notice: `source_seq`/`studio` name the event it
/// relays — `studio` `None` is Hollywood's own ledger — and
/// `token` is the non-event dedup key (`failed`'s error text).
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct HNotice {
    pub label: &'static str,
    pub studio: Option<String>,
    pub role: Option<String>,
    pub source_seq: Option<u64>,
    pub token: Option<String>,
    pub body: String,
}

/// The label a stage-written message's file carries —
/// `messages/<seq>-<label>.md`, the seq being the message's own.
pub(crate) fn label(m: &HMessage) -> Option<&str> {
    let rest = m.file.as_deref()?.strip_prefix("messages/")?;
    let (_, l) = rest.split_once('-')?;
    l.strip_suffix(".md")
}

/// The order a digest lists labels in — stalled/failed/card actionables first.
pub(crate) fn rank(m: &HMessage) -> u8 {
    match label(m) {
        Some("stalled") => 0,
        Some("failed") => 1,
        Some("card") => 2,
        Some("prompt") => 3,
        Some("attention") => 4,
        Some("finished") => 5,
        Some("recovered") => 6,
        _ => 7,
    }
}

/// Was this notice already told — queued, delivered or dropped all
/// count, like the studios' handled set.
fn told(h: &HollywoodState, n: &HNotice) -> bool {
    h.messages.iter().any(|m| {
        m.to == "hollywood"
            && m.from == "stage"
            && m.studio == n.studio
            && match n.source_seq {
                Some(s) => m.source_seq == Some(s),
                None => m.source_seq.is_none() && label(m) == Some(n.label) && m.text == n.token,
            }
    })
}

/// The label and body one `attention.raised` owes Hollywood: a
/// prompt only the user may answer, a stall it could not route, else a
/// plain attention. A producer that stalls or cannot resume is always
/// Hollywood's — the studio can only tell its live director, and only
/// the user or Hollywood can put the producer back — so its notice
/// leads with the producer, not the generic unrouted wording.
///
/// The body carries the attention's *facts*: its class, its age and,
/// for a screen snapshot, when the screen was taken with the stale
/// warning (P5, H1) — so Hollywood never reads a launch-time snapshot
/// as the role's current state. `now_ms` ages the raise.
fn attention_notice(
    studio: &str,
    s: &StudioState,
    a: &backlot_model::state::Attention,
    now_ms: u64,
) -> HNotice {
    let producer_down =
        a.role == "producer" && matches!(a.reason.as_str(), STALLED | CANNOT_RESUME);
    let unrouted = producer_down
        || (matches!(a.reason.as_str(), STALLED | CANNOT_RESUME)
            && stall::notice_to(s, a).is_none());
    let user_owned =
        a.reason == PERMISSION_PROMPT && (a.needs_user || actor_task(&a.role).is_none());
    let (label, head) = if user_owned {
        (
            "prompt",
            format!("# {studio}: permission prompt — needs the user"),
        )
    } else if producer_down {
        let what = if a.reason == STALLED {
            "producer stalled"
        } else {
            "producer cannot resume"
        };
        ("stalled", format!("# {studio}: {what} — needs attention"))
    } else if unrouted {
        (
            "stalled",
            format!("# {studio}: a stall the stage could not route"),
        )
    } else {
        ("attention", format!("# {studio}: attention opened"))
    };
    let fact = crate::render::attention_fact(a, now_ms);
    HNotice {
        label,
        studio: Some(studio.to_string()),
        role: Some(a.role.clone()),
        source_seq: Some(a.seq),
        token: None,
        body: format!(
            "{head}\n\n`{}` — {}: {}\n\nfact: {fact}\n",
            a.role, a.reason, a.detail
        ),
    }
}

/// Keep only the newest candidate per `(studio, role, label)` (and at
/// most one `recovered`) before checking `told` so superseded items never
/// surface on a later tick. Distinct cards (`studio == None`) do not coalesce.
fn coalesce(candidates: Vec<HNotice>) -> Vec<HNotice> {
    let mut best_studio: BTreeMap<(String, Option<String>, &'static str), usize> = BTreeMap::new();
    let mut best_recovered: Option<usize> = None;
    let mut keep = vec![true; candidates.len()];
    for (idx, n) in candidates.iter().enumerate() {
        let seq = n.source_seq.unwrap_or(0);
        if let Some(studio) = &n.studio {
            let key = (studio.clone(), n.role.clone(), n.label);
            if let Some(&prev) = best_studio.get(&key) {
                if seq >= candidates[prev].source_seq.unwrap_or(0) {
                    keep[prev] = false;
                    best_studio.insert(key, idx);
                } else {
                    keep[idx] = false;
                }
            } else {
                best_studio.insert(key, idx);
            }
        } else if n.label == "recovered" {
            if let Some(prev) = best_recovered {
                if seq >= candidates[prev].source_seq.unwrap_or(0) {
                    keep[prev] = false;
                    best_recovered = Some(idx);
                } else {
                    keep[idx] = false;
                }
            } else {
                best_recovered = Some(idx);
            }
        }
    }
    candidates
        .into_iter()
        .enumerate()
        .filter_map(|(i, n)| keep[i].then_some(n))
        .collect()
}

/// Every actionable notice the studios and Hollywood's own ledger owe,
/// oldest first. Closed/`info` attentions, `card.applied`, `card.done`,
/// and `accepted` never emit wake-up notices.
pub(crate) fn owed(
    studios: &BTreeMap<String, StudioState>,
    errors: &BTreeMap<String, String>,
    h: &HollywoodState,
    now_ms: u64,
) -> Vec<HNotice> {
    let mut out = Vec::new();
    for (name, s) in studios {
        if let Some(f) = &s.finished {
            out.push(HNotice {
                label: "finished",
                studio: Some(name.clone()),
                role: None,
                source_seq: Some(f.seq),
                token: None,
                body: format!("# {name}: studio finished\n\n{}\n", f.reason),
            });
            continue;
        }
        for a in &s.attention {
            if !a.open || AttentionClass::of(&a.reason, a.needs_user) == AttentionClass::Info {
                continue;
            }
            out.push(attention_notice(name, s, a, now_ms));
        }
        if let Some(e) = errors.get(name) {
            out.push(HNotice {
                label: "failed",
                studio: Some(name.clone()),
                role: None,
                source_seq: None,
                token: Some(e.clone()),
                body: format!("# {name}: the stage cannot run this studio\n\n{e}\n"),
            });
        }
    }
    // Unloaded/broken studios live only in `errors`.
    for (name, e) in errors {
        if !studios.contains_key(name) {
            out.push(HNotice {
                label: "failed",
                studio: Some(name.clone()),
                role: None,
                source_seq: None,
                token: Some(e.clone()),
                body: format!("# {name}: the stage cannot run this studio\n\n{e}\n"),
            });
        }
    }
    // Hollywood's own words: `recovered` and non-done card failures.
    if let Some(seq) = h.role.recovered_seq {
        out.push(HNotice {
            label: "recovered",
            studio: None,
            role: None,
            source_seq: Some(seq),
            token: None,
            body: "# your terminal was lost — the session was resumed\n".into(),
        });
    }
    for c in h.cards.values() {
        if let Some(seq) = c.closed_seq
            && matches!(
                c.state,
                CardState::Failed | CardState::Stale | CardState::Cancelled
            )
        {
            out.push(HNotice {
                label: "card",
                studio: None,
                role: None,
                source_seq: Some(seq),
                token: None,
                body: card_outcome(c),
            });
        }
    }
    let mut out = coalesce(out);
    out.retain(|n| !told(h, n));
    out.sort_by_key(|n| n.source_seq.unwrap_or(u64::MAX));
    out
}

/// The card-outcome notice body — `card c3 — done`, the detail the
/// closing event recorded when there is one.
fn card_outcome(c: &backlot_model::hollywood::Card) -> String {
    let mut body = format!(
        "# card c{} — {}\n\n{}",
        c.seq,
        c.state.as_str(),
        c.action.label()
    );
    if !c.detail.is_empty() {
        body.push_str(&format!(" — {}", c.detail));
    }
    body.push('\n');
    body
}
