//! The overseer's views: the Hollywood block `card list` and
//! `status --all` print, and the confirmation line after a Hollywood
//! ledger append.

use std::fmt::Write as _;

use backlot_model::hollywood::{CardState, HollywoodEvent, HollywoodKind, HollywoodState};

use crate::config::HollywoodCfg;

/// The one-line confirmation after a Hollywood ledger append — same
/// shape as a studio's `event N: <kind> <subject>`.
pub fn hconfirm(event: &HollywoodEvent) -> String {
    let subject = match &event.kind {
        HollywoodKind::CardProposed { action, why, .. } => {
            format!(" c{} {} — {}", event.seq, action.label(), why)
        }
        HollywoodKind::CardApplied { card }
        | HollywoodKind::CardCancelled { card, .. }
        | HollywoodKind::CardDone { card, .. }
        | HollywoodKind::CardFailed { card, .. }
        | HollywoodKind::CardStale { card, .. } => format!(" c{card}"),
        HollywoodKind::Reported { headline, .. } => format!(" \"{headline}\""),
        HollywoodKind::MessageSent { to, .. } => format!(" → {to}"),
    };
    format!("event {}: {}{}", event.seq, event.kind.name(), subject)
}

/// The identity line's tail: `fake/model-f/high`, or why the layer
/// is not configured.
fn cfg_line(cfg: &Result<Option<HollywoodCfg>, String>) -> String {
    match cfg {
        Ok(Some(c)) => {
            let mut s = format!("{}/{}", c.runner, c.model);
            if let Some(e) = c.effort.as_deref() {
                let _ = write!(s, "/{e}");
            }
            s
        }
        Ok(None) => "not configured — set [hollywood] in the backlot config".into(),
        Err(e) => format!("config error: {e}"),
    }
}

/// The Hollywood block: its runner line, the newest headline, and
/// every card with its state — `card list` and the `status --all`
/// trailer share it (D16's list is a superset of this).
pub fn hollywood_block(
    state: &HollywoodState,
    cfg: &Result<Option<HollywoodCfg>, String>,
) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "hollywood ({})", cfg_line(cfg));
    if let Some(h) = state.last_headline() {
        let _ = writeln!(out, "  headline: \"{h}\"");
    }
    if state.cards.is_empty() {
        let _ = writeln!(out, "  cards: none");
    } else {
        let _ = writeln!(out, "  cards:");
        // Open cards first, then the closed ones — each `c<seq>`.
        for c in state
            .cards
            .values()
            .filter(|c| c.state == CardState::Open)
            .chain(state.cards.values().filter(|c| c.state != CardState::Open))
        {
            let _ = write!(
                out,
                "    c{:<3} {:<9} {} {} — {}",
                c.seq,
                c.state.as_str(),
                c.action.studio(),
                c.action.label(),
                c.why
            );
            if !c.detail.is_empty() {
                let _ = write!(out, " ({})", c.detail);
            }
            let _ = writeln!(out);
        }
    }
    out
}

/// The `status --all --json` Hollywood object.
pub fn hollywood_json(
    state: &HollywoodState,
    cfg: &Result<Option<HollywoodCfg>, String>,
) -> serde_json::Value {
    let config = match cfg {
        Ok(Some(c)) => serde_json::json!({
            "runner": c.runner,
            "model": c.model,
            "effort": c.effort,
            "key": c.key,
            "default_cast": c.default_cast,
        }),
        Ok(None) => serde_json::Value::Null,
        Err(e) => serde_json::json!({ "error": e }),
    };
    serde_json::json!({
        "config": config,
        "headline": state.last_headline(),
        "cards": state.cards.values().map(|c| serde_json::json!({
            "id": format!("c{}", c.seq),
            "studio": c.action.studio(),
            "action": c.action.label(),
            "why": c.why,
            "state": c.state.as_str(),
            "detail": c.detail,
        })).collect::<Vec<_>>(),
    })
}
