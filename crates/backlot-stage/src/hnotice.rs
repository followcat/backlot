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

use backlot_model::event::{CANNOT_RESUME, PERMISSION_PROMPT, STALLED, Verdict};
use backlot_model::hollywood::{CardState, HMessage, HollywoodState};
use backlot_model::state::{StudioState, actor_task};

use crate::stall;

/// One owed notice: `source_seq`/`studio` name the event it
/// relays — `studio` `None` is Hollywood's own ledger — and
/// `token` is the non-event dedup key (`failed`'s error text).
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct HNotice {
    pub label: &'static str,
    pub studio: Option<String>,
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

/// The order a digest lists labels in — what needs a hand first.
pub(crate) fn rank(m: &HMessage) -> u8 {
    match label(m) {
        Some("prompt") => 0,
        Some("stalled") => 1,
        Some("attention") => 2,
        Some("failed") => 3,
        Some("accepted") => 4,
        Some("finished") => 5,
        Some("recovered") => 6,
        Some("card") => 8,
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
/// prompt only the user may answer, a stall it could not route,
/// else a plain attention.
fn attention_notice(studio: &str, s: &StudioState, a: &backlot_model::state::Attention) -> HNotice {
    let unrouted =
        matches!(a.reason.as_str(), STALLED | CANNOT_RESUME) && stall::notice_to(s, a).is_none();
    let user_owned =
        a.reason == PERMISSION_PROMPT && (a.needs_user || actor_task(&a.role).is_none());
    let (label, head) = if user_owned {
        (
            "prompt",
            format!("# {studio}: permission prompt — needs the user"),
        )
    } else if unrouted {
        (
            "stalled",
            format!("# {studio}: a stall the stage could not route"),
        )
    } else {
        ("attention", format!("# {studio}: attention opened"))
    };
    HNotice {
        label,
        studio: Some(studio.to_string()),
        source_seq: Some(a.seq),
        token: None,
        body: format!("{head}\n\n`{}` — {}: {}\n", a.role, a.reason, a.detail),
    }
}

/// Every notice the studios and Hollywood's own ledger owe the
/// layer, oldest first. `errors` is the stage's per-studio error
/// map (`failed`/`broken`) — the `failed` notice's source.
pub(crate) fn owed(
    studios: &BTreeMap<String, StudioState>,
    errors: &BTreeMap<String, String>,
    h: &HollywoodState,
) -> Vec<HNotice> {
    let mut out = Vec::new();
    for (name, s) in studios {
        for a in &s.attention {
            out.push(attention_notice(name, s, a));
        }
        if let Some(f) = &s.finished {
            out.push(HNotice {
                label: "finished",
                studio: Some(name.clone()),
                source_seq: Some(f.seq),
                token: None,
                body: format!("# {name}: studio finished\n\n{}\n", f.reason),
            });
        }
        for (id, t) in &s.tasks {
            for r in &t.reviews {
                if matches!(r.verdict, Verdict::Accepted | Verdict::PartiallyAccepted) {
                    out.push(HNotice {
                        label: "accepted",
                        studio: Some(name.clone()),
                        source_seq: Some(r.seq),
                        token: None,
                        body: format!(
                            "# {name}: delivery accepted\n\nTask `{id}` delivery {} — {} by {}: {}\n",
                            r.delivery,
                            verdict_str(r.verdict),
                            r.reviewer,
                            r.conclusion
                        ),
                    });
                }
            }
        }
        if let Some(e) = errors.get(name) {
            out.push(HNotice {
                label: "failed",
                studio: Some(name.clone()),
                source_seq: None,
                token: Some(e.clone()),
                body: format!("# {name}: the stage cannot run this studio\n\n{e}\n"),
            });
        }
    }
    // Hollywood's own words: the recovered-terminal notice and
    // every card outcome.
    if let Some(seq) = h.role.recovered_seq {
        out.push(HNotice {
            label: "recovered",
            studio: None,
            source_seq: Some(seq),
            token: None,
            body: "# your terminal was lost — the session was resumed\n".into(),
        });
    }
    for c in h.cards.values() {
        if let Some(seq) = c.applied_seq {
            out.push(HNotice {
                label: "card",
                studio: None,
                source_seq: Some(seq),
                token: None,
                body: format!(
                    "# card c{} applied\n\n{} — carrying it out\n",
                    c.seq,
                    c.action.label()
                ),
            });
        }
        if let Some(seq) = c.closed_seq
            && c.state != CardState::Applied
        {
            out.push(HNotice {
                label: "card",
                studio: None,
                source_seq: Some(seq),
                token: None,
                body: card_outcome(c),
            });
        }
    }
    out.retain(|n| !told(h, n));
    out.sort_by_key(|n| n.source_seq.unwrap_or(u64::MAX));
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

/// Does a queued notice still stand? `Some(reason)` drops it —
/// mirrors the studios' stale table on Hollywood's labels. A send
/// in flight is the caller's problem, not a reason to drop.
pub(crate) fn stale_reason(
    m: &HMessage,
    studios: &BTreeMap<String, StudioState>,
    errors: &BTreeMap<String, String>,
) -> Option<String> {
    let l = label(m)?;
    let studio = m.studio.as_deref().and_then(|s| studios.get(s));
    match l {
        "attention" | "prompt" | "stalled" => {
            let source = m.source_seq?;
            let open = studio
                .and_then(|s| s.attention.iter().find(|a| a.seq == source))
                .is_some_and(|a| a.open);
            if open {
                None
            } else {
                Some("attention cleared".into())
            }
        }
        "failed" => {
            let cur = m
                .studio
                .as_deref()
                .and_then(|s| errors.get(s))
                .map(String::as_str);
            if cur == m.text.as_deref() {
                None
            } else {
                Some("studio recovered or the error changed".into())
            }
        }
        // Finished, accepted, card and recovered are facts — they
        // never go stale.
        _ => None,
    }
}
