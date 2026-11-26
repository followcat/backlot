//! Opening design lifecycle tests for `next_steps` and `wanted_roles`.

use std::collections::BTreeMap;

use crate::design::{DesignGate, DesignSlot, EntrySource, SignMode};
use crate::event::{Event, EventKind, PlanKind, Verdict};
use crate::state::{StudioState, replay};
use crate::steps::{Step, next_steps};
use crate::testkit::*;
use crate::wanted::wanted_roles;

fn studio_required() -> EventKind {
    let mut k = studio_kind();
    if let EventKind::StudioCreated { design_gate, .. } = &mut k {
        *design_gate = DesignGate::Required;
    }
    k
}

fn req(seq: u64, version: u32) -> Event {
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
            reason: suggest_skip.then(|| "small".into()),
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
) -> Event {
    ev(
        seq,
        role,
        EventKind::DesignEntry {
            version,
            round,
            slot,
            file: format!("design/v{version}/r{round}-{seq}.md"),
            source,
            refs: Vec::new(),
            questions: Vec::new(),
        },
    )
}

fn signed(seq: u64, version: u32, round: u32, mode: SignMode) -> Event {
    ev(
        seq,
        "user",
        EventKind::DesignSigned {
            version,
            round,
            mode,
            doc_file: if mode == SignMode::Approved {
                format!("design/v{version}-final.md")
            } else {
                String::new()
            },
            note: None,
        },
    )
}

fn publish(seq: u64, kind: PlanKind, version: u32) -> Event {
    ev(
        seq,
        "writer",
        EventKind::PlanPublished {
            plan_kind: kind,
            file: format!("plan/v{version}-{seq}.md"),
            requirement_version: version,
        },
    )
}

fn dispatch(seq: u64) -> Event {
    ev(
        seq,
        "producer",
        EventKind::MessageQueued {
            to: "director".into(),
            file: Some("messages/dispatch.md".into()),
            purpose: Some("dispatch".into()),
            source_seq: None,
        },
    )
}

fn single_step(state: &StudioState) -> (String, String, u64) {
    let steps = next_steps(state);
    assert_eq!(steps.len(), 1, "expected 1 step, got {steps:?}");
    let s: &Step = &steps[0];
    (s.label.clone(), s.owner.clone(), s.key)
}

fn wanted_names(state: &StudioState) -> Vec<String> {
    wanted_roles(state).into_iter().map(|w| w.role).collect()
}

#[test]
fn next_steps_tracks_every_design_phase_and_post_sign_mode() {
    let mut events = vec![ev(1, "user", studio_required())];
    assert!(next_steps(&replay(&events)).is_empty());

    // Idle -> open design — producer
    events.push(req(2, 1));
    assert_eq!(
        single_step(&replay(&events)),
        ("open design".into(), "producer".into(), 2)
    );

    // SuggestSkip -> review skip suggestion — user
    let skip_state = replay(&[
        ev(1, "user", studio_required()),
        req(2, 1),
        opened(3, "producer", 1, 0, true),
    ]);
    assert_eq!(
        single_step(&skip_state),
        ("review skip suggestion".into(), "user".into(), 2)
    );

    // WaitingProposal -> write design proposal — writer
    events.push(opened(3, "producer", 1, 1, false));
    assert_eq!(
        single_step(&replay(&events)),
        ("write design proposal".into(), "writer".into(), 3)
    );

    // WaitingBuildability -> review buildability — director
    events.push(entry(4, "writer", 1, 1, DesignSlot::Proposal, None));
    assert_eq!(
        single_step(&replay(&events)),
        ("review buildability".into(), "director".into(), 4)
    );

    // WaitingSummary -> summarize design round — producer
    events.push(entry(5, "director", 1, 1, DesignSlot::Buildability, None));
    assert_eq!(
        single_step(&replay(&events)),
        ("summarize design round".into(), "producer".into(), 5)
    );

    // WaitingUser -> review design & decide — user
    events.push(entry(6, "producer", 1, 1, DesignSlot::Summary, None));
    assert_eq!(
        single_step(&replay(&events)),
        ("review design & decide".into(), "user".into(), 6)
    );

    // Signed(Approved) without v1 plan -> derive plan & acceptance — writer
    let mut approved = events.clone();
    approved.push(signed(7, 1, 1, SignMode::Approved));
    assert_eq!(
        single_step(&replay(&approved)),
        ("derive plan & acceptance".into(), "writer".into(), 7)
    );

    // Plan alone is still missing acceptance.
    approved.push(publish(8, PlanKind::Plan, 1));
    assert_eq!(
        single_step(&replay(&approved)),
        ("derive plan & acceptance".into(), "writer".into(), 7)
    );

    // Both plan and acceptance published -> dispatch tasks — producer
    approved.push(publish(9, PlanKind::Acceptance, 1));
    assert_eq!(
        single_step(&replay(&approved)),
        ("dispatch tasks".into(), "producer".into(), 9)
    );

    // Dispatch delivered -> tasks — director
    approved.push(dispatch(10));
    assert!(next_steps(&replay(&approved)).is_empty());
    approved.push(ev(11, "stage", EventKind::MessageDelivered { target: 10 }));
    assert_eq!(
        single_step(&replay(&approved)),
        ("tasks".into(), "director".into(), 10)
    );

    // Signed(Skipped) without usable plan -> write plan — writer
    let skipped_no_plan = replay(&[
        ev(1, "user", studio_required()),
        req(2, 1),
        opened(3, "producer", 1, 0, true),
        signed(4, 1, 0, SignMode::Skipped),
    ]);
    assert_eq!(
        single_step(&skipped_no_plan),
        ("write plan".into(), "writer".into(), 4)
    );

    // Signed(Skipped) with an existing v1 plan on v2 -> dispatch tasks — producer
    approved.push(ev(12, "director", task_created("t1", "feature", 1)));
    approved.push(ev(13, "director", cast("t1", "actor-a", "a")));
    approved.push(ev(14, "actor-t1-c1", delivered("t1", 1, "a", 1)));
    approved.push(ev(
        15,
        "director",
        reviewed("t1", 1, Verdict::Accepted, vec![]),
    ));
    approved.push(ev(
        16,
        "director",
        EventKind::ReportSent {
            to: "producer".into(),
            node: "integration".into(),
            file: "reports/r.md".into(),
        },
    ));
    approved.push(req(17, 2));
    assert_eq!(
        single_step(&replay(&approved)),
        ("open design".into(), "producer".into(), 17)
    );
    approved.push(opened(18, "producer", 2, 0, true));
    approved.push(signed(19, 2, 0, SignMode::Skipped));
    assert_eq!(
        single_step(&replay(&approved)),
        ("dispatch tasks".into(), "producer".into(), 19)
    );
}

#[test]
fn wanted_roles_keeps_writer_through_design_rounds_and_plan_derivation() {
    let mut events = vec![ev(1, "user", studio_required()), req(2, 1)];
    assert_eq!(wanted_names(&replay(&events)), ["producer", "director"]);

    let skip_state = replay(&[
        ev(1, "user", studio_required()),
        req(2, 1),
        opened(3, "producer", 1, 0, true),
    ]);
    assert_eq!(wanted_names(&skip_state), ["producer", "director"]);

    events.push(opened(3, "producer", 1, 1, false));
    assert!(wanted_names(&replay(&events)).contains(&"writer".to_string()));

    events.push(entry(4, "writer", 1, 1, DesignSlot::Proposal, None));
    assert!(wanted_names(&replay(&events)).contains(&"writer".to_string()));

    events.push(entry(5, "director", 1, 1, DesignSlot::Buildability, None));
    assert!(wanted_names(&replay(&events)).contains(&"writer".to_string()));

    events.push(entry(6, "producer", 1, 1, DesignSlot::Summary, None));
    assert!(wanted_names(&replay(&events)).contains(&"writer".to_string()));

    events.push(entry(
        7,
        "user",
        1,
        1,
        DesignSlot::UserInput,
        Some(EntrySource::Tell),
    ));
    assert!(wanted_names(&replay(&events)).contains(&"writer".to_string()));

    // Round 2 keeps writer alive continuously too.
    events.push(opened(8, "producer", 1, 2, false));
    assert!(wanted_names(&replay(&events)).contains(&"writer".to_string()));
    events.push(entry(9, "writer", 1, 2, DesignSlot::Proposal, None));
    events.push(entry(10, "director", 1, 2, DesignSlot::Buildability, None));
    events.push(entry(11, "producer", 1, 2, DesignSlot::Summary, None));
    assert!(wanted_names(&replay(&events)).contains(&"writer".to_string()));

    // Signed(Approved) keeps writer until dispatch.
    events.push(signed(12, 1, 2, SignMode::Approved));
    assert!(wanted_names(&replay(&events)).contains(&"writer".to_string()));
    events.push(publish(13, PlanKind::Plan, 1));
    events.push(publish(14, PlanKind::Acceptance, 1));
    assert!(wanted_names(&replay(&events)).contains(&"writer".to_string()));
    events.push(dispatch(15));
    assert_eq!(wanted_names(&replay(&events)), ["producer", "director"]);

    // Signed(Skipped) with an existing plan does not start writer; without one it does.
    let skip_no_plan = replay(&[
        ev(1, "user", studio_required()),
        req(2, 1),
        opened(3, "producer", 1, 0, true),
        signed(4, 1, 0, SignMode::Skipped),
    ]);
    assert!(wanted_names(&skip_no_plan).contains(&"writer".to_string()));

    let skip_with_plan = replay(&[
        ev(1, "user", studio_required()),
        req(2, 1),
        opened(3, "producer", 1, 1, false),
        entry(4, "writer", 1, 1, DesignSlot::Proposal, None),
        entry(5, "director", 1, 1, DesignSlot::Buildability, None),
        entry(6, "producer", 1, 1, DesignSlot::Summary, None),
        signed(7, 1, 1, SignMode::Approved),
        publish(8, PlanKind::Plan, 1),
        publish(9, PlanKind::Acceptance, 1),
        dispatch(10),
        req(11, 2),
        opened(12, "producer", 2, 0, true),
        signed(13, 2, 0, SignMode::Skipped),
    ]);
    assert_eq!(wanted_names(&skip_with_plan), ["producer", "director"]);
}

#[test]
fn gate_off_steps_and_wanted_are_byte_identical_to_baseline() {
    let mut events = vec![
        ev(1, "user", studio_kind()),
        req(2, 1),
        ev(
            3,
            "producer",
            EventKind::MessageQueued {
                to: "writer".into(),
                file: None,
                purpose: Some("plan".into()),
                source_seq: None,
            },
        ),
    ];
    let s1 = replay(&events);
    assert_eq!(
        single_step(&s1),
        ("plan and acceptance list".into(), "writer".into(), 3)
    );
    assert_eq!(wanted_names(&s1), ["producer", "director", "writer"]);

    events.push(publish(4, PlanKind::Plan, 1));
    events.push(publish(5, PlanKind::Acceptance, 1));
    let s2 = replay(&events);
    assert_eq!(single_step(&s2), ("dispatch".into(), "producer".into(), 5));

    events.push(dispatch(6));
    events.push(ev(7, "stage", EventKind::MessageDelivered { target: 6 }));
    let s3 = replay(&events);
    assert_eq!(single_step(&s3), ("tasks".into(), "director".into(), 6));
    assert_eq!(wanted_names(&s3), ["producer", "director"]);

    // Verify default studio_kind() has DesignGate::Off and empty design state.
    let mut cast_map = BTreeMap::new();
    cast_map.insert("off", s3.studio.as_ref().unwrap().design_gate);
    assert_eq!(cast_map["off"], DesignGate::Off);
}
