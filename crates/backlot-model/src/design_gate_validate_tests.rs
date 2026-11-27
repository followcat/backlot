//! Opening design hard-gate and validation tests (`validate`).

use crate::design::{DesignGate, DesignQuestion, DesignSlot, EntrySource, SignMode};
use crate::event::{Event, EventKind, PlanKind};
use crate::state::{StudioState, replay};
use crate::steps::next_steps;
use crate::summary::summary;
use crate::testkit::*;
use crate::validate::{Refusal, validate};
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
            reason: suggest_skip.then(|| "small".to_string()),
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
    refs: Vec<u64>,
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
            refs,
            questions: Vec::<DesignQuestion>::new(),
            checks: Vec::new(),
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

fn dispatch_ev() -> EventKind {
    EventKind::MessageQueued {
        to: "director".into(),
        file: Some("messages/dispatch.md".into()),
        purpose: Some("dispatch".into()),
        source_seq: None,
    }
}

fn plan_ev(version: u32) -> EventKind {
    EventKind::PlanPublished {
        plan_kind: PlanKind::Plan,
        file: format!("plan/v{version}-plan.md"),
        requirement_version: version,
    }
}

fn assert_gated(state: &StudioState, phase: &str) {
    let want = Err(Refusal::DesignNotSigned(state.requirement_version));
    let next_task = format!("t{}", state.tasks.len() + 1);
    assert_eq!(
        validate(
            state,
            &new_ev(
                "director",
                task_created(&next_task, "feature", state.requirement_version)
            )
        ),
        want,
        "{phase}: TaskCreated"
    );
    assert_eq!(
        validate(state, &new_ev("director", cast(&next_task, "actor-a", "a"))),
        want,
        "{phase}: TaskCast"
    );
    assert_eq!(
        validate(state, &new_ev("producer", dispatch_ev())),
        want,
        "{phase}: dispatch"
    );
    assert_eq!(
        validate(state, &new_ev("writer", plan_ev(state.requirement_version))),
        want,
        "{phase}: PlanPublished"
    );
}

#[test]
fn unsigned_design_refuses_tasks_casts_dispatch_and_plans() {
    let mut events = vec![ev(1, "user", studio_required()), req(2, 1)];
    assert_gated(&replay(&events), "Idle");

    let skip_state = replay(&[
        ev(1, "user", studio_required()),
        req(2, 1),
        opened(3, "producer", 1, 0, true),
    ]);
    assert_gated(&skip_state, "SuggestSkip");

    events.push(opened(3, "producer", 1, 1, false));
    assert_gated(&replay(&events), "WaitingProposal");

    events.push(entry(4, "writer", 1, 1, DesignSlot::Proposal, None, vec![]));
    assert_gated(&replay(&events), "WaitingBuildability");

    events.push(entry(
        5,
        "director",
        1,
        1,
        DesignSlot::Buildability,
        None,
        vec![],
    ));
    assert_gated(&replay(&events), "WaitingSummary");

    events.push(entry(
        6,
        "producer",
        1,
        1,
        DesignSlot::Summary,
        None,
        vec![],
    ));
    assert_gated(&replay(&events), "WaitingUser");

    // Normal sign-off unlocks PlanPublished, dispatch, TaskCreated, TaskCast.
    let mut approved = events.clone();
    approved.push(signed(7, 1, 1, SignMode::Approved));
    let signed_state = replay(&approved);
    assert_eq!(
        validate(&signed_state, &new_ev("writer", plan_ev(1))),
        Ok(())
    );
    assert_eq!(
        validate(&signed_state, &new_ev("producer", dispatch_ev())),
        Ok(())
    );
    assert_eq!(
        validate(
            &signed_state,
            &new_ev("director", task_created("t1", "feature", 1))
        ),
        Ok(())
    );
    approved.push(ev(8, "director", task_created("t1", "feature", 1)));
    let with_task = replay(&approved);
    assert_eq!(
        validate(&with_task, &new_ev("director", cast("t1", "actor-a", "a"))),
        Ok(())
    );

    // Explicit skip sign-off also unlocks them.
    let skipped = replay(&[
        ev(1, "user", studio_required()),
        req(2, 1),
        opened(3, "producer", 1, 0, true),
        signed(4, 1, 0, SignMode::Skipped),
    ]);
    assert_eq!(validate(&skipped, &new_ev("writer", plan_ev(1))), Ok(()));
    assert_eq!(
        validate(&skipped, &new_ev("producer", dispatch_ev())),
        Ok(())
    );
    assert_eq!(
        validate(
            &skipped,
            &new_ev("director", task_created("t1", "feature", 1))
        ),
        Ok(())
    );

    // A new req.version re-arms the hard gate even though v1 was signed.
    approved.push(req(9, 2));
    assert_gated(&replay(&approved), "v2 Idle after v1 Signed");
}

#[test]
fn intra_round_order_and_user_input_and_supplement_refs_enforced() {
    let put = |st: &StudioState, who: &str, seq: u64, slot: DesignSlot, refs: Vec<u64>| {
        validate(
            st,
            &new_ev(who, entry(seq, who, 1, 1, slot, None, refs).kind),
        )
    };
    let mut events = vec![
        ev(1, "user", studio_required()),
        req(2, 1),
        opened(3, "producer", 1, 1, false),
    ];
    let waiting_prop = replay(&events);
    for (who, seq, slot) in [
        ("director", 4, DesignSlot::Buildability),
        ("producer", 4, DesignSlot::Summary),
    ] {
        assert_eq!(
            put(&waiting_prop, who, seq, slot, vec![]),
            Err(Refusal::DesignWrongPhase {
                action: "record design entry",
                phase: crate::design::DesignPhase::WaitingProposal,
            })
        );
    }
    events.push(entry(4, "writer", 1, 1, DesignSlot::Proposal, None, vec![]));
    let waiting_build = replay(&events);
    assert!(matches!(
        put(&waiting_build, "producer", 5, DesignSlot::Summary, vec![]),
        Err(Refusal::DesignWrongPhase { .. })
    ));
    assert!(matches!(
        put(&waiting_build, "writer", 5, DesignSlot::Supplement, vec![4]),
        Err(Refusal::DesignWrongPhase { .. })
    ));

    events.push(entry(
        5,
        "director",
        1,
        1,
        DesignSlot::Buildability,
        None,
        vec![],
    ));
    events.push(entry(
        6,
        "producer",
        1,
        1,
        DesignSlot::Summary,
        None,
        vec![],
    ));
    // A plain tell to writer exists as message seq 7.
    events.push(ev(
        7,
        "user",
        EventKind::MessageQueued {
            to: "writer".into(),
            file: Some("messages/7-tell.md".into()),
            purpose: None,
            source_seq: None,
        },
    ));
    let waiting_user = replay(&events);

    // WaitingUser without UserInput refuses design next (round 2).
    assert_eq!(
        validate(
            &waiting_user,
            &new_ev("producer", opened(8, "producer", 1, 2, false).kind)
        ),
        Err(Refusal::DesignNeedsUserInput)
    );

    // Supplement in WaitingUser refuses empty refs, unknown seqs, and req.version seqs.
    for bad_refs in [vec![], vec![999], vec![2]] {
        assert!(matches!(
            put(&waiting_user, "writer", 8, DesignSlot::Supplement, bad_refs),
            Err(Refusal::InvalidDesign(_))
        ));
    }
    // Ref pointing to message seq 7 or entry seq 4/6 is accepted.
    assert_eq!(
        put(&waiting_user, "writer", 8, DesignSlot::Supplement, vec![7]),
        Ok(())
    );
    assert_eq!(
        put(
            &waiting_user,
            "director",
            8,
            DesignSlot::Supplement,
            vec![4, 6]
        ),
        Ok(())
    );

    // Recording UserInput unlocks round 2.
    events.push(entry(
        8,
        "user",
        1,
        1,
        DesignSlot::UserInput,
        Some(EntrySource::Tell),
        vec![],
    ));
    let with_input = replay(&events);
    assert_eq!(
        validate(
            &with_input,
            &new_ev("producer", opened(9, "producer", 1, 2, false).kind)
        ),
        Ok(())
    );
}

#[test]
fn gate_off_allows_task_cast_dispatch_and_plan_unchanged() {
    let default_studio = replay(&[ev(1, "user", studio_kind()), req(2, 1)]);
    assert_eq!(
        default_studio.studio.as_ref().unwrap().design_gate,
        DesignGate::Off
    );
    assert_eq!(
        validate(&default_studio, &new_ev("writer", plan_ev(1))),
        Ok(())
    );
    assert_eq!(
        validate(&default_studio, &new_ev("producer", dispatch_ev())),
        Ok(())
    );
    assert_eq!(
        validate(
            &default_studio,
            &new_ev("director", task_created("t1", "feature", 1))
        ),
        Ok(())
    );

    let with_task = replay(&[
        ev(1, "user", studio_kind()),
        req(2, 1),
        ev(3, "director", task_created("t1", "feature", 1)),
    ]);
    assert_eq!(
        validate(&with_task, &new_ev("director", cast("t1", "actor-a", "a"))),
        Ok(())
    );

    // Default gate=off flow produces no design summary and keeps legacy steps & wanted set.
    assert!(next_steps(&default_studio).is_empty());
    let roles: Vec<String> = wanted_roles(&default_studio)
        .into_iter()
        .map(|w| w.role)
        .collect();
    assert_eq!(roles, ["producer", "director"]);
    let doc = summary(
        &[("acme-shop".into(), default_studio)],
        &Default::default(),
        &Default::default(),
        &Default::default(),
        &Default::default(),
        1,
    );
    assert_eq!(doc.studios[0].design, None);
}
