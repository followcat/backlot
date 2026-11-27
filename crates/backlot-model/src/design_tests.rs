//! Opening design model tests: reduce chain, req version reset, JSONL
//! round-trip, and backward compatibility.

use crate::design::{DesignGate, DesignPhase, DesignQuestion, DesignSlot, EntrySource, SignMode};
use crate::event::{Event, EventKind};
use crate::state::{StudioState, replay};
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
    source: Option<EntrySource>,
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
            source,
            refs: Vec::new(),
            questions,
            checks: Vec::new(),
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
fn full_reduce_chain_through_round_2_to_approved() {
    let mut events = vec![ev(1, "user", studio_required()), req_ev(2, 1)];
    let s0 = replay(&events);
    assert!(s0.design.is_none());
    assert_eq!(s0.design_phase(), DesignPhase::Idle);

    events.push(opened(3, "producer", 1, 1, false));
    let s1 = replay(&events);
    let d1 = s1.design.as_ref().expect("opened creates design state");
    assert_eq!(d1.phase(), DesignPhase::WaitingProposal);
    assert_eq!(d1.current_round_num(), 1);
    assert_eq!(d1.doc_file, "design/v1.md");
    assert_eq!(d1.waiting_since_ms, None);

    events.push(entry(4, "writer", 1, 1, DesignSlot::Proposal, None, vec![]));
    assert_eq!(
        replay(&events).design_phase(),
        DesignPhase::WaitingBuildability
    );

    events.push(entry(
        5,
        "director",
        1,
        1,
        DesignSlot::Buildability,
        None,
        vec![],
    ));
    assert_eq!(replay(&events).design_phase(), DesignPhase::WaitingSummary);

    events.push(entry(
        6,
        "producer",
        1,
        1,
        DesignSlot::Summary,
        None,
        vec![
            q("q1", "Pick A or B?", "Option A"),
            q("q2", "Run unit tests?", "Yes"),
        ],
    ));
    let s_wait = replay(&events);
    let d_wait = s_wait.design.as_ref().unwrap();
    assert_eq!(d_wait.phase(), DesignPhase::WaitingUser);
    assert_eq!(d_wait.waiting_since_ms, Some(6_000));
    assert_eq!(d_wait.open_questions().len(), 2);
    assert_eq!(d_wait.latest_summary_seq(), Some(6));
    assert!(!d_wait.has_user_input_after_last_summary());

    events.push(entry(
        7,
        "user",
        1,
        1,
        DesignSlot::UserInput,
        Some(EntrySource::Tell),
        vec![],
    ));
    let s_input = replay(&events);
    let d_input = s_input.design.as_ref().unwrap();
    assert_eq!(d_input.phase(), DesignPhase::WaitingUser);
    assert!(d_input.has_user_input_after_last_summary());

    // Round 2
    events.push(opened(8, "producer", 1, 2, false));
    let s_r2 = replay(&events);
    let d_r2 = s_r2.design.as_ref().unwrap();
    assert_eq!(d_r2.phase(), DesignPhase::WaitingProposal);
    assert_eq!(d_r2.current_round_num(), 2);
    assert_eq!(d_r2.waiting_since_ms, None);
    assert!(d_r2.open_questions().is_empty());
    assert!(!d_r2.has_user_input_after_last_summary());
    assert_eq!(d_r2.rounds.len(), 2);

    events.push(entry(9, "writer", 1, 2, DesignSlot::Proposal, None, vec![]));
    events.push(entry(
        10,
        "director",
        1,
        2,
        DesignSlot::Buildability,
        None,
        vec![],
    ));
    events.push(entry(
        11,
        "producer",
        1,
        2,
        DesignSlot::Summary,
        None,
        vec![q("q1", "Final check?", "Yes")],
    ));
    events.push(entry(
        12,
        "writer",
        1,
        2,
        DesignSlot::Supplement,
        None,
        vec![],
    ));
    assert_eq!(replay(&events).design_phase(), DesignPhase::WaitingUser);

    events.push(ev(
        13,
        "user",
        EventKind::DesignSigned {
            version: 1,
            round: 2,
            mode: SignMode::Approved,
            doc_file: "design/v1-final.md".into(),
            note: Some("approved for dispatch".into()),
        },
    ));
    let s_done = replay(&events);
    let d_done = s_done.design.as_ref().unwrap();
    assert_eq!(d_done.phase(), DesignPhase::Signed);
    assert_eq!(d_done.sign_mode, Some(SignMode::Approved));
    assert_eq!(d_done.doc_file, "design/v1-final.md");
    assert_eq!(d_done.sign_note.as_deref(), Some("approved for dispatch"));
    assert_eq!(d_done.waiting_since_ms, None);
    assert!(d_done.open_questions().is_empty());
}

#[test]
fn skipped_path_and_suggest_skip_to_round_1() {
    let base = vec![
        ev(1, "user", studio_required()),
        req_ev(2, 1),
        opened(3, "producer", 1, 0, true),
    ];
    let s_skip = replay(&base);
    let d_skip = s_skip.design.as_ref().unwrap();
    assert_eq!(d_skip.phase(), DesignPhase::SuggestSkip);
    assert!(d_skip.suggest_skip);
    assert_eq!(d_skip.skip_reason.as_deref(), Some("small touchup"));
    assert_eq!(d_skip.waiting_since_ms, Some(3_000));

    let mut approved_skip = base.clone();
    approved_skip.push(ev(
        4,
        "user",
        EventKind::DesignSigned {
            version: 1,
            round: 0,
            mode: SignMode::Skipped,
            doc_file: String::new(),
            note: Some("external plan".into()),
        },
    ));
    let s_signed = replay(&approved_skip);
    let d_signed = s_signed.design.as_ref().unwrap();
    assert_eq!(d_signed.phase(), DesignPhase::Signed);
    assert_eq!(d_signed.sign_mode, Some(SignMode::Skipped));
    assert_eq!(d_signed.waiting_since_ms, None);

    // Reject skip -> open Round 1
    let mut discuss = base;
    discuss.push(opened(4, "user", 1, 1, false));
    let s_r1 = replay(&discuss);
    let d_r1 = s_r1.design.as_ref().unwrap();
    assert_eq!(d_r1.phase(), DesignPhase::WaitingProposal);
    assert!(!d_r1.suggest_skip);
    assert_eq!(d_r1.skip_reason, None);
    assert_eq!(d_r1.waiting_since_ms, None);
    assert_eq!(d_r1.current_round_num(), 1);
}

#[test]
fn req_version_change_preserves_signed_history_and_resets_unsigned() {
    let mut events = vec![
        ev(1, "user", studio_required()),
        req_ev(2, 1),
        opened(3, "producer", 1, 1, false),
        entry(4, "writer", 1, 1, DesignSlot::Proposal, None, vec![]),
        entry(5, "director", 1, 1, DesignSlot::Buildability, None, vec![]),
        entry(6, "producer", 1, 1, DesignSlot::Summary, None, vec![]),
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
        req_ev(8, 2),
    ];
    let s2 = replay(&events);
    assert!(s2.design.is_none());
    assert_eq!(s2.design_phase(), DesignPhase::Idle);
    let v1 = s2.design_history.get(&1).expect("v1 signed history kept");
    assert_eq!(v1.phase(), DesignPhase::Signed);
    assert_eq!(v1.rounds.len(), 1);

    // Unsigned v2 opened then superseded by v3 resets v2 without storing in design_history.
    events.push(opened(9, "producer", 2, 1, false));
    assert_eq!(replay(&events).design_phase(), DesignPhase::WaitingProposal);
    events.push(req_ev(10, 3));
    let s3 = replay(&events);
    assert!(s3.design.is_none());
    assert_eq!(s3.design_phase(), DesignPhase::Idle);
    assert!(s3.design_history.contains_key(&1));
    assert!(!s3.design_history.contains_key(&2));
}

#[test]
fn jsonl_round_trip_and_legacy_studio_created_defaults_to_off() {
    for tag in ["design.opened", "design.entry", "design.signed"] {
        assert!(EventKind::is_known_tag(tag), "{tag} must be known");
    }
    let samples = vec![
        ev(1, "user", studio_required()),
        opened(2, "producer", 1, 1, false),
        entry(
            3,
            "producer",
            1,
            1,
            DesignSlot::Summary,
            None,
            vec![q("q1", "Keep old path?", "Yes")],
        ),
        entry(
            4,
            "stage",
            1,
            1,
            DesignSlot::UserInput,
            Some(EntrySource::Hook),
            vec![],
        ),
        ev(
            5,
            "user",
            EventKind::DesignSigned {
                version: 1,
                round: 1,
                mode: SignMode::Approved,
                doc_file: "design/v1-final.md".into(),
                note: Some("go".into()),
            },
        ),
    ];
    for sample in samples {
        let line = serde_json::to_string(&sample).unwrap();
        let back: Event = serde_json::from_str(&line).unwrap();
        assert_eq!(back, sample);
    }

    // Old studio.created JSON without `design_gate` reads as `Off`.
    let old_line = r#"{"seq":1,"ts_ms":0,"by":{"role":"user"},"kind":"studio.created","title":"old","repo":"/home/user/acme-shop","mode":"standard","roles":{"producer":{"runner":"r","model":"m","effort":"e","permission_profile":"p"},"director":{"runner":"r","model":"m","effort":"e","permission_profile":"p"},"writer":{"runner":"r","model":"m","effort":"e","permission_profile":"p"}},"candidates":[],"rework_cap":2,"stall_limit_s":600}"#;
    let parsed: Event = serde_json::from_str(old_line).unwrap();
    match parsed.kind {
        EventKind::StudioCreated { design_gate, .. } => assert_eq!(design_gate, DesignGate::Off),
        other => panic!("expected studio.created, got {other:?}"),
    }
    let state: StudioState = replay(&[parsed]);
    assert_eq!(state.studio.unwrap().design_gate, DesignGate::Off);
}
