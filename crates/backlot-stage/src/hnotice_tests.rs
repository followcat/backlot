//! The notices Hollywood is owed (R23): which studio facts become
//! a notice, which label each carries, how `source_seq` dedups a
//! restart, and when a queued notice drops instead of sending.

use std::collections::BTreeMap;

use backlot_model::event::{CANNOT_RESUME, PERMISSION_PROMPT, STALLED, Verdict};
use backlot_model::hollywood::{self, CardState, HMessage, HollywoodKind, HollywoodState};
use backlot_model::state::{Attention, FinishRecord, ReviewRecord, StudioState};
use backlot_model::testkit as tk;

use crate::hnotice::{self, HNotice};

fn attention(seq: u64, role: &str, reason: &str, needs_user: bool) -> Attention {
    Attention {
        seq,
        role: role.into(),
        reason: reason.into(),
        detail: "d".into(),
        open: true,
        ts_ms: 0,
        needs_user,
        cleared_reason: None,
    }
}

fn one(studios: &[(&str, StudioState)]) -> Vec<HNotice> {
    let map: BTreeMap<String, StudioState> = studios
        .iter()
        .map(|(n, s)| (n.to_string(), s.clone()))
        .collect();
    hnotice::owed(&map, &BTreeMap::new(), &HollywoodState::default())
}

/// An open attention is one notice, named with its studio and
/// pointing at the raising seq.
#[test]
fn an_attention_names_its_studio() {
    let mut s = StudioState::default();
    s.attention
        .push(attention(7, "actor-t1-c1", PERMISSION_PROMPT, false));
    // An actor's own prompt is just attention — the director may
    // still answer it.
    let n = one(&[("acme-shop", s)]);
    assert_eq!(n.len(), 1);
    assert_eq!(n[0].label, "attention");
    assert_eq!(n[0].studio.as_deref(), Some("acme-shop"));
    assert_eq!(n[0].source_seq, Some(7));
    assert!(n[0].body.contains("acme-shop"), "{}", n[0].body);
}

/// A prompt only the user may answer — `needs_user` or a role with
/// no task — is the `prompt` notice; a stall nobody routes is
/// `stalled`.
#[test]
fn the_prompt_and_unrouted_stall_labels() {
    let mut s = StudioState::default();
    s.attention
        .push(attention(3, "producer", PERMISSION_PROMPT, true));
    s.attention.push(attention(4, "producer", STALLED, false));
    let n = one(&[("acme", s)]);
    let labels: Vec<&str> = n.iter().map(|x| x.label).collect();
    assert_eq!(labels, vec!["prompt", "stalled"]);

    // A `cannot resume` the producer owns routes nowhere either.
    let mut s = StudioState::default();
    s.attention
        .push(attention(5, "producer", CANNOT_RESUME, false));
    let n = one(&[("acme", s)]);
    assert_eq!(n[0].label, "stalled");
}

/// `studio.finished`, an accepted delivery and a stage error each
/// owe one notice — the error's dedup token is its text, not a seq.
#[test]
fn finished_accepted_and_failed() {
    let mut s = StudioState {
        finished: Some(FinishRecord {
            seq: 9,
            reason: "all accepted".into(),
            ts_ms: 0,
        }),
        ..Default::default()
    };
    let mut task = tk::base_state().tasks["t1"].clone();
    task.reviews.push(ReviewRecord {
        delivery: 1,
        reviewer: "director".into(),
        verdict: Verdict::Accepted,
        conclusion: "done".into(),
        findings: Vec::new(),
        upstream_changes: Vec::new(),
        seq: 12,
        ts_ms: 0,
    });
    s.tasks.insert("t1".into(), task);
    let mut errors = BTreeMap::new();
    errors.insert("acme".to_string(), "ledger: corrupt".to_string());
    let map: BTreeMap<String, StudioState> = [("acme".to_string(), s)].into();
    let n = hnotice::owed(&map, &errors, &HollywoodState::default());
    let labels: Vec<&str> = n.iter().map(|x| x.label).collect();
    assert_eq!(labels, vec!["finished", "accepted", "failed"], "{labels:?}");
    let failed = &n[2];
    assert_eq!(failed.source_seq, None);
    assert_eq!(failed.token.as_deref(), Some("ledger: corrupt"));
}

/// Hollywood's own words: a `resumed recovered` and each card's
/// outcome are notices on no studio — `studio: None` points at
/// Hollywood's own ledger.
#[test]
fn the_recovered_word_and_card_outcomes() {
    let mut h = HollywoodState::default();
    h.role.recovered_seq = Some(5);
    // An applied card: the "carrying it out" word.
    hollywood::apply(&mut h, &tk::hev(1, "hollywood", tk::hpropose("t1")));
    hollywood::apply(
        &mut h,
        &tk::hev(2, "user", HollywoodKind::CardApplied { card: 1 }),
    );
    let n = hnotice::owed(&BTreeMap::new(), &BTreeMap::new(), &h);
    let labels: Vec<&str> = n.iter().map(|x| x.label).collect();
    assert_eq!(labels, vec!["card", "recovered"], "{labels:?}");
    assert!(n.iter().all(|x| x.studio.is_none()));
    assert!(n[0].body.contains("applied"), "{}", n[0].body);
    assert_eq!(n[0].source_seq, Some(2));
    assert_eq!(n[1].source_seq, Some(5));

    // A closed card: the outcome word, once.
    hollywood::apply(
        &mut h,
        &tk::hev(
            3,
            "stage",
            HollywoodKind::CardDone {
                card: 1,
                note: String::new(),
            },
        ),
    );
    let n = hnotice::owed(&BTreeMap::new(), &BTreeMap::new(), &h);
    let card_notices: Vec<&HNotice> = n.iter().filter(|x| x.label == "card").collect();
    assert_eq!(card_notices.len(), 2);
    let done = card_notices[1];
    assert_eq!(done.source_seq, Some(3));
    assert!(done.body.contains("done"), "{}", done.body);
    assert_eq!(h.cards[&1].state, CardState::Done);
}

/// A notice already in the inbox — queued, delivered or dropped —
/// is never owed again: `source_seq` is the dedup key across a
/// stage restart.
#[test]
fn told_notices_are_never_owed_again() {
    let mut s = StudioState::default();
    s.attention
        .push(attention(7, "actor-t1-c1", PERMISSION_PROMPT, false));
    let map: BTreeMap<String, StudioState> = [("acme".to_string(), s)].into();
    for (delivered, dropped) in [(false, false), (true, false), (false, true)] {
        let mut h = HollywoodState::default();
        h.messages.push(HMessage {
            seq: 3,
            ts_ms: 0,
            from: "stage".into(),
            to: "hollywood".into(),
            studio: Some("acme".into()),
            text: None,
            file: Some("messages/3-attention.md".into()),
            source_seq: Some(7),
            delivered,
            dropped,
        });
        assert!(
            hnotice::owed(&map, &BTreeMap::new(), &h).is_empty(),
            "delivered={delivered} dropped={dropped} was owed again"
        );
    }
    // A `failed` dedups on its error text, the same one told stays
    // told; a changed error is a new notice. The attention's own
    // told-marker rides along — it stays told as well.
    let mut errors = BTreeMap::new();
    errors.insert("acme".to_string(), "e1".to_string());
    let mut h = HollywoodState::default();
    h.messages.push(HMessage {
        seq: 3,
        ts_ms: 0,
        from: "stage".into(),
        to: "hollywood".into(),
        studio: Some("acme".into()),
        text: None,
        file: Some("messages/3-attention.md".into()),
        source_seq: Some(7),
        delivered: true,
        dropped: false,
    });
    h.messages.push(HMessage {
        seq: 4,
        ts_ms: 0,
        from: "stage".into(),
        to: "hollywood".into(),
        studio: Some("acme".into()),
        text: Some("e1".into()),
        file: Some("messages/4-failed.md".into()),
        source_seq: None,
        delivered: false,
        dropped: false,
    });
    assert!(hnotice::owed(&map, &errors, &h).is_empty());
    errors.insert("acme".to_string(), "e2".to_string());
    let owed = hnotice::owed(&map, &errors, &h);
    assert_eq!(owed.len(), 1);
    assert_eq!(owed[0].label, "failed");
}

/// A queued notice whose source moved on drops, never sends: an
/// attention that cleared and a `failed` whose error changed.
/// Finished and card outcomes are facts — they never go stale.
#[test]
fn a_moved_source_drops_the_notice() {
    let msg = |label: &str, studio: Option<&str>, seq: Option<u64>, text: Option<&str>| HMessage {
        seq: 1,
        ts_ms: 0,
        from: "stage".into(),
        to: "hollywood".into(),
        studio: studio.map(str::to_string),
        text: text.map(str::to_string),
        file: Some(format!("messages/1-{label}.md")),
        source_seq: seq,
        delivered: false,
        dropped: false,
    };
    let mut s = StudioState::default();
    s.attention
        .push(attention(7, "actor-t1-c1", PERMISSION_PROMPT, false));
    s.finished = Some(FinishRecord {
        seq: 9,
        reason: "done".into(),
        ts_ms: 0,
    });
    let map: BTreeMap<String, StudioState> = [("acme".to_string(), s)].into();
    let errors = BTreeMap::new();

    let open = msg("attention", Some("acme"), Some(7), None);
    assert_eq!(hnotice::stale_reason(&open, &map, &errors), None);
    // The attention clears — the queued copy drops.
    let gone = msg("attention", Some("acme"), Some(8), None);
    assert!(hnotice::stale_reason(&gone, &map, &errors).is_some());
    // A `failed` whose text no longer matches drops too.
    let failed = msg("failed", Some("acme"), None, Some("e1"));
    assert!(hnotice::stale_reason(&failed, &map, &errors).is_some());
    // Facts stand: finished never goes stale.
    let finished = msg("finished", Some("acme"), Some(9), None);
    assert_eq!(hnotice::stale_reason(&finished, &map, &errors), None);
}

/// Labels read back off the file name; the rank puts what needs a
/// hand first in the digest.
#[test]
fn labels_and_the_digest_order() {
    let m = |file: &str| HMessage {
        seq: 1,
        ts_ms: 0,
        from: "stage".into(),
        to: "hollywood".into(),
        studio: None,
        text: None,
        file: Some(file.into()),
        source_seq: None,
        delivered: false,
        dropped: false,
    };
    assert_eq!(hnotice::label(&m("messages/3-prompt.md")), Some("prompt"));
    assert_eq!(hnotice::label(&m("messages/x.md")), None);
    assert_eq!(hnotice::label(&m("other/3-prompt.md")), None);
    let prompt = hnotice::rank(&m("messages/1-prompt.md"));
    let card = hnotice::rank(&m("messages/2-card.md"));
    assert!(prompt < card, "the prompt does not lead the digest");
}
