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
    assert!(v.get("headline_seq").is_none());
    assert!(v.get("cards").is_none());
}

/// `can_answer_prompt` follows Hollywood's runner: false for a runner
/// with no approval key, true for cmd; a document written before the
/// field existed reads `true`.
#[test]
fn hollywood_can_answer_prompt_follows_the_runner() {
    let state = HollywoodState::default();
    let off = hollywood_section(&state, "live", Some("opencode"), Some("m"), None);
    assert!(!off.can_answer_prompt);
    let on = hollywood_section(&state, "live", Some("cmd"), Some("m"), None);
    assert!(on.can_answer_prompt);
    // No runner configured reads true — nothing to answer.
    let none = hollywood_section(&state, "down", None, None, None);
    assert!(none.can_answer_prompt);
    // The field is skipped when true, present when false.
    assert!(
        serde_json::to_value(&on)
            .unwrap()
            .get("can_answer_prompt")
            .is_none()
    );
    let v = serde_json::to_value(&off).unwrap();
    assert_eq!(v["can_answer_prompt"], false);
    let back: crate::hollywood_summary::HollywoodSummary = serde_json::from_value(v).unwrap();
    assert!(!back.can_answer_prompt);
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
                body: Some("reports/20-body.md".into()),
            },
        ),
    );
    for seq in 21..=28 {
        apply(
            &mut state,
            &hev(
                seq,
                "user",
                HollywoodKind::MessageSent {
                    to: "hollywood".into(),
                    studio: None,
                    text: Some("✦".repeat(300)),
                    file: None,
                    source_seq: None,
                },
            ),
        );
    }
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
    assert_eq!(s.headline_seq, Some(20));
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
                body: Some("reports/20-body.md".into()),
            },
        ),
    );
    for seq in 21..=28 {
        apply(
            &mut state,
            &hev(
                seq,
                "user",
                HollywoodKind::MessageSent {
                    to: "hollywood".into(),
                    studio: None,
                    text: Some("✦".repeat(300)),
                    file: None,
                    source_seq: None,
                },
            ),
        );
    }
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

/// The dialog's lines: a `tell` and a `reported` headline interleave
/// on their seqs, textless messages never show, and the cap keeps
/// the newest.
#[test]
fn the_exchange_is_the_conversation_newest_last() {
    let mut state = HollywoodState::default();
    let tell = |text: String| HollywoodKind::MessageSent {
        to: "hollywood".into(),
        studio: None,
        text: Some(text),
        file: None,
        source_seq: None,
    };
    apply(&mut state, &hev(2, "user", tell("first".into())));
    apply(
        &mut state,
        &hev(
            3,
            "hollywood",
            HollywoodKind::Reported {
                headline: "on it".into(),
                body: None,
            },
        ),
    );
    apply(
        &mut state,
        &hev(4, "user", tell("and stop the stage".into())),
    );
    // A notice without inline text (a file copy) never draws a line.
    apply(
        &mut state,
        &hev(
            5,
            "stage",
            HollywoodKind::MessageSent {
                to: "hollywood".into(),
                studio: None,
                text: None,
                file: Some("messages/5.txt".into()),
                source_seq: Some(9),
            },
        ),
    );
    for seq in 6..=20 {
        apply(&mut state, &hev(seq, "user", tell(format!("line {seq}"))));
    }
    let s = hollywood_section(&state, "live", None, None, None);
    let exchange = &s.exchange;
    assert_eq!(exchange.len(), crate::summary::MAX_EXCHANGE);
    // The newest lines won the cap; seqs ascend into the input line.
    assert_eq!(exchange.first().unwrap().seq, 13);
    assert_eq!(exchange.last().unwrap().seq, 20);
    assert_eq!(exchange[0].who, "user");
    assert_eq!(exchange[0].text, "line 13");
    // Order is the ledger's, so the answer sits between its tells.
    let small = hollywood_section(
        &{
            let mut state = HollywoodState::default();
            apply(&mut state, &hev(1, "user", tell("hi".into())));
            apply(
                &mut state,
                &hev(
                    2,
                    "hollywood",
                    HollywoodKind::Reported {
                        headline: "hi back".into(),
                        body: None,
                    },
                ),
            );
            apply(&mut state, &hev(3, "user", tell("go".into())));
            state
        },
        "live",
        None,
        None,
        None,
    );
    let lines: Vec<_> = small
        .exchange
        .iter()
        .map(|e| (e.who.as_str(), e.text.as_str()))
        .collect();
    assert_eq!(
        lines,
        [("user", "hi"), ("hollywood", "hi back"), ("user", "go")]
    );
}

/// Exchange lines keep up to `HEADLINE_CHARS` (200) characters intact
/// on both user tells and Hollywood report headlines, and truncate
/// cleanly past 200 without splitting multibyte UTF-8 scalar values.
#[test]
fn exchange_preserves_up_to_200_chars_and_truncates_cleanly_above() {
    use crate::summary::{HEADLINE_CHARS, Summary};

    let mut state = HollywoodState::default();
    let tell_97 = "a".repeat(97);
    let report_150 = "b".repeat(150);
    let tell_200 = "c".repeat(HEADLINE_CHARS);
    let report_200 = "d".repeat(HEADLINE_CHARS);
    let tell_over = format!("{}tail", "é".repeat(HEADLINE_CHARS));
    let report_over = format!("{}tail", "✦".repeat(HEADLINE_CHARS));

    let tell = |text: String| HollywoodKind::MessageSent {
        to: "hollywood".into(),
        studio: None,
        text: Some(text),
        file: None,
        source_seq: None,
    };
    apply(&mut state, &hev(1, "user", tell(tell_97.clone())));
    apply(
        &mut state,
        &hev(
            2,
            "hollywood",
            HollywoodKind::Reported {
                headline: report_150.clone(),
                body: None,
            },
        ),
    );
    apply(&mut state, &hev(3, "user", tell(tell_200.clone())));
    apply(
        &mut state,
        &hev(
            4,
            "hollywood",
            HollywoodKind::Reported {
                headline: report_200.clone(),
                body: Some("reports/4-detail.md".into()),
            },
        ),
    );
    apply(&mut state, &hev(5, "user", tell(tell_over)));
    apply(
        &mut state,
        &hev(
            6,
            "hollywood",
            HollywoodKind::Reported {
                headline: report_over,
                body: Some("reports/6-detail.md".into()),
            },
        ),
    );

    let s = hollywood_section(&state, "live", None, None, None);
    assert_eq!(s.headline_seq, Some(6));
    assert_eq!(s.exchange.len(), 6);
    assert_eq!(s.exchange[0].text, tell_97);
    assert!(!s.exchange[0].has_body);
    assert_eq!(s.exchange[1].text, report_150);
    assert!(!s.exchange[1].has_body);
    assert_eq!(s.exchange[2].text, tell_200);
    assert_eq!(s.exchange[3].text, report_200);
    assert!(s.exchange[3].has_body);
    assert_eq!(s.exchange[4].text, "é".repeat(HEADLINE_CHARS));
    assert_eq!(s.exchange[4].text.chars().count(), HEADLINE_CHARS);
    assert_eq!(s.exchange[5].text, "✦".repeat(HEADLINE_CHARS));
    assert_eq!(s.exchange[5].text.chars().count(), HEADLINE_CHARS);
    assert!(s.exchange[5].has_body);

    // Pre-existing wire documents without `headline_seq` or `has_body`
    // deserialize cleanly, and false `has_body` is omitted on the wire.
    let v = serde_json::to_value(&s).unwrap();
    assert!(v["exchange"][0].get("has_body").is_none());
    assert_eq!(v["exchange"][3]["has_body"], true);
    let legacy = serde_json::json!({
        "schema": SUMMARY_SCHEMA,
        "updated_ms": 1,
        "studios": [],
        "hollywood": {
            "state": "live",
            "headline": "hello",
            "exchange": [{ "seq": 1, "who": "user", "text": "hi" }]
        }
    });
    let parsed: Summary = serde_json::from_value(legacy).unwrap();
    let hw = parsed.hollywood.unwrap();
    assert_eq!(hw.headline_seq, None);
    assert_eq!(hw.work, None);
    assert!(!hw.pending);
    assert!(!hw.exchange[0].has_body);
}

#[test]
fn hollywood_work_and_pending_project_cleanly_and_compare_in_same_as() {
    use crate::summary::{Summary, Work, hollywood_section_with_work};

    let mut state = HollywoodState::default();
    let tell = HollywoodKind::MessageSent {
        to: "hollywood".into(),
        studio: None,
        text: Some("status check".into()),
        file: None,
        source_seq: None,
    };
    apply(&mut state, &hev(1, "user", tell));

    // Queued behind send gate: `pending` is true; `work` only publishes when `live`.
    let down = hollywood_section_with_work(&state, "down", None, None, None, Some(Work::Working));
    assert_eq!(down.work, None, "non-live role omits stale work");
    assert!(down.pending);

    let working =
        hollywood_section_with_work(&state, "live", None, None, None, Some(Work::Working));
    assert_eq!(working.work, Some(Work::Working));
    assert!(working.pending);

    // Delivered clears `pending`; `Waiting` serializes cleanly and skips false `pending`.
    apply(
        &mut state,
        &hev(2, "stage", HollywoodKind::MessageDelivered { target: 1 }),
    );
    let settled =
        hollywood_section_with_work(&state, "live", None, None, None, Some(Work::Waiting));
    assert_eq!(settled.work, Some(Work::Waiting));
    assert!(!settled.pending);
    let wire = serde_json::to_value(&settled).unwrap();
    assert_eq!(wire["work"], "waiting");
    assert!(wire.get("pending").is_none());

    // `Summary::same_as` detects work/pending flips without timestamp churn.
    let mut a = Summary {
        schema: SUMMARY_SCHEMA.into(),
        updated_ms: 10,
        studios: Vec::new(),
        hollywood: Some(settled.clone()),
        backlot_version: None,
        backlot_build: None,
        agent_skill: None,
        accounts: Vec::new(),
    };
    let mut b = a.clone();
    b.updated_ms = 9999;
    assert!(a.same_as(&b), "ticking updated_ms alone never churns");
    a.hollywood.as_mut().unwrap().work = Some(Work::Working);
    assert!(!a.same_as(&b), "work transition publishes immediately");

    // A `message.seen` tell does not stay `pending` on the wire, and v0.4.0
    // consumers still deserialize the emitted `HollywoodSummary`.
    #[derive(serde::Deserialize)]
    #[allow(dead_code)]
    struct V040HollywoodSummary {
        state: String,
        #[serde(default)]
        runner: Option<String>,
        #[serde(default)]
        model: Option<String>,
        #[serde(default)]
        effort: Option<String>,
        #[serde(default)]
        work: Option<Work>,
        #[serde(default)]
        pending: bool,
        #[serde(default)]
        doing: Option<String>,
        #[serde(default)]
        attention: Option<String>,
        #[serde(default)]
        attention_user: bool,
        #[serde(default)]
        attention_detail: Option<String>,
        #[serde(default)]
        headline: Option<String>,
        #[serde(default)]
        restarted_ms: Option<u64>,
        #[serde(default)]
        headline_seq: Option<u64>,
        #[serde(default)]
        cards: Vec<crate::hollywood_summary::CardSummary>,
        #[serde(default)]
        exchange: Vec<crate::hollywood_summary::Exchange>,
    }

    let mut seen_state = HollywoodState::default();
    apply(
        &mut seen_state,
        &hev(
            1,
            "user",
            HollywoodKind::MessageSent {
                to: "hollywood".into(),
                studio: None,
                text: Some("seen already".into()),
                file: None,
                source_seq: None,
            },
        ),
    );
    apply(
        &mut seen_state,
        &hev(
            2,
            "hollywood",
            HollywoodKind::MessageSeen {
                target: 1,
                by_seq: 2,
            },
        ),
    );
    let summary = hollywood_section_with_work(
        &seen_state,
        "live",
        Some("claude"),
        None,
        None,
        Some(Work::Waiting),
    );
    assert!(!summary.pending, "seen tell must not keep pending=true");
    let raw = serde_json::to_string(&summary).unwrap();
    let frozen: V040HollywoodSummary = serde_json::from_str(&raw).unwrap();
    assert!(!frozen.pending);
    assert_eq!(frozen.state, "live");
}

#[test]
fn hollywood_section_populates_exchange_and_card_metadata() {
    let mut state = HollywoodState::default();
    apply(&mut state, &hev(1, "hollywood", hpropose("t1")));
    apply(
        &mut state,
        &hev(
            2,
            "user",
            HollywoodKind::MessageSent {
                to: "hollywood".into(),
                studio: None,
                text: Some("status check".into()),
                file: None,
                source_seq: None,
            },
        ),
    );
    apply(
        &mut state,
        &hev(
            3,
            "hollywood",
            HollywoodKind::Reported {
                headline: "all green".into(),
                body: Some("reports/3.md".into()),
            },
        ),
    );
    // Zero timestamp omits ts_ms.
    let zero_msg = crate::hollywood::NewHollywoodEvent {
        by: by("user"),
        kind: HollywoodKind::MessageSent {
            to: "hollywood".into(),
            studio: None,
            text: Some("zero ts".into()),
            file: None,
            source_seq: None,
        },
    }
    .stamp(4, 0);
    apply(&mut state, &zero_msg);

    let s = hollywood_section(&state, "live", Some("agy"), Some("pro"), None);
    assert_eq!(s.cards.len(), 1);
    assert_eq!(s.cards[0].ts_ms, Some(1000));

    // The card Hollywood proposed (#1) leads the talk, then the tell
    // and the report.
    assert_eq!(s.exchange.len(), 4);
    assert_eq!(s.exchange[0].who, "hollywood");
    assert_eq!(s.exchange[0].kind.as_deref(), Some("card"));
    assert_eq!(
        s.exchange[0].text,
        "proposed card c1 · acme-shop · pause t1"
    );
    assert_eq!(s.exchange[1].to.as_deref(), Some("hollywood"));
    assert_eq!(s.exchange[1].kind.as_deref(), Some("tell"));
    assert_eq!(s.exchange[1].task, None);
    assert_eq!(s.exchange[1].ts_ms, Some(2000));

    assert_eq!(s.exchange[2].to.as_deref(), Some("user"));
    assert_eq!(s.exchange[2].kind.as_deref(), Some("report"));
    assert_eq!(s.exchange[2].task, None);
    assert_eq!(s.exchange[2].ts_ms, Some(3000));

    assert_eq!(s.exchange[3].ts_ms, None);

    let v = serde_json::to_value(&s).unwrap();
    assert!(v["exchange"][1].get("task").is_none());
    assert!(v["exchange"][2].get("task").is_none());
    assert!(v["exchange"][3].get("ts_ms").is_none());
    assert_eq!(v["cards"][0]["ts_ms"], 1000);
}

/// Hollywood's own `card.proposed` lines join the exchange — `who` is
/// `hollywood`, `kind` is `card`, and the text names the card and its
/// action in the CARD row's words. A card the stage proposed (an
/// `Outward` card) is not Hollywood's word and never draws a line.
#[test]
fn the_exchange_carries_hollywood_s_own_cards_only() {
    let mut state = HollywoodState::default();
    // #1: a card Hollywood proposed (a pause).
    apply(&mut state, &hev(1, "hollywood", hpropose("t1")));
    // #2: the person's line that asked for it.
    apply(
        &mut state,
        &hev(
            2,
            "user",
            HollywoodKind::MessageSent {
                to: "hollywood".into(),
                studio: None,
                text: Some("recast producer and director".into()),
                file: None,
                source_seq: None,
            },
        ),
    );
    // #3: a card the stage proposed (an outward push) — not Hollywood's.
    apply(
        &mut state,
        &hev(
            3,
            "stage",
            HollywoodKind::CardProposed {
                action: CardAction::Outward {
                    studio: "acme-shop".into(),
                    role: "actor-a".into(),
                    source_seq: 9,
                    argv: vec!["git".into(), "push".into(), "origin".into(), "HEAD".into()],
                    cwd: "/home/user/work".into(),
                    repo: "repo-a".into(),
                    remote_url: "git@host-a:acme/repo-a.git".into(),
                    rule: "recorded".into(),
                    commits: Vec::new(),
                    commits_total: 0,
                },
                why: "the actor asked".into(),
                base: CardBase::StudioAbsent,
            },
        ),
    );
    let s = hollywood_section(&state, "live", None, None, None);
    let lines: Vec<(&str, Option<&str>, &str)> = s
        .exchange
        .iter()
        .map(|e| (e.who.as_str(), e.kind.as_deref(), e.text.as_str()))
        .collect();
    assert_eq!(
        lines,
        [
            (
                "hollywood",
                Some("card"),
                "proposed card c1 · acme-shop · pause t1"
            ),
            ("user", Some("tell"), "recast producer and director"),
        ],
        "only Hollywood's card line joins the talk"
    );
    // The card line's text carries the same words the CARD row uses.
    assert_eq!(s.exchange[0].to.as_deref(), Some("user"));
    assert!(!s.exchange[0].has_body);
}
