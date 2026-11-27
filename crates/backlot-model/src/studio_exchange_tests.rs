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
use crate::testkit::{base_events, base_state, cast, ev, role_started, state_with};

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
        Some("ask"),
        None,
    ));
    state.messages.push(msg(
        11,
        "producer",
        "user",
        Some("messages/11-q2.md"),
        Some("ask"),
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
            purpose: Some("ask".into()),
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
            purpose: Some("ask".into()),
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
            account: None,
            reset_at_ms: None,
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

/// `studio.finished` is the answering event for every open question
/// (L6): the producer's stale `tell user` never reads as still
/// waiting once the studio is finished.
#[test]
fn finishing_clears_the_open_question() {
    let mut events = base_events();
    events.push(ev(4, "producer", role_started("producer")));
    events.push(ev(
        10,
        "producer",
        EventKind::MessageQueued {
            to: "user".into(),
            file: Some("messages/10-q.md".into()),
            purpose: Some("ask".into()),
            source_seq: None,
        },
    ));
    // The requirement and the task land before the question, so the
    // finish has nothing open to refuse.
    events.push(ev(
        11,
        "producer",
        EventKind::StudioFinished {
            reason: "done".into(),
        },
    ));
    let state = replay(&events);
    let mut snippets = BTreeMap::new();
    snippets.insert(
        ("acme-shop".into(), 10),
        "what would you like us to build?".into(),
    );
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
    assert!(
        producer.asks.is_none(),
        "a finished studio shows no open question: {:?}",
        producer.asks
    );
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

#[test]
fn ask_answered_clears_asks_without_hiding_the_exchange_question() {
    let mut events = base_events();
    events.push(ev(4, "producer", role_started("producer")));
    events.push(ev(
        10,
        "producer",
        EventKind::MessageQueued {
            to: "user".into(),
            file: Some("messages/10-q.md".into()),
            purpose: Some("ask".into()),
            source_seq: None,
        },
    ));
    let mut snippets = BTreeMap::new();
    snippets.insert(("acme-shop".into(), 10), "what should we build?".into());

    let before = replay(&events);
    let sum_before = summary_with_exchange(
        &[("acme-shop".into(), before.clone())],
        &BTreeMap::new(),
        &BTreeMap::new(),
        &BTreeMap::new(),
        &BTreeMap::new(),
        &snippets,
        1,
    )
    .studios
    .remove(0);
    let prod_before = sum_before
        .roles
        .iter()
        .find(|r| r.role == "producer")
        .unwrap();
    assert_eq!(prod_before.asks.as_deref(), Some("what should we build?"));

    events.push(ev(
        11,
        "stage",
        EventKind::AskAnswered {
            role: "producer".into(),
            target: 10,
        },
    ));
    let after = replay(&events);
    assert_eq!(after.ask_answers.len(), 1);
    let sum_after = summary_with_exchange(
        &[("acme-shop".into(), after)],
        &BTreeMap::new(),
        &BTreeMap::new(),
        &BTreeMap::new(),
        &BTreeMap::new(),
        &snippets,
        2,
    )
    .studios
    .remove(0);
    let prod_after = sum_after
        .roles
        .iter()
        .find(|r| r.role == "producer")
        .unwrap();
    assert!(prod_after.asks.is_none(), "{:?}", prod_after.asks);
    assert_eq!(sum_after.exchange.len(), 1);
    assert_eq!(sum_after.exchange[0].seq, 10);
    assert_eq!(sum_after.exchange[0].text, "what should we build?");
}

#[test]
fn restart_recast_resume_and_progress_after_ask_do_not_clear_it() {
    let mut events = base_events();
    events.push(ev(4, "producer", role_started("producer")));
    events.push(ev(
        10,
        "producer",
        EventKind::MessageQueued {
            to: "user".into(),
            file: Some("messages/10-q.md".into()),
            purpose: Some("ask".into()),
            source_seq: None,
        },
    ));
    events.push(ev(
        11,
        "producer",
        EventKind::RoleProgress {
            role: "producer".into(),
            task: None,
            text: "waiting on answer".into(),
        },
    ));
    events.push(ev(
        12,
        "user",
        EventKind::RoleRestart {
            role: "producer".into(),
            fresh: false,
            reason: None,
        },
    ));
    events.push(ev(
        13,
        "user",
        EventKind::RoleRecast {
            role: "producer".into(),
            actor: crate::event::ActorIdentity {
                runner: "fake".into(),
                runner_version: None,
                model: "m".into(),
                effort: Some("high".into()),
                permission_profile: "standard".into(),
            },
            reason: Some("new model".into()),
            card: None,
        },
    ));
    events.push(ev(
        14,
        "stage",
        EventKind::RoleResumed {
            role: "producer".into(),
            terminal: Some("bl-acme-shop-producer".into()),
            session_id: Some("s-prod".into()),
            actor: None,
            reason: Some(crate::event::RECOVERED.into()),
        },
    ));

    let state = replay(&events);
    let mut snippets = BTreeMap::new();
    snippets.insert(("acme-shop".into(), 10), "what should we build?".into());
    let studio = summary_with_exchange(
        &[("acme-shop".into(), state.clone())],
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
    assert_eq!(producer.asks.as_deref(), Some("what should we build?"));
    assert_eq!(
        state.role_stage_touch_seq.get("producer").copied(),
        Some(14)
    );
}

#[test]
fn role_stage_touch_seq_tracks_terminal_writes_and_lifecycle_since_ask() {
    let mut events = base_events();
    events.push(ev(4, "producer", role_started("producer")));
    // Message queued BEFORE the ask (seq 5), then delivered AFTER the ask (seq 12).
    events.push(ev(
        5,
        "director",
        EventKind::MessageQueued {
            to: "producer".into(),
            file: Some("messages/5-hi.md".into()),
            purpose: Some("ask".into()),
            source_seq: None,
        },
    ));
    events.push(ev(
        10,
        "producer",
        EventKind::MessageQueued {
            to: "user".into(),
            file: Some("messages/10-q.md".into()),
            purpose: Some("ask".into()),
            source_seq: None,
        },
    ));
    events.push(ev(12, "stage", EventKind::MessageDelivered { target: 5 }));
    let state = replay(&events);
    assert_eq!(
        state.role_stage_touch_seq.get("producer").copied(),
        Some(12)
    );
}

#[test]
fn studio_exchange_populates_to_kind_task_and_ts_ms() {
    let mut state = state_with(
        &base_events(),
        vec![("director", cast("t1", "actor-a", "a"))],
    );
    // Non-task message with non-zero ts_ms.
    state.messages.push(msg(
        10,
        "user",
        "producer",
        Some("messages/10-tell.txt"),
        None,
        None,
    ));
    // Actor message with ts_ms == 0 (should omit ts_ms, populate task from recipient).
    let mut zero_ts = msg(
        11,
        "user",
        "actor-t1-c1",
        Some("messages/11-actor.txt"),
        None,
        None,
    );
    zero_ts.ts_ms = 0;
    state.messages.push(zero_ts);
    // Task-bound report (node is in state.tasks).
    state.reports.push(ReportRecord {
        seq: 12,
        from: "director".into(),
        to: "user".into(),
        node: "t1".into(),
        file: "reports/12-t1.md".into(),
        delivered: false,
        ts_ms: 1200,
        delivered_ms: None,
    });
    // Non-task report from non-actor role (milestone node not in state.tasks).
    state.reports.push(ReportRecord {
        seq: 13,
        from: "producer".into(),
        to: "user".into(),
        node: "milestone-1".into(),
        file: "reports/13-m1.md".into(),
        delivered: false,
        ts_ms: 1300,
        delivered_ms: None,
    });
    // Report from actor role whose node is not a task id falls back to actor's task.
    state.reports.push(ReportRecord {
        seq: 14,
        from: "actor-t1-c1".into(),
        to: "user".into(),
        node: "notes".into(),
        file: "reports/14-notes.md".into(),
        delivered: false,
        ts_ms: 0,
        delivered_ms: None,
    });

    let ex = studio_exchange(&state, &BTreeMap::new());
    assert_eq!(ex.len(), 5);

    assert_eq!(ex[0].seq, 10);
    assert_eq!(ex[0].to.as_deref(), Some("producer"));
    assert_eq!(ex[0].kind.as_deref(), Some("tell"));
    assert_eq!(ex[0].task, None);
    assert_eq!(ex[0].ts_ms, Some(1000));

    assert_eq!(ex[1].seq, 11);
    assert_eq!(ex[1].to.as_deref(), Some("actor-t1-c1"));
    assert_eq!(ex[1].kind.as_deref(), Some("tell"));
    assert_eq!(ex[1].task.as_deref(), Some("t1"));
    assert_eq!(ex[1].ts_ms, None);

    assert_eq!(ex[2].seq, 12);
    assert_eq!(ex[2].to.as_deref(), Some("user"));
    assert_eq!(ex[2].kind.as_deref(), Some("report"));
    assert_eq!(ex[2].task.as_deref(), Some("t1"));
    assert_eq!(ex[2].ts_ms, Some(1200));

    assert_eq!(ex[3].seq, 13);
    assert_eq!(ex[3].to.as_deref(), Some("user"));
    assert_eq!(ex[3].kind.as_deref(), Some("report"));
    assert_eq!(ex[3].task, None);
    assert_eq!(ex[3].ts_ms, Some(1300));

    assert_eq!(ex[4].seq, 14);
    assert_eq!(ex[4].to.as_deref(), Some("user"));
    assert_eq!(ex[4].kind.as_deref(), Some("report"));
    assert_eq!(ex[4].task.as_deref(), Some("t1"));
    assert_eq!(ex[4].ts_ms, None);

    // Wire serialization omits None fields.
    let v = serde_json::to_value(&ex[1]).unwrap();
    assert!(v.get("ts_ms").is_none());
    assert_eq!(v["to"], "actor-t1-c1");
    assert_eq!(v["kind"], "tell");
    assert_eq!(v["task"], "t1");
}
