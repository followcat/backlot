//! Unit tests for `StudioSummary.exchange`: ordering, filtering out
//! internal notices and role-to-role mail, 200-char UTF-8 truncation,
//! wire backward-compatibility, and the 60 KiB document target.

use std::collections::BTreeMap;

use crate::event::EventKind;
use crate::state::{Message, ReportRecord, StudioState, replay};
use crate::summary::{
    HEADLINE_CHARS, MAX_STUDIO_EXCHANGE, SUMMARY_SCHEMA, Summary, studio_exchange,
    studio_exchange_files, summary_with_exchange,
};
use crate::testkit::{base_events, base_state, ev, role_started, state_with};

fn msg(
    seq: u64,
    from: &str,
    to: &str,
    file: Option<&str>,
    purpose: Option<&str>,
    source_seq: Option<u64>,
) -> Message {
    Message {
        seq,
        from: from.into(),
        to: to.into(),
        file: file.map(String::from),
        purpose: purpose.map(String::from),
        source_seq,
        delivered: false,
        dropped: false,
        ts_ms: seq * 100,
        delivered_ms: None,
    }
}

#[test]
fn studio_exchange_filters_internal_mail_and_orders_by_seq() {
    let mut state = base_state();
    // Included: user -> producer (#2).
    state.messages.push(msg(
        2,
        "user",
        "producer",
        Some("messages/2-tell.txt"),
        None,
        None,
    ));
    // Excluded: internal plan request (#3), dispatch (#4), stage notice (#5),
    // writer -> producer (#6), actor -> director (#7), director -> producer report (#8).
    state
        .messages
        .push(msg(3, "producer", "writer", None, Some("plan"), None));
    state.messages.push(msg(
        4,
        "producer",
        "director",
        Some("messages/4-dispatch.md"),
        Some("dispatch"),
        None,
    ));
    state.messages.push(msg(
        5,
        "stage",
        "producer",
        Some("messages/5-delivered.md"),
        None,
        Some(1),
    ));
    state.messages.push(msg(
        6,
        "writer",
        "producer",
        Some("messages/6-plan-ready.md"),
        None,
        None,
    ));
    state.messages.push(msg(
        7,
        "actor-t1-c1",
        "director",
        Some("messages/7-question.md"),
        None,
        None,
    ));
    state.reports.push(ReportRecord {
        seq: 8,
        from: "director".into(),
        to: "producer".into(),
        node: "integration".into(),
        file: "reports/8-integration.md".into(),
        delivered: false,
        ts_ms: 800,
        delivered_ms: None,
    });
    // Excluded: hollywood message with source_seq or purpose (#9, #10), or dropped (#11).
    state.messages.push(msg(
        9,
        "hollywood",
        "producer",
        Some("messages/9-notice.md"),
        None,
        Some(1),
    ));
    state.messages.push(msg(
        10,
        "hollywood",
        "producer",
        Some("messages/10-purpose.md"),
        Some("nudge"),
        None,
    ));
    let mut dropped = msg(
        11,
        "user",
        "producer",
        Some("messages/11-dropped.txt"),
        None,
        None,
    );
    dropped.dropped = true;
    state.messages.push(dropped);

    // Included: plain hollywood -> producer (#12), producer -> user tell (#13),
    // and producer -> user report (#14).
    state.messages.push(msg(
        12,
        "hollywood",
        "producer",
        Some("messages/12-overseer.md"),
        None,
        None,
    ));
    state.messages.push(msg(
        13,
        "producer",
        "user",
        Some("messages/13-reply.md"),
        None,
        None,
    ));
    state.reports.push(ReportRecord {
        seq: 14,
        from: "producer".into(),
        to: "user".into(),
        node: "milestone-1".into(),
        file: "reports/14-milestone-1.md".into(),
        delivered: false,
        ts_ms: 1400,
        delivered_ms: None,
    });

    let files = studio_exchange_files(&state);
    assert_eq!(
        files,
        vec![
            (2, "messages/2-tell.txt".into()),
            (12, "messages/12-overseer.md".into()),
            (13, "messages/13-reply.md".into()),
            (14, "reports/14-milestone-1.md".into()),
        ]
    );

    let mut snippets = BTreeMap::new();
    snippets.insert(2, "check the cart queue".into());
    snippets.insert(12, "prioritize checkout tests".into());
    snippets.insert(13, "queue is clean, 2 tasks left".into());
    snippets.insert(14, "Milestone 1 shipped".into());

    let ex = studio_exchange(&state, &snippets);
    let summary_rows: Vec<_> = ex
        .iter()
        .map(|e| (e.seq, e.who.as_str(), e.text.as_str(), e.file.as_deref()))
        .collect();
    assert_eq!(
        summary_rows,
        vec![
            (
                2,
                "user",
                "check the cart queue",
                Some("messages/2-tell.txt")
            ),
            (
                12,
                "hollywood",
                "prioritize checkout tests",
                Some("messages/12-overseer.md")
            ),
            (
                13,
                "producer",
                "queue is clean, 2 tasks left",
                Some("messages/13-reply.md")
            ),
            (
                14,
                "producer",
                "Milestone 1 shipped",
                Some("reports/14-milestone-1.md")
            ),
        ]
    );
}

#[test]
fn studio_exchange_caps_count_and_200_char_utf8_and_stays_under_budget() {
    let mut state = base_state();
    let mut snippets = BTreeMap::new();
    for seq in 1..=10u64 {
        state.messages.push(msg(
            seq,
            if seq % 2 == 0 { "producer" } else { "user" },
            if seq % 2 == 0 { "user" } else { "producer" },
            Some(&format!("messages/{seq}.md")),
            None,
            None,
        ));
    }
    let exact_200 = "é".repeat(HEADLINE_CHARS);
    let over_200 = format!("{}extra", "✦".repeat(HEADLINE_CHARS));
    snippets.insert(9, exact_200.clone());
    snippets.insert(10, over_200);

    let ex = studio_exchange(&state, &snippets);
    assert_eq!(ex.len(), MAX_STUDIO_EXCHANGE);
    assert_eq!(ex.first().unwrap().seq, 5);
    assert_eq!(ex.last().unwrap().seq, 10);
    // Missing snippet falls back cleanly to `from → to`.
    assert_eq!(ex[0].text, "user → producer");
    // 200 chars survive intact; >200 truncates on UTF-8 char boundaries.
    assert_eq!(ex[4].text, exact_200);
    assert_eq!(ex[5].text, "✦".repeat(HEADLINE_CHARS));
    assert_eq!(ex[5].text.chars().count(), HEADLINE_CHARS);

    // Old summary JSON without `exchange` parses cleanly; empty `exchange` omits key.
    let legacy = serde_json::json!({
        "schema": SUMMARY_SCHEMA,
        "updated_ms": 1,
        "studios": [{
            "id": "acme-shop",
            "title": "Acme shop",
            "state": "live",
            "requirement_version": 1,
            "mode": "standard",
            "open_reviews": 0,
            "waiting": 0
        }]
    });
    let parsed: Summary = serde_json::from_value(legacy).unwrap();
    assert!(parsed.studios[0].exchange.is_empty());

    // 12 studios (6 live + 6 finished) with full 200-char exchanges respect the 60 KiB target.
    let mut all_snippets = BTreeMap::new();
    let studios: Vec<(String, StudioState)> = (0..12)
        .map(|i| {
            let name = format!("studio-{i}");
            let mut s = state.clone();
            if i >= 6 {
                s.finished = Some(crate::state::FinishRecord {
                    seq: 99,
                    reason: "done".into(),
                    ts_ms: 1000,
                });
            }
            for seq in 5..=10u64 {
                all_snippets.insert((name.clone(), seq), "✦".repeat(HEADLINE_CHARS));
            }
            (name, s)
        })
        .collect();
    let doc = summary_with_exchange(
        &studios,
        &BTreeMap::new(),
        &BTreeMap::new(),
        &BTreeMap::new(),
        &BTreeMap::new(),
        &all_snippets,
        1,
    );
    let bytes = serde_json::to_vec(&doc).unwrap().len();
    assert!(bytes < 60_000, "summary grew to {bytes} bytes");
}

/// A role's newest `tell user` lands as `asks`; the user's `tell`
/// back to that role answers it.
#[test]
fn role_asks_tracks_the_newest_unanswered_tell_user() {
    let mut state = state_with(&base_events(), vec![("producer", role_started("producer"))]);
    state.messages.push(msg(
        10,
        "producer",
        "user",
        Some("messages/10-q.md"),
        None,
        None,
    ));
    state.messages.push(msg(
        11,
        "producer",
        "user",
        Some("messages/11-q2.md"),
        None,
        None,
    ));
    let mut snippets = BTreeMap::new();
    snippets.insert(("acme-shop".into(), 10), "first question".into());
    snippets.insert(("acme-shop".into(), 11), "what should we build?".into());
    let build = |s: &StudioState| {
        summary_with_exchange(
            &[("acme-shop".into(), s.clone())],
            &BTreeMap::new(),
            &BTreeMap::new(),
            &BTreeMap::new(),
            &BTreeMap::new(),
            &snippets,
            1,
        )
        .studios
        .remove(0)
    };
    let studio = build(&state);
    let producer = studio
        .roles
        .iter()
        .find(|r| r.role == "producer")
        .expect("the producer's row");
    assert_eq!(producer.asks.as_deref(), Some("what should we build?"));

    state.messages.push(msg(
        12,
        "user",
        "producer",
        Some("messages/12-a.md"),
        None,
        None,
    ));
    let studio = build(&state);
    let producer = studio.roles.iter().find(|r| r.role == "producer").unwrap();
    assert!(producer.asks.is_none(), "{:?}", producer.asks);
}

/// A ledger action the role records afterwards answers the question —
/// the person answered in the role's terminal, not with a `tell`.
#[test]
fn role_asks_clears_when_the_role_acts_after_it() {
    let mut events = base_events();
    events.push(ev(4, "producer", role_started("producer")));
    events.push(ev(
        10,
        "producer",
        EventKind::MessageQueued {
            to: "user".into(),
            file: Some("messages/10-q.md".into()),
            purpose: None,
            source_seq: None,
        },
    ));
    events.push(ev(
        11,
        "producer",
        EventKind::ReqVersion {
            version: 2,
            file: "req/v2.md".into(),
            summary: "second cut".into(),
        },
    ));
    let state = replay(&events);
    let mut snippets = BTreeMap::new();
    snippets.insert(("acme-shop".into(), 10), "which shop?".into());
    let studio = summary_with_exchange(
        &[("acme-shop".into(), state)],
        &BTreeMap::new(),
        &BTreeMap::new(),
        &BTreeMap::new(),
        &BTreeMap::new(),
        &snippets,
        1,
    )
    .studios
    .remove(0);
    let producer = studio.roles.iter().find(|r| r.role == "producer").unwrap();
    assert!(producer.asks.is_none(), "{:?}", producer.asks);
}

/// A stage-written event — an attention raised on the role — is not
/// the role answering, so the question stays.
#[test]
fn a_stage_written_event_does_not_answer_the_role() {
    let mut events = base_events();
    events.push(ev(4, "producer", role_started("producer")));
    events.push(ev(
        10,
        "producer",
        EventKind::MessageQueued {
            to: "user".into(),
            file: Some("messages/10-q.md".into()),
            purpose: None,
            source_seq: None,
        },
    ));
    events.push(ev(
        11,
        "stage",
        EventKind::AttentionRaised {
            role: "producer".into(),
            reason: "session id not found".into(),
            detail: "still looking".into(),
            needs_user: false,
        },
    ));
    let state = replay(&events);
    let mut snippets = BTreeMap::new();
    snippets.insert(("acme-shop".into(), 10), "which shop?".into());
    let studio = summary_with_exchange(
        &[("acme-shop".into(), state)],
        &BTreeMap::new(),
        &BTreeMap::new(),
        &BTreeMap::new(),
        &BTreeMap::new(),
        &snippets,
        1,
    )
    .studios
    .remove(0);
    let producer = studio.roles.iter().find(|r| r.role == "producer").unwrap();
    assert_eq!(producer.asks.as_deref(), Some("which shop?"));
}

#[test]
fn studio_pending_tell_tracks_undelivered_user_messages_only() {
    let mut state = base_state();
    // Internal producer -> director message does not set `pending_tell`.
    state.messages.push(msg(
        2,
        "producer",
        "director",
        Some("messages/2-dispatch.md"),
        Some("dispatch"),
        None,
    ));
    // Producer -> user reply (`to == "user"`) never sits behind a terminal send gate.
    state.messages.push(msg(
        3,
        "producer",
        "user",
        Some("messages/3-reply.md"),
        None,
        None,
    ));
    let build = |s: &StudioState| {
        summary_with_exchange(
            &[("acme-shop".into(), s.clone())],
            &BTreeMap::new(),
            &BTreeMap::new(),
            &BTreeMap::new(),
            &BTreeMap::new(),
            &BTreeMap::new(),
            1,
        )
        .studios
        .remove(0)
    };
    let calm = build(&state);
    assert!(!calm.pending_tell);
    assert!(
        serde_json::to_value(&calm)
            .unwrap()
            .get("pending_tell")
            .is_none()
    );

    // User -> producer tell waiting behind send gate sets `pending_tell = true`.
    state.messages.push(msg(
        4,
        "user",
        "producer",
        Some("messages/4-tell.txt"),
        None,
        None,
    ));
    let queued = build(&state);
    assert!(queued.pending_tell);
    assert_eq!(serde_json::to_value(&queued).unwrap()["pending_tell"], true);

    // Delivery clears `pending_tell` automatically.
    state.messages.last_mut().unwrap().delivered = true;
    assert!(!build(&state).pending_tell);
}
