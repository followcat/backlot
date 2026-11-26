//! Append-time role and phase validation tests for `design.*` events.

use crate::design::{DesignGate, DesignQuestion, DesignSlot, EntrySource, SignMode};
use crate::event::EventKind;
use crate::state::{StudioState, replay};
use crate::testkit::*;
use crate::validate::{Refusal, validate};

fn studio_required() -> EventKind {
    let mut k = studio_kind();
    if let EventKind::StudioCreated { design_gate, .. } = &mut k {
        *design_gate = DesignGate::Required;
    }
    k
}

fn req(seq: u64, version: u32) -> crate::event::Event {
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

fn open_kind(version: u32, round: u32, suggest_skip: bool) -> EventKind {
    EventKind::DesignOpened {
        version,
        round,
        suggest_skip,
        reason: suggest_skip.then(|| "small".to_string()),
        doc_file: format!("design/v{version}.md"),
    }
}

fn entry_kind(
    version: u32,
    round: u32,
    slot: DesignSlot,
    source: Option<EntrySource>,
    questions: Vec<DesignQuestion>,
) -> EventKind {
    EventKind::DesignEntry {
        version,
        round,
        slot,
        file: format!("design/v{version}/r{round}.md"),
        source,
        refs: Vec::new(),
        questions,
    }
}

fn sign_kind(version: u32, round: u32, mode: SignMode) -> EventKind {
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
    }
}

#[test]
fn role_rules_restrict_design_events_and_slots() {
    let mut base = vec![ev(1, "user", studio_required()), req(2, 1)];
    let idle = replay(&base);
    // DesignOpened: producer & user only.
    for ok_role in ["producer", "user"] {
        assert_eq!(
            validate(&idle, &new_ev(ok_role, open_kind(1, 1, false))),
            Ok(()),
            "{ok_role} may open"
        );
    }
    for bad_role in ["director", "writer", "stage", "actor-t1-c1", "hollywood"] {
        assert!(
            matches!(
                validate(&idle, &new_ev(bad_role, open_kind(1, 1, false))),
                Err(Refusal::RoleBound { .. })
            ),
            "{bad_role} must not open design"
        );
    }

    // Proposal: writer only.
    base.push(ev(3, "producer", open_kind(1, 1, false)));
    let waiting_prop = replay(&base);
    let prop = entry_kind(1, 1, DesignSlot::Proposal, None, vec![]);
    assert_eq!(
        validate(&waiting_prop, &new_ev("writer", prop.clone())),
        Ok(())
    );
    for bad in ["producer", "director", "user", "stage", "actor-t1-c1"] {
        assert!(
            matches!(
                validate(&waiting_prop, &new_ev(bad, prop.clone())),
                Err(Refusal::RoleBound { .. })
            ),
            "{bad} must not write proposal"
        );
    }

    // Buildability: director only.
    base.push(ev(4, "writer", prop));
    let waiting_build = replay(&base);
    let build = entry_kind(1, 1, DesignSlot::Buildability, None, vec![]);
    assert_eq!(
        validate(&waiting_build, &new_ev("director", build.clone())),
        Ok(())
    );
    for bad in ["producer", "writer", "user", "stage", "actor-t1-c1"] {
        assert!(
            matches!(
                validate(&waiting_build, &new_ev(bad, build.clone())),
                Err(Refusal::RoleBound { .. })
            ),
            "{bad} must not write buildability"
        );
    }

    // Summary: producer only.
    base.push(ev(5, "director", build));
    let waiting_sum = replay(&base);
    let sum = entry_kind(1, 1, DesignSlot::Summary, None, vec![]);
    assert_eq!(
        validate(&waiting_sum, &new_ev("producer", sum.clone())),
        Ok(())
    );
    for bad in ["director", "writer", "user", "stage", "actor-t1-c1"] {
        assert!(
            matches!(
                validate(&waiting_sum, &new_ev(bad, sum.clone())),
                Err(Refusal::RoleBound { .. })
            ),
            "{bad} must not write summary"
        );
    }

    // WaitingUser: UserInput, Supplement, and DesignSigned (user only).
    base.push(ev(6, "producer", sum));
    let waiting_user = replay(&base);
    assert_eq!(
        validate(
            &waiting_user,
            &new_ev(
                "user",
                entry_kind(1, 1, DesignSlot::UserInput, Some(EntrySource::Tell), vec![])
            )
        ),
        Ok(())
    );
    assert_eq!(
        validate(
            &waiting_user,
            &new_ev(
                "producer",
                entry_kind(
                    1,
                    1,
                    DesignSlot::UserInput,
                    Some(EntrySource::ProducerNote),
                    vec![]
                )
            )
        ),
        Ok(())
    );
    assert_eq!(
        validate(
            &waiting_user,
            &new_ev(
                "stage",
                entry_kind(1, 1, DesignSlot::UserInput, Some(EntrySource::Hook), vec![])
            )
        ),
        Ok(())
    );
    // Producer cannot forge Tell source.
    assert!(
        validate(
            &waiting_user,
            &new_ev(
                "producer",
                entry_kind(1, 1, DesignSlot::UserInput, Some(EntrySource::Tell), vec![])
            )
        )
        .is_err()
    );

    let mut supp_with_ref = entry_kind(1, 1, DesignSlot::Supplement, None, vec![]);
    if let EventKind::DesignEntry { refs, .. } = &mut supp_with_ref {
        *refs = vec![6];
    }
    for ok_role in ["writer", "director"] {
        assert_eq!(
            validate(&waiting_user, &new_ev(ok_role, supp_with_ref.clone())),
            Ok(())
        );
    }
    for bad in ["producer", "user", "stage", "actor-t1-c1"] {
        assert!(
            matches!(
                validate(&waiting_user, &new_ev(bad, supp_with_ref.clone())),
                Err(Refusal::RoleBound { .. })
            ),
            "{bad} must not write supplement"
        );
    }

    let sign = sign_kind(1, 1, SignMode::Approved);
    assert_eq!(
        validate(&waiting_user, &new_ev("user", sign.clone())),
        Ok(())
    );
    for bad in ["producer", "director", "writer", "stage", "actor-t1-c1"] {
        assert_eq!(
            validate(&waiting_user, &new_ev(bad, sign.clone())),
            Err(Refusal::RoleBound {
                role: bad.into(),
                who: "the user",
            }),
            "{bad} must not sign off"
        );
    }
    assert!(matches!(
        validate(&waiting_user, &new_ev("hollywood", sign)),
        Err(Refusal::RoleBound { .. })
    ));
}

#[test]
fn phase_rules_enforce_order_and_gate_tasks_when_required() {
    let no_req = replay(&[ev(1, "user", studio_required())]);
    assert_eq!(
        validate(&no_req, &new_ev("producer", open_kind(1, 1, false))),
        Err(Refusal::UnknownReqVersion(1))
    );

    let v_entry = |st: &StudioState, who: &str, slot: DesignSlot, q: Vec<DesignQuestion>| {
        validate(st, &new_ev(who, entry_kind(1, 1, slot, None, q)))
    };
    let idle = replay(&[ev(1, "user", studio_required()), req(2, 1)]);
    assert_eq!(
        validate(&idle, &new_ev("director", task_created("t1", "fix", 1))),
        Err(Refusal::DesignNotSigned(1))
    );
    assert!(v_entry(&idle, "writer", DesignSlot::Proposal, vec![]).is_err());
    assert!(validate(&idle, &new_ev("user", sign_kind(1, 1, SignMode::Approved))).is_err());

    // WaitingProposal refuses director buildability and user sign-off.
    let waiting_prop = replay(&[
        ev(1, "user", studio_required()),
        req(2, 1),
        ev(3, "producer", open_kind(1, 1, false)),
    ]);
    assert!(v_entry(&waiting_prop, "director", DesignSlot::Buildability, vec![]).is_err());
    assert!(
        validate(
            &waiting_prop,
            &new_ev("user", sign_kind(1, 1, SignMode::Approved))
        )
        .is_err()
    );

    // WaitingSummary rejects > 3 questions.
    let waiting_sum = replay(&[
        ev(1, "user", studio_required()),
        req(2, 1),
        ev(3, "producer", open_kind(1, 1, false)),
        ev(
            4,
            "writer",
            entry_kind(1, 1, DesignSlot::Proposal, None, vec![]),
        ),
        ev(
            5,
            "director",
            entry_kind(1, 1, DesignSlot::Buildability, None, vec![]),
        ),
    ]);
    let q_item = |id: &str| DesignQuestion {
        id: id.into(),
        prompt: "p".into(),
        recommendation: "r".into(),
        section_ref: Some("§R1.W".into()),
    };
    let qs = vec![q_item("1"), q_item("2"), q_item("3"), q_item("4")];
    assert!(v_entry(&waiting_sum, "producer", DesignSlot::Summary, qs).is_err());

    // WaitingUser rejects round 2 without UserInput; accepts after UserInput.
    let mut wait_user_evs = vec![
        ev(1, "user", studio_required()),
        req(2, 1),
        ev(3, "producer", open_kind(1, 1, false)),
        ev(
            4,
            "writer",
            entry_kind(1, 1, DesignSlot::Proposal, None, vec![]),
        ),
        ev(
            5,
            "director",
            entry_kind(1, 1, DesignSlot::Buildability, None, vec![]),
        ),
        ev(
            6,
            "producer",
            entry_kind(1, 1, DesignSlot::Summary, None, vec![]),
        ),
    ];
    let wait_user = replay(&wait_user_evs);
    assert!(
        validate(&wait_user, &new_ev("producer", open_kind(1, 2, false))).is_err(),
        "round 2 without UserInput must be refused"
    );

    wait_user_evs.push(ev(
        7,
        "user",
        entry_kind(1, 1, DesignSlot::UserInput, Some(EntrySource::Tell), vec![]),
    ));
    let with_input = replay(&wait_user_evs);
    assert_eq!(
        validate(&with_input, &new_ev("producer", open_kind(1, 2, false))),
        Ok(())
    );

    // SuggestSkip allows Skipped, refuses Approved.
    let skip_state = replay(&[
        ev(1, "user", studio_required()),
        req(2, 1),
        ev(3, "producer", open_kind(1, 0, true)),
    ]);
    assert!(
        validate(
            &skip_state,
            &new_ev("user", sign_kind(1, 0, SignMode::Approved))
        )
        .is_err()
    );
    assert_eq!(
        validate(
            &skip_state,
            &new_ev("user", sign_kind(1, 0, SignMode::Skipped))
        ),
        Ok(())
    );
    assert_eq!(
        validate(&skip_state, &new_ev("user", open_kind(1, 1, false))),
        Ok(())
    );

    // Signed refuses duplicate sign or new round.
    wait_user_evs.push(ev(8, "user", sign_kind(1, 1, SignMode::Approved)));
    let signed = replay(&wait_user_evs);
    assert!(
        validate(
            &signed,
            &new_ev("user", sign_kind(1, 1, SignMode::Approved))
        )
        .is_err()
    );
    assert!(validate(&signed, &new_ev("producer", open_kind(1, 2, false))).is_err());
}
