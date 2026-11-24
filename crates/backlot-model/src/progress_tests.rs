//! Unit tests for `role.progress` (D5): normalization, role gates,
//! ledger reduction, zero obligations, and `RoleSummary` projections.

use std::collections::BTreeMap;

use crate::event::{
    Event, EventBy, EventKind, LEDGER_VERSION, MAX_PROGRESS_CHARS, MAX_RUNNING_CHARS, NewEvent,
    age_short, normalize_progress_text,
};
use crate::hollywood::{HOLLYWOOD, HollywoodEvent, HollywoodKind, NewHollywoodEvent};
use crate::hollywood_state::replay as hreplay;
use crate::hollywood_validate::validate as hvalidate;
use crate::refusal::Refusal;
use crate::state::replay;
use crate::summary::{
    RoleProgress, RoleRunning, RoleSummary, SUMMARY_SCHEMA, Summary, Work, summary,
    summary_with_running,
};
use crate::testkit::*;
use crate::validate::validate;

#[test]
fn normalize_strips_control_collapses_lines_and_caps_at_200() {
    assert_eq!(
        normalize_progress_text("  running\x07 the\n\tparity\rsuite\x1b  "),
        "running the parity suite"
    );
    let clean = normalize_progress_text("  step 1\x00\nstep\t2\x7f  ");
    assert_eq!(clean, "step 1 step 2");
    assert!(normalize_progress_text("\n\t\x00\x07  ").is_empty());

    let long = "a".repeat(250);
    let capped = normalize_progress_text(&long);
    assert_eq!(capped.chars().count(), MAX_PROGRESS_CHARS);
}

#[test]
fn role_progress_validates_for_studio_roles_and_refuses_others() {
    let mut base = base_events();
    base.push(ev(4, "director", cast("t1", "actor-a", "a")));
    let state = replay(&base);

    for (caller, task) in [
        ("producer", None),
        ("director", None),
        ("writer", None),
        ("actor-t1-c1", Some("t1")),
        ("actor-t1-c1", None),
    ] {
        let ev = NewEvent::new(
            EventBy::role(caller),
            EventKind::RoleProgress {
                role: caller.into(),
                task: task.map(String::from),
                text: "checking tests".into(),
            },
        );
        assert_eq!(validate(&state, &ev), Ok(()), "caller {caller}");
    }

    // Bare `actor`, `user`, `stage`, and `hollywood` cannot record onto a studio ledger.
    let bare = NewEvent::new(
        EventBy::role("actor"),
        EventKind::RoleProgress {
            role: "actor".into(),
            task: Some("t1".into()),
            text: "x".into(),
        },
    );
    assert_eq!(validate(&state, &bare), Err(Refusal::BareActor));

    for bad_caller in ["user", "stage", HOLLYWOOD] {
        let ev = NewEvent::new(
            EventBy::role(bad_caller),
            EventKind::RoleProgress {
                role: bad_caller.into(),
                task: None,
                text: "x".into(),
            },
        );
        assert!(validate(&state, &ev).is_err(), "must refuse {bad_caller}");
    }

    // Actor reporting for another task or spoofing role fails.
    let wrong_task = NewEvent::new(
        EventBy::role("actor-t1-c1"),
        EventKind::RoleProgress {
            role: "actor-t1-c1".into(),
            task: Some("t2".into()),
            text: "x".into(),
        },
    );
    assert!(validate(&state, &wrong_task).is_err());

    let spoof = NewEvent::new(
        EventBy::role("director"),
        EventKind::RoleProgress {
            role: "producer".into(),
            task: None,
            text: "x".into(),
        },
    );
    assert!(validate(&state, &spoof).is_err());

    // Empty, control chars, or >200 chars are refused at append time.
    for bad_text in ["", "a\nb", "a\x07b", &"x".repeat(MAX_PROGRESS_CHARS + 1)] {
        let ev = NewEvent::new(
            EventBy::role("producer"),
            EventKind::RoleProgress {
                role: "producer".into(),
                task: None,
                text: bad_text.into(),
            },
        );
        assert!(validate(&state, &ev).is_err(), "must refuse {bad_text:?}");
    }
}

#[test]
fn role_progress_reduces_without_adding_obligations() {
    let mut base = base_events();
    base.push(ev(4, "director", cast("t1", "actor-a", "a")));
    base.push(ev(
        5,
        "stage",
        EventKind::RoleStarted {
            role: "actor-t1-c1".into(),
            terminal: Some("t7".into()),
            actor: Some(actor("a")),
            session_id: Some("s1".into()),
            reason: None,
        },
    ));
    let mut events = base.clone();
    events.push(Event {
        v: LEDGER_VERSION,
        seq: 6,
        ts_ms: 9_000,
        by: EventBy::role("actor-t1-c1"),
        kind: EventKind::RoleProgress {
            role: "actor-t1-c1".into(),
            task: Some("t1".into()),
            text: "running parity suite".into(),
        },
    });
    let state = replay(&events);
    let expected = RoleProgress {
        text: "running parity suite".into(),
        ts_ms: 9_000,
    };
    assert_eq!(state.progress.get("actor-t1-c1"), Some(&expected));
    assert_eq!(
        state.roles["actor-t1-c1"].progress.as_ref(),
        Some(&expected)
    );
    assert_eq!(state.last_by_role["actor-t1-c1"].kind, "role.progress");
    assert!(state.open_attention().next().is_none());

    // Absence of progress raises no attentions and changes no owed steps.
    let baseline = replay(&base);
    assert!(baseline.progress.is_empty());
    assert!(baseline.open_attention().next().is_none());
    assert_eq!(
        crate::steps::next_steps(&baseline),
        crate::steps::next_steps(&state)
    );
}

#[test]
fn summary_populates_progress_and_working_only_running_capped_at_60() {
    let mut base = base_events();
    base.push(ev(4, "director", cast("t1", "actor-a", "a")));
    base.push(Event {
        v: LEDGER_VERSION,
        seq: 5,
        ts_ms: 1_000,
        by: EventBy::role("stage"),
        kind: EventKind::RoleStarted {
            role: "producer".into(),
            terminal: Some("t1".into()),
            actor: Some(actor("prod")),
            session_id: None,
            reason: None,
        },
    });
    base.push(Event {
        v: LEDGER_VERSION,
        seq: 6,
        ts_ms: 2_000,
        by: EventBy::role("stage"),
        kind: EventKind::RoleStarted {
            role: "actor-t1-c1".into(),
            terminal: Some("t2".into()),
            actor: Some(actor("a")),
            session_id: None,
            reason: None,
        },
    });
    base.push(Event {
        v: LEDGER_VERSION,
        seq: 7,
        ts_ms: 12_000,
        by: EventBy::role("actor-t1-c1"),
        kind: EventKind::RoleProgress {
            role: "actor-t1-c1".into(),
            task: Some("t1".into()),
            text: "running the parity suite".into(),
        },
    });
    let state = replay(&base);

    let work = BTreeMap::from([
        (
            ("acme-shop".to_string(), "actor-t1-c1".to_string()),
            Work::Working,
        ),
        (
            ("acme-shop".to_string(), "producer".to_string()),
            Work::Waiting,
        ),
    ]);
    let long_cmd = format!("cargo test --workspace --filter {}", "x".repeat(80));
    let running = BTreeMap::from([
        (
            ("acme-shop".to_string(), "actor-t1-c1".to_string()),
            RoleRunning {
                cmd: long_cmd,
                since_ms: 15_000,
            },
        ),
        (
            ("acme-shop".to_string(), "producer".to_string()),
            RoleRunning {
                cmd: "should not appear".into(),
                since_ms: 15_000,
            },
        ),
    ]);

    let s = summary_with_running(
        &[("acme-shop".into(), state)],
        &BTreeMap::new(),
        &work,
        &BTreeMap::new(),
        &BTreeMap::new(),
        &BTreeMap::new(),
        &running,
        60_000,
    );
    assert_eq!(s.schema, SUMMARY_SCHEMA);
    let actor_row = s.studios[0]
        .roles
        .iter()
        .find(|r| r.role == "actor-t1-c1")
        .unwrap();
    assert_eq!(
        actor_row.progress,
        Some(RoleProgress {
            text: "running the parity suite".into(),
            ts_ms: 12_000,
        })
    );
    let d = actor_row
        .running
        .as_ref()
        .expect("working role has running");
    assert_eq!(d.since_ms, 15_000);
    assert_eq!(d.cmd.chars().count(), MAX_RUNNING_CHARS);
    assert!(d.cmd.starts_with("cargo test --workspace"));

    let prod_row = s.studios[0]
        .roles
        .iter()
        .find(|r| r.role == "producer")
        .unwrap();
    assert_eq!(prod_row.running, None, "non-working role must omit running");

    // Empty running map omits running cleanly via `summary`.
    let plain = summary(
        &[],
        &BTreeMap::new(),
        &BTreeMap::new(),
        &BTreeMap::new(),
        &BTreeMap::new(),
        1,
    );
    assert_eq!(plain.schema, "backlot/1");
}

#[test]
fn old_summaries_without_progress_or_doing_and_legacy_doing_string_still_parse() {
    let old = r#"{
        "schema": "backlot/1",
        "updated_ms": 7,
        "studios": [{
            "id": "acme-shop",
            "title": "acme shop",
            "state": "live",
            "requirement_version": 1,
            "mode": "standard",
            "open_reviews": 0,
            "waiting": 0,
            "roles": [
                { "role": "producer", "name": "producer", "state": "live" },
                { "role": "director", "name": "director", "state": "live", "doing": "watching", "since_ms": 3 }
            ]
        }]
    }"#;
    let parsed: Summary = serde_json::from_str(old).unwrap();
    let roles = &parsed.studios[0].roles;
    assert_eq!(roles[0].progress, None);
    assert_eq!(roles[0].running, None);
    assert_eq!(roles[0].doing, None);
    assert_eq!(roles[1].progress, None);
    assert_eq!(roles[1].running, None);
    assert_eq!(roles[1].doing.as_deref(), Some("watching"));

    // Structured `progress` and `running` round-trip back faithfully alongside `doing`.
    let role: RoleSummary = serde_json::from_str(
        r#"{
            "role": "actor-t1-c1",
            "name": "alice",
            "state": "live",
            "doing": "writes code",
            "work": "working",
            "since_ms": 1000,
            "progress": { "text": "running the parity suite", "ts_ms": 5000 },
            "running": { "cmd": "cargo test", "since_ms": 8000 }
        }"#,
    )
    .unwrap();
    assert_eq!(role.doing.as_deref(), Some("writes code"));
    assert_eq!(
        role.progress.as_ref().map(|p| p.text.as_str()),
        Some("running the parity suite")
    );
    assert_eq!(
        role.running.as_ref().map(|d| d.cmd.as_str()),
        Some("cargo test")
    );
    assert_eq!(role.running.as_ref().map(|d| d.since_ms), Some(8000));
}

#[test]
fn hollywood_role_progress_validates_and_reduces() {
    let state = hreplay(&[]);
    let ok_ev = NewHollywoodEvent::new(
        EventBy::role(HOLLYWOOD),
        HollywoodKind::RoleProgress {
            text: "checking studio health".into(),
        },
    );
    assert_eq!(hvalidate(&state, &ok_ev), Ok(()));
    let stamped = HollywoodEvent {
        v: LEDGER_VERSION,
        seq: 1,
        ts_ms: 4_000,
        by: EventBy::role(HOLLYWOOD),
        kind: ok_ev.kind.clone(),
    };
    let reduced = hreplay(&[stamped]);
    assert_eq!(
        reduced.progress,
        Some(RoleProgress {
            text: "checking studio health".into(),
            ts_ms: 4_000,
        })
    );
    assert!(
        hvalidate(
            &state,
            &NewHollywoodEvent::new(
                EventBy::role("user"),
                HollywoodKind::RoleProgress {
                    text: "nope".into(),
                }
            )
        )
        .is_err()
    );
    assert_eq!(age_short(47 * 60_000), "47m");
}

#[derive(Debug, serde::Deserialize)]
struct FrozenV030Summary {
    #[allow(dead_code)]
    schema: String,
    studios: Vec<FrozenV030StudioSummary>,
}

#[derive(Debug, serde::Deserialize)]
struct FrozenV030StudioSummary {
    roles: Vec<FrozenV030RoleSummary>,
}

#[allow(dead_code)]
#[derive(Debug, serde::Deserialize)]
struct FrozenV030RoleSummary {
    role: String,
    name: String,
    #[serde(default)]
    terminal: Option<String>,
    #[serde(default)]
    runner: Option<String>,
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    effort: Option<String>,
    state: String,
    #[serde(default)]
    doing: Option<String>,
    #[serde(default)]
    work: Option<Work>,
    #[serde(default)]
    failed: Option<String>,
    #[serde(default)]
    workdir: Option<String>,
    #[serde(default)]
    task: Option<String>,
    #[serde(default)]
    priority: Option<crate::event::Priority>,
    #[serde(default)]
    resumable: bool,
    #[serde(default)]
    since_ms: u64,
    #[serde(default)]
    attention: Option<String>,
    #[serde(default)]
    attention_class: Option<crate::attention::AttentionClass>,
    #[serde(default)]
    asks: Option<String>,
    #[serde(default)]
    attention_user: bool,
    #[serde(default)]
    attention_seq: Option<u64>,
    #[serde(default)]
    attention_detail: Option<String>,
    #[serde(default)]
    stage_answer_ms: Option<u64>,
    #[serde(default = "default_true_for_frozen")]
    can_answer_prompt: bool,
    #[serde(default)]
    restarted_ms: Option<u64>,
}

fn default_true_for_frozen() -> bool {
    true
}

#[test]
fn frozen_v030_summary_reads_new_document_with_legacy_doing_string() {
    let mut events = base_events();
    events.extend([
        ev(
            7,
            "stage",
            EventKind::RoleStarted {
                role: "producer".into(),
                terminal: Some("t1".into()),
                actor: Some(actor("prod")),
                session_id: None,
                reason: None,
            },
        ),
        ev(
            8,
            "producer",
            EventKind::RoleProgress {
                role: "producer".into(),
                task: None,
                text: "running the parity suite".into(),
            },
        ),
    ]);
    let state = replay(&events);
    let live = BTreeMap::from([("t1".to_string(), "writes code".to_string())]);
    let work = BTreeMap::from([(
        ("acme-shop".to_string(), "producer".to_string()),
        Work::Working,
    )]);
    let proc_cmd = BTreeMap::from([(
        ("acme-shop".to_string(), "producer".to_string()),
        RoleRunning {
            cmd: "cargo test".into(),
            since_ms: 5_000,
        },
    )]);
    let s = summary_with_running(
        &[("acme-shop".into(), state)],
        &live,
        &work,
        &BTreeMap::new(),
        &BTreeMap::new(),
        &BTreeMap::new(),
        &proc_cmd,
        60_000,
    );
    let wire = serde_json::to_string(&s).unwrap();
    let legacy: FrozenV030Summary =
        serde_json::from_str(&wire).expect("v0.3.0 RoleSummary must parse new summary wire format");
    assert_eq!(
        legacy.studios[0].roles[0].doing.as_deref(),
        Some("writes code")
    );
}
