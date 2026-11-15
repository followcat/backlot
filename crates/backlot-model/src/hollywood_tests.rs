//! Hollywood ledger tests: the event envelope's wire shape and the
//! replay-equals-apply promise the studio ledger makes too.

use crate::hollywood::*;
use crate::testkit::*;

/// Every kind serializes with its `kind` tag and its own fields.
#[test]
fn the_envelope_matches_the_studio_ledger() {
    let e = hev(3, "hollywood", hpropose("t1"));
    let v = serde_json::to_value(&e).unwrap();
    assert_eq!(v["seq"], 3);
    assert_eq!(v["ts_ms"], 3000);
    assert_eq!(v["by"]["role"], "hollywood");
    assert_eq!(v["kind"], "card.proposed");
    assert_eq!(v["action"]["action"], "pause");
    assert_eq!(v["action"]["studio"], "acme-shop");
    assert_eq!(v["base"]["base"], "task_status");
    assert_eq!(v["base"]["was"], "running");
    assert_eq!(v["why"], "actor is looping");
    for (kind, tag) in [
        (HollywoodKind::CardApplied { card: 3 }, "card.applied"),
        (
            HollywoodKind::CardCancelled {
                card: 3,
                reason: "no".into(),
            },
            "card.cancelled",
        ),
        (
            HollywoodKind::CardDone {
                card: 3,
                note: String::new(),
            },
            "card.done",
        ),
        (
            HollywoodKind::CardFailed {
                card: 3,
                reason: "boom".into(),
            },
            "card.failed",
        ),
        (
            HollywoodKind::CardStale {
                card: 3,
                reason: "moved".into(),
            },
            "card.stale",
        ),
        (
            HollywoodKind::Reported {
                headline: "all quiet".into(),
                body: Some("reports/3.md".into()),
            },
            "hollywood.reported",
        ),
        (
            HollywoodKind::MessageSent {
                to: "hollywood".into(),
                studio: None,
                text: Some("hello".into()),
                file: None,
                source_seq: None,
            },
            "message.sent",
        ),
    ] {
        let v = serde_json::to_value(hev(9, "user", kind)).unwrap();
        assert_eq!(v["kind"], tag);
    }
}

/// Replaying the log is applying each event in order — the same
/// promise the studio ledger makes.
#[test]
fn replay_equals_incremental_apply() {
    let events = vec![
        hev(1, "hollywood", hpropose("t1")),
        hev(2, "hollywood", hpropose("t2")),
        hev(3, "user", HollywoodKind::CardApplied { card: 1 }),
        hev(
            4,
            "stage",
            HollywoodKind::CardFailed {
                card: 1,
                reason: "stage down".into(),
            },
        ),
        hev(
            5,
            "user",
            HollywoodKind::CardCancelled {
                card: 2,
                reason: "changed my mind".into(),
            },
        ),
        hev(
            6,
            "hollywood",
            HollywoodKind::Reported {
                headline: "one done, one dropped".into(),
                body: None,
            },
        ),
        hev(
            7,
            "user",
            HollywoodKind::MessageSent {
                to: "hollywood".into(),
                studio: None,
                text: Some("thanks".into()),
                file: None,
                source_seq: None,
            },
        ),
    ];
    let mut inc = HollywoodState::default();
    for e in &events {
        apply(&mut inc, e);
    }
    let state = replay(&events);
    assert_eq!(state, inc);
    assert_eq!(state.last_seq, 7);
    assert_eq!(state.cards[&1].state, CardState::Failed);
    assert_eq!(state.cards[&1].detail, "stage down");
    assert_eq!(state.cards[&1].closed_seq, Some(4));
    assert_eq!(state.cards[&2].state, CardState::Cancelled);
    assert_eq!(state.reports.len(), 1);
    assert_eq!(state.last_headline(), Some("one done, one dropped"));
    assert_eq!(state.messages.len(), 1);
    assert_eq!(state.messages[0].from, "user");
    assert_eq!(state.messages[0].to, "hollywood");
}
