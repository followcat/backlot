//! The summary document's `hollywood` section (R27, D20): an old
//! document still parses, the fields cap where D20 says, and the
//! whole document stays under 64 KiB at those limits.

use std::collections::BTreeMap;

use crate::hollywood::{CardAction, CardBase, HollywoodKind, HollywoodState, apply};
use crate::state::{StudioState, TaskStatus};
use crate::summary::{MAX_OPEN_CARDS, SUMMARY_SCHEMA, Summary, hollywood_section, summary};
use crate::testkit::*;

// ── The summary's `hollywood` section (R27, D20) ───────────────

/// A document written before the field existed still parses —
/// `hollywood` just comes out `None`.
#[test]
fn a_document_without_hollywood_still_parses() {
    let old = serde_json::json!({
        "schema": SUMMARY_SCHEMA,
        "updated_ms": 1,
        "studios": [],
    });
    let s: Summary = serde_json::from_value(old).unwrap();
    assert_eq!(s.hollywood, None);
    // And the section itself skips none-fields on the wire.
    let section = hollywood_section(&HollywoodState::default(), "down", None, None, None);
    let v = serde_json::to_value(&section).unwrap();
    assert_eq!(v["state"], "down");
    assert!(v.get("runner").is_none());
    assert!(v.get("headline").is_none());
    assert!(v.get("cards").is_none());
}

/// The headline caps at 200 chars, only the first 10 open cards
/// reach the document, and every card string is capped.
#[test]
fn hollywood_section_caps() {
    let mut state = HollywoodState::default();
    for i in 1..=12u64 {
        apply(&mut state, &hev(i, "hollywood", hpropose(&format!("t{i}"))));
    }
    apply(
        &mut state,
        &hev(
            20,
            "hollywood",
            HollywoodKind::Reported {
                headline: "h".repeat(300),
                body: None,
            },
        ),
    );
    let s = hollywood_section(
        &state,
        "live",
        Some("runner-a"),
        Some("model-a"),
        Some("high"),
    );
    assert_eq!(s.state, "live");
    assert_eq!(s.runner.as_deref(), Some("runner-a"));
    assert_eq!(s.headline.as_ref().unwrap().chars().count(), 200);
    assert_eq!(s.cards.len(), MAX_OPEN_CARDS);
    assert_eq!(s.cards[0].id, "c1");
    assert_eq!(s.cards[0].action, "pause t1");
    assert!(s.cards[0].why.len() <= 96);
}

/// At the caps' limits — a full studio list plus the biggest
/// Hollywood section — the document stays under 64 KiB.
#[test]
fn the_document_stays_under_64kib_at_the_hollywood_caps() {
    let mut state = HollywoodState::default();
    let long = "y".repeat(300);
    for i in 1..=12u64 {
        apply(
            &mut state,
            &hev(
                i,
                "hollywood",
                HollywoodKind::CardProposed {
                    action: CardAction::Pause {
                        studio: long.clone(),
                        task: "t1".into(),
                    },
                    why: long.clone(),
                    base: CardBase::TaskStatus {
                        was: TaskStatus::Running,
                    },
                },
            ),
        );
    }
    apply(
        &mut state,
        &hev(
            20,
            "hollywood",
            HollywoodKind::Reported {
                headline: "h".repeat(300),
                body: None,
            },
        ),
    );
    let studios: Vec<(String, StudioState)> = (0..12)
        .map(|i| (format!("studio-{i}"), base_state()))
        .collect();
    let mut s = summary(
        &studios,
        &BTreeMap::new(),
        &BTreeMap::new(),
        &BTreeMap::new(),
        &BTreeMap::new(),
        1,
    );
    s.hollywood = Some(hollywood_section(
        &state,
        "live",
        Some("r"),
        Some("m"),
        Some("high"),
    ));
    let bytes = serde_json::to_vec(&s).unwrap().len();
    assert!(bytes < 64 * 1024, "{bytes}");
}
