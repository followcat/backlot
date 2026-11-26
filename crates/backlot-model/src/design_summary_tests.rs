//! Cast settings/diff and `StudioSummary.design` tests.

use std::collections::BTreeMap;

use crate::cast::{self, CastSettings};
use crate::design::{DesignGate, DesignPhase, DesignQuestion, DesignSlot, SignMode};
use crate::event::{Event, EventKind};
use crate::state::replay;
use crate::summary::{Summary, summary, summary_with_exchange};
use crate::testkit::*;

fn studio_required() -> EventKind {
    let mut k = studio_kind();
    if let EventKind::StudioCreated { design_gate, .. } = &mut k {
        *design_gate = DesignGate::Required;
    }
    k
}

fn req_ev(seq: u64, version: u32) -> Event {
    ev(
        seq,
        "producer",
        EventKind::ReqVersion {
            version,
            file: format!("req/v{version}.md"),
            summary: format!("req v{version}"),
        },
    )
}

fn opened(seq: u64, role: &str, version: u32, round: u32, suggest_skip: bool) -> Event {
    ev(
        seq,
        role,
        EventKind::DesignOpened {
            version,
            round,
            suggest_skip,
            reason: suggest_skip.then(|| "small touchup".to_string()),
            doc_file: format!("design/v{version}.md"),
        },
    )
}

fn entry(
    seq: u64,
    role: &str,
    version: u32,
    round: u32,
    slot: DesignSlot,
    questions: Vec<DesignQuestion>,
) -> Event {
    let tag = match slot {
        DesignSlot::Proposal => "proposal",
        DesignSlot::Buildability => "buildability",
        DesignSlot::Summary => "summary",
        DesignSlot::UserInput => "user_input",
        DesignSlot::Supplement => "supplement",
    };
    ev(
        seq,
        role,
        EventKind::DesignEntry {
            version,
            round,
            slot,
            file: format!("design/v{version}/r{round}-{seq}-{tag}.md"),
            source: None,
            refs: Vec::new(),
            questions,
        },
    )
}

fn q(id: &str, prompt: &str, rec: &str) -> DesignQuestion {
    DesignQuestion {
        id: id.into(),
        prompt: prompt.into(),
        recommendation: rec.into(),
        section_ref: Some("§R1.W".into()),
    }
}

#[test]
fn cast_settings_toml_and_diff_include_design_gate() {
    let state = replay(&base_events());
    let base = CastSettings::of(&state).unwrap();
    assert_eq!(base.design_gate, DesignGate::Off);

    let mut next = base.clone();
    next.design_gate = DesignGate::Required;
    assert!(!base.same_cast(&next));

    let toml_text = cast::to_toml(&next);
    assert!(
        toml_text.contains("design_gate = \"required\""),
        "{toml_text}"
    );
    let diff = cast::diff(&base, &next);
    let lines = diff.lines();
    assert!(
        lines.iter().any(|l| l == "design_gate off → required"),
        "{lines:?}"
    );
}

#[test]
fn summary_omits_inactive_design_places_in_asks_and_caps_fields() {
    let s_off = summary(
        &[("acme-shop".into(), base_state())],
        &BTreeMap::new(),
        &BTreeMap::new(),
        &BTreeMap::new(),
        &BTreeMap::new(),
        42,
    );
    assert_eq!(s_off.studios[0].design, None);
    let off_json = serde_json::to_string(&s_off).unwrap();
    assert!(!off_json.contains("\"design\""), "{off_json}");

    let before_open = replay(&[ev(1, "user", studio_required()), req_ev(2, 1)]);
    let s_idle = summary(
        &[("acme-shop".into(), before_open)],
        &BTreeMap::new(),
        &BTreeMap::new(),
        &BTreeMap::new(),
        &BTreeMap::new(),
        42,
    );
    assert_eq!(s_idle.studios[0].design, None);

    let long = "x".repeat(200);
    let waiting_state = replay(&[
        ev(1, "user", studio_required()),
        req_ev(2, 1),
        opened(3, "producer", 1, 1, false),
        entry(4, "writer", 1, 1, DesignSlot::Proposal, vec![]),
        entry(5, "director", 1, 1, DesignSlot::Buildability, vec![]),
        entry(
            6,
            "producer",
            1,
            1,
            DesignSlot::Summary,
            vec![
                q("q1", &long, &long),
                q("q2", &long, &long),
                q("q3", &long, &long),
                q("q4", &long, &long),
            ],
        ),
    ]);
    let snippets = BTreeMap::from([(
        ("acme-shop".to_string(), 6u64),
        format!("Writer recommends {long}"),
    )]);
    let doc: Summary = summary_with_exchange(
        &[("acme-shop".into(), waiting_state)],
        &BTreeMap::new(),
        &BTreeMap::new(),
        &BTreeMap::new(),
        &BTreeMap::new(),
        &snippets,
        42,
    );
    let st = &doc.studios[0];
    assert_eq!(st.waiting, 0, "DECISION counts in ?N, never !N");
    let ds = st.design.as_ref().expect("active design summary");
    assert_eq!(ds.version, 1);
    assert_eq!(ds.round, 1);
    assert_eq!(ds.phase, DesignPhase::WaitingUser);
    assert!(ds.waiting_user);
    assert_eq!(ds.waiting_since_ms, Some(6_000));
    assert_eq!(ds.questions.len(), 3);
    assert_eq!(ds.summary.as_ref().unwrap().chars().count(), 96);
    for item in &ds.questions {
        assert_eq!(item.prompt.chars().count(), 96);
        assert_eq!(item.recommendation.chars().count(), 96);
    }

    let mut signed_evs = vec![
        ev(1, "user", studio_required()),
        req_ev(2, 1),
        opened(3, "producer", 1, 1, false),
        entry(4, "writer", 1, 1, DesignSlot::Proposal, vec![]),
        entry(5, "director", 1, 1, DesignSlot::Buildability, vec![]),
        entry(
            6,
            "producer",
            1,
            1,
            DesignSlot::Summary,
            vec![q("q1", "Ship?", "Yes")],
        ),
        ev(
            7,
            "user",
            EventKind::DesignSigned {
                version: 1,
                round: 1,
                mode: SignMode::Approved,
                doc_file: "design/v1-final.md".into(),
                note: None,
            },
        ),
        ev(8, "director", task_created("t1", "feature", 1)),
        ev(9, "director", cast("t1", "actor-a", "a")),
    ];
    let slim_doc = summary_with_exchange(
        &[("acme-shop".into(), replay(&signed_evs))],
        &BTreeMap::new(),
        &BTreeMap::new(),
        &BTreeMap::new(),
        &BTreeMap::new(),
        &snippets,
        42,
    );
    let slim_ds = slim_doc.studios[0].design.as_ref().unwrap();
    assert_eq!(slim_ds.phase, DesignPhase::Signed);
    assert!(!slim_ds.waiting_user);
    assert_eq!(slim_ds.waiting_since_ms, None);
    assert_eq!(slim_ds.summary, None);
    assert!(slim_ds.questions.is_empty());

    let mut off_cast = CastSettings::of(&replay(&signed_evs)).unwrap();
    off_cast.design_gate = DesignGate::Off;
    signed_evs.push(ev(10, "user", off_cast.event_kind(None)));
    let turned_off = summary(
        &[("acme-shop".into(), replay(&signed_evs))],
        &BTreeMap::new(),
        &BTreeMap::new(),
        &BTreeMap::new(),
        &BTreeMap::new(),
        42,
    );
    assert_eq!(turned_off.studios[0].design, None);
}
