//! Staleness, coalescing, rank, and S2 notice-filtering tests (`hnotice`).

use std::collections::BTreeMap;

use backlot_model::event::PERMISSION_PROMPT;
use backlot_model::hollywood::{self, HMessage, HollywoodKind, HollywoodState};
use backlot_model::state::{Attention, FinishRecord, StudioState};
use backlot_model::testkit as tk;

use crate::hnotice;

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
        account: None,
        reset_at_ms: None,
    }
}

/// A queued notice whose source moved on drops, never sends: cleared,
/// superseded, `card.applied` after card closes, or post-finish studio notice.
#[test]
fn a_moved_source_drops_the_notice() {
    let msg =
        |seq: u64, label: &str, studio: Option<&str>, src: Option<u64>, text: Option<&str>| {
            HMessage {
                seq,
                ts_ms: 0,
                from: "stage".into(),
                to: "hollywood".into(),
                studio: studio.map(str::to_string),
                text: text.map(str::to_string),
                file: Some(format!("messages/{seq}-{label}.md")),
                source_seq: src,
                delivered: false,
                dropped: false,
                seen: false,
            }
        };
    let mut s = StudioState::default();
    s.attention
        .push(attention(7, "actor-t1-c1", PERMISSION_PROMPT, false));
    s.attention
        .push(attention(8, "actor-t1-c1", PERMISSION_PROMPT, false));
    let mut map: BTreeMap<String, StudioState> = [("acme".to_string(), s)].into();
    let errors = BTreeMap::new();
    let mut h = HollywoodState::default();

    let older_open = msg(1, "attention", Some("acme"), Some(7), None);
    let open = msg(2, "attention", Some("acme"), Some(8), None);
    assert_eq!(hnotice::stale_reason(&open, &map, &errors, &h), None);
    h.messages.push(older_open.clone());
    h.messages.push(open.clone());
    assert_eq!(
        hnotice::stale_reason(&older_open, &map, &errors, &h).as_deref(),
        Some("superseded by newer notice")
    );
    assert_eq!(hnotice::stale_reason(&open, &map, &errors, &h), None);

    let gone = msg(3, "attention", Some("acme"), Some(9), None);
    assert!(hnotice::stale_reason(&gone, &map, &errors, &h).is_some());
    let failed = msg(4, "failed", Some("acme"), None, Some("e1"));
    assert!(hnotice::stale_reason(&failed, &map, &errors, &h).is_some());

    map.get_mut("acme").unwrap().finished = Some(FinishRecord {
        seq: 9,
        reason: "done".into(),
        ts_ms: 0,
    });
    assert_eq!(
        hnotice::stale_reason(&open, &map, &errors, &h).as_deref(),
        Some("studio finished")
    );
    let finished = msg(5, "finished", Some("acme"), Some(9), None);
    assert_eq!(hnotice::stale_reason(&finished, &map, &errors, &h), None);

    hollywood::apply(&mut h, &tk::hev(1, "hollywood", tk::hpropose("t1")));
    hollywood::apply(
        &mut h,
        &tk::hev(2, "user", HollywoodKind::CardApplied { card: 1 }),
    );
    let applied = msg(6, "card", None, Some(2), None);
    assert_eq!(hnotice::stale_reason(&applied, &map, &errors, &h), None);
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
    assert_eq!(
        hnotice::stale_reason(&applied, &map, &errors, &h).as_deref(),
        Some("superseded by outcome")
    );
}

/// Labels read back off the file name; the rank puts stalled/failed/card
/// before prompts, attentions, finished, and recovered.
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
        seen: false,
    };
    assert_eq!(hnotice::label(&m("messages/3-prompt.md")), Some("prompt"));
    assert_eq!(hnotice::label(&m("messages/x.md")), None);
    assert_eq!(hnotice::label(&m("other/3-prompt.md")), None);
    let ranks: Vec<u8> = [
        "stalled",
        "failed",
        "card",
        "prompt",
        "attention",
        "finished",
        "recovered",
    ]
    .iter()
    .map(|l| hnotice::rank(&m(&format!("messages/1-{l}.md"))))
    .collect();
    assert_eq!(ranks, vec![0, 1, 2, 3, 4, 5, 6]);
}

#[test]
fn owed_filters_closed_info_and_card_applied() {
    let mut s = StudioState::default();
    let mut closed = attention(1, "producer", PERMISSION_PROMPT, true);
    closed.open = false;
    s.attention.push(closed);
    s.attention
        .push(attention(2, "producer", "session id not found", false));
    let map: BTreeMap<String, StudioState> = [("acme".to_string(), s)].into();

    let mut h = HollywoodState::default();
    hollywood::apply(&mut h, &tk::hev(1, "hollywood", tk::hpropose("t1")));
    hollywood::apply(
        &mut h,
        &tk::hev(2, "user", HollywoodKind::CardApplied { card: 1 }),
    );
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
    let n = hnotice::owed(&map, &BTreeMap::new(), &h, 0);
    assert!(
        n.is_empty(),
        "closed, info, card.applied, card.done must owe 0 wakeups, got {n:?}"
    );

    hollywood::apply(&mut h, &tk::hev(4, "hollywood", tk::hpropose("t2")));
    hollywood::apply(
        &mut h,
        &tk::hev(5, "user", HollywoodKind::CardApplied { card: 4 }),
    );
    hollywood::apply(
        &mut h,
        &tk::hev(
            6,
            "stage",
            HollywoodKind::CardFailed {
                card: 4,
                reason: "boom".into(),
            },
        ),
    );
    let n = hnotice::owed(&map, &BTreeMap::new(), &h, 0);
    assert_eq!(n.len(), 1, "only card.failed should wake Hollywood: {n:?}");
    assert_eq!(n[0].label, "card");
    assert_eq!(n[0].source_seq, Some(6));
}

#[test]
fn recovered_coalesces_and_drops_on_fresh_start() {
    let mut h = HollywoodState::default();
    let resumed = |reason: &str| HollywoodKind::RoleState {
        state: hollywood::RoleMotion::Resumed,
        terminal: Some("t1".into()),
        actor: None,
        session_id: Some("sess".into()),
        reason: Some(reason.into()),
    };
    let queued_recovered = |seq: u64, source_seq: u64| HMessage {
        seq,
        ts_ms: seq * 1000,
        from: "stage".into(),
        to: "hollywood".into(),
        studio: None,
        text: None,
        file: Some(format!("messages/{seq}-recovered.md")),
        source_seq: Some(source_seq),
        delivered: false,
        dropped: false,
        seen: false,
    };
    hollywood::apply(&mut h, &tk::hev(1, "stage", resumed("recovered")));
    h.messages.push(queued_recovered(10, 1));
    hollywood::apply(&mut h, &tk::hev(2, "stage", resumed("recovered")));
    h.messages.push(queued_recovered(11, 2));
    hollywood::apply(&mut h, &tk::hev(3, "stage", resumed("recovered")));
    h.messages.push(queued_recovered(12, 3));

    let map = BTreeMap::new();
    let errs = BTreeMap::new();
    let kept: Vec<u64> = h
        .messages
        .iter()
        .filter(|m| hnotice::stale_reason(m, &map, &errs, &h).is_none())
        .map(|m| m.seq)
        .collect();
    assert_eq!(
        kept,
        vec![12],
        "3 recovered notices must collapse to newest"
    );

    hollywood::apply(
        &mut h,
        &tk::hev(
            4,
            "stage",
            HollywoodKind::RoleState {
                state: hollywood::RoleMotion::Started,
                terminal: Some("t2".into()),
                actor: None,
                session_id: None,
                reason: Some("fresh".into()),
            },
        ),
    );
    assert!(
        hnotice::stale_reason(&h.messages[2], &map, &errs, &h).is_some(),
        "fresh RoleMotion::Started must drop surviving recovered notice"
    );
}

#[test]
fn passive_items_skips_dropped_notices_finished_studios_and_sorts_newest_first() {
    use backlot_model::event::{EventKind, Verdict};

    let finished_s = tk::state_with(
        &tk::base_events(),
        vec![
            ("director", tk::cast("t1", "actor-a", "a")),
            ("actor-a", tk::delivered("t1", 1, "a", 1)),
            ("director", tk::reviewed("t1", 1, Verdict::Accepted, vec![])),
            (
                "user",
                EventKind::StudioFinished {
                    reason: "done".into(),
                },
            ),
        ],
    );
    let live_s = tk::state_with(
        &tk::base_events(),
        vec![
            ("director", tk::cast("t1", "actor-a", "a")),
            ("actor-a", tk::delivered("t1", 1, "a", 1)),
            ("director", tk::reviewed("t1", 1, Verdict::Accepted, vec![])),
        ],
    );
    let mut map = BTreeMap::new();
    map.insert("done-shop".into(), finished_s);
    map.insert("live-shop".into(), live_s);

    // Pre-S2 dropped `accepted` for live-shop + dropped `card` for c1 -> both skipped;
    // un-notified c4 stays owed, sorted newest first.
    let h = hollywood::replay(&[
        tk::hev(1, "hollywood", tk::hpropose("t1")),
        tk::hev(2, "user", HollywoodKind::CardApplied { card: 1 }),
        tk::hev(
            3,
            "stage",
            HollywoodKind::CardDone {
                card: 1,
                note: String::new(),
            },
        ),
        tk::hev(
            4,
            "stage",
            HollywoodKind::MessageSent {
                to: "hollywood".into(),
                studio: None,
                text: None,
                file: Some("messages/4-card.md".into()),
                source_seq: Some(3),
            },
        ),
        tk::hev(
            5,
            "stage",
            HollywoodKind::MessageDropped {
                target: 4,
                reason: "superseded".into(),
            },
        ),
        tk::hev(
            6,
            "stage",
            HollywoodKind::MessageSent {
                to: "hollywood".into(),
                studio: Some("live-shop".into()),
                text: None,
                file: Some("messages/6-accepted.md".into()),
                source_seq: Some(6),
            },
        ),
        tk::hev(
            7,
            "stage",
            HollywoodKind::MessageDropped {
                target: 6,
                reason: "studio finished".into(),
            },
        ),
        tk::hev(8, "hollywood", tk::hpropose("t1")),
        tk::hev(9, "user", HollywoodKind::CardApplied { card: 8 }),
        tk::hev(
            10,
            "stage",
            HollywoodKind::CardDone {
                card: 8,
                note: String::new(),
            },
        ),
    ]);

    let items = hnotice::passive_items(&map, &h, &Default::default());
    let texts: Vec<&str> = items.iter().map(|i| i.text.as_str()).collect();
    assert_eq!(texts, vec!["c8 done"]);
}
