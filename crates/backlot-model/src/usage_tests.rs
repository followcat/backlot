//! Usage ledger fields (P4a): `Usage` on `task.delivered`, the
//! `UsageTotals` sums on state/records/summary, and the compact
//! display spellings. All fields optional, all sums factual (D8).

use serde_json::json;

use crate::event::*;
use crate::state::replay;
use crate::summary::summary;
use crate::testkit::*;

fn delivered_with(task: &str, n: u32, req: u32, usage: Usage) -> EventKind {
    EventKind::TaskDelivered {
        task: task.into(),
        delivery: n,
        actor: actor("a"),
        commits: "a..b".into(),
        report: format!("reports/{task}-d{n}.md"),
        requirement_version: req,
        checks: Vec::new(),
        usage: Some(usage),
    }
}

/// A `task.delivered` line without `usage` loads as `None`; writing
/// an event with `usage: None` emits no `usage` key at all — old
/// and new logs stay byte-compatible.
#[test]
fn usage_is_optional_and_omitted_when_absent() {
    let legacy = json!({
        "seq": 7, "ts_ms": 5, "by": {"role": "actor-t1-c1"},
        "kind": "task.delivered", "task": "t1", "delivery": 1,
        "actor": {"runner": "r", "model": "m", "permission_profile": "standard"},
        "commits": "a..b", "report": "reports/9-t1-d1.md",
        "requirement_version": 1
    });
    let parsed: Event = serde_json::from_value(legacy).unwrap();
    let EventKind::TaskDelivered { usage, .. } = &parsed.kind else {
        unreachable!()
    };
    assert_eq!(*usage, None);

    let bare = ev(9, "actor-t1-c1", delivered("t1", 1, "a", 1));
    let line = serde_json::to_value(&bare).unwrap();
    assert!(line.get("usage").is_none(), "no usage key: {line}");

    let used = ev(
        9,
        "actor-t1-c1",
        delivered_with(
            "t1",
            1,
            1,
            Usage {
                input_tokens: Some(12_400),
                output_tokens: None,
                cache_read_tokens: Some(2_000),
                cost_micros_usd: Some(80_000),
            },
        ),
    );
    let line = serde_json::to_value(&used).unwrap();
    assert_eq!(line["usage"]["input_tokens"], 12_400);
    assert_eq!(line["usage"]["cache_read_tokens"], 2_000);
    assert_eq!(line["usage"]["cost_micros_usd"], 80_000);
    assert!(line["usage"].get("output_tokens").is_none(), "{line}");
}

/// `UsageTotals` folds `Usage` in field-by-field; absent fields add
/// nothing and the empty totals report `is_empty`.
#[test]
fn totals_add_and_empty() {
    let mut t = UsageTotals::default();
    assert!(t.is_empty());
    assert_eq!(t.compact(), "");
    t.add(&Usage {
        input_tokens: Some(100),
        ..Usage::default()
    });
    t.add(&Usage {
        input_tokens: Some(50),
        output_tokens: Some(30),
        cost_micros_usd: Some(10_000),
        ..Usage::default()
    });
    assert!(!t.is_empty());
    assert_eq!(t.input_tokens, 150);
    assert_eq!(t.output_tokens, 30);
    assert_eq!(t.cache_read_tokens, 0);
    assert_eq!(t.cost_micros_usd, 10_000);
}

/// `tokens`/`cost` spell the compact display formats.
#[test]
fn token_and_cost_formatting() {
    for (n, want) in [
        (0, "0"),
        (950, "950"),
        (1_000, "1.0k"),
        (12_400, "12.4k"),
        (999_999, "999k"),
        (1_234_567, "1.2M"),
    ] {
        assert_eq!(UsageTotals::tokens(n), want, "{n}");
    }
    for (micros, want) in [
        (0, "$0"),
        (80_000, "$0.08"),
        (1_234_500, "$1.2345"),
        (5_000_000, "$5"),
        (1_000_001, "$1.000001"),
    ] {
        assert_eq!(UsageTotals::cost(micros), want, "{micros}");
    }
}

/// `compact`/`compact_dock` order segments in/out/cache/cost and
/// drop the ones that stayed zero.
#[test]
fn compact_spellings_skip_zero_segments() {
    let t = UsageTotals {
        input_tokens: 12_400,
        output_tokens: 3_100,
        cache_read_tokens: 2_000,
        cost_micros_usd: 80_000,
    };
    assert_eq!(t.compact(), "12.4k in / 3.1k out / 2.0k cache · $0.08");
    assert_eq!(t.compact_dock(), "12.4k/3.1k/2.0kc tok · $0.08");
    let t = UsageTotals {
        output_tokens: 3_100,
        ..UsageTotals::default()
    };
    assert_eq!(t.compact(), "3.1k out");
    assert_eq!(t.compact_dock(), "3.1k tok");
    let t = UsageTotals {
        cost_micros_usd: 5_000_000,
        ..UsageTotals::default()
    };
    assert_eq!(t.compact(), "$5");
    assert_eq!(t.compact_dock(), "$5");
}

/// The studio state's `usage` sums every delivery's figures across
/// tasks; a delivery without usage adds nothing.
#[test]
fn studio_state_sums_delivery_usage() {
    let mut events = base_events();
    events.push(ev(
        6,
        "actor-t1-c1",
        delivered_with(
            "t1",
            1,
            1,
            Usage {
                input_tokens: Some(10_000),
                output_tokens: Some(2_000),
                cost_micros_usd: Some(50_000),
                ..Usage::default()
            },
        ),
    ));
    // A task of another kind, delivered with no usage at all.
    events.push(ev(7, "director", task_created("t2", "chore", 1)));
    events.push(ev(8, "director", cast("t2", "actor-b", "b")));
    events.push(ev(9, "actor-t2-c1", delivered("t2", 1, "b", 1)));
    let state = replay(&events);
    assert_eq!(state.usage.input_tokens, 10_000);
    assert_eq!(state.usage.output_tokens, 2_000);
    assert_eq!(state.usage.cost_micros_usd, 50_000);
    // The records map the same figures onto the delivering actor —
    // actor-b's delivery carried none.
    let recs = crate::records::records(&[crate::records::StudioLog {
        studio: "acme-shop".into(),
        events: events.clone(),
    }]);
    let a = recs.iter().find(|r| r.actor == actor("a")).unwrap();
    assert_eq!(a.usage.input_tokens, 10_000);
    assert_eq!(a.usage.cost_micros_usd, 50_000);
    let b = recs.iter().find(|r| r.actor == actor("b")).unwrap();
    assert!(b.usage.is_empty());
}

/// The summary publishes `usage` only while a counter moved — a
/// usage-less studio serializes byte-identically to before.
#[test]
fn summary_omits_usage_until_a_counter_moves() {
    let events = base_events();
    let state = replay(&events);
    let doc = summary(
        &[("acme-shop".to_string(), state)],
        &Default::default(),
        &Default::default(),
        &Default::default(),
        &Default::default(),
        1,
    );
    assert_eq!(doc.studios[0].usage, None);
    let v = serde_json::to_value(&doc).unwrap();
    assert!(v["studios"][0].get("usage").is_none(), "{v}");

    let mut events = base_events();
    events.push(ev(
        6,
        "actor-t1-c1",
        delivered_with(
            "t1",
            1,
            1,
            Usage {
                input_tokens: Some(9_900),
                output_tokens: Some(1_100),
                ..Usage::default()
            },
        ),
    ));
    let state = replay(&events);
    let doc = summary(
        &[("acme-shop".to_string(), state)],
        &Default::default(),
        &Default::default(),
        &Default::default(),
        &Default::default(),
        1,
    );
    assert_eq!(
        doc.studios[0].usage,
        Some(UsageTotals {
            input_tokens: 9_900,
            output_tokens: 1_100,
            cache_read_tokens: 0,
            cost_micros_usd: 0,
        })
    );
}
