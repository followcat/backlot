//! Hollywood's append rules (D17a, R26): every role rule gets an
//! accepted and a refused case — on the overseer's own ledger and
//! at the studio-side gate.

use crate::event::{EventKind, Priority, TaskStateChange, Verdict};
use crate::hollywood::*;
use crate::testkit::*;
use crate::validate::{Refusal, validate as studio_validate};

/// `card.proposed` is Hollywood's own.
#[test]
fn only_hollywood_proposes_cards() {
    let state = HollywoodState::default();
    assert!(validate(&state, &nhw("hollywood", hpropose("t1"))).is_ok());
    for role in ["user", "producer", "director", "stage", "actor-t1-c1"] {
        let e = validate(&state, &nhw(role, hpropose("t1"))).unwrap_err();
        assert_eq!(
            e,
            HRefusal::RoleBound {
                role: role.into(),
                who: "hollywood"
            },
            "{role}"
        );
    }
}

/// `card.applied` and `card.cancelled` are the user's alone, and
/// only on an open card.
#[test]
fn only_the_user_applies_or_cancels() {
    let mut state = HollywoodState::default();
    apply(&mut state, &hev(1, "hollywood", hpropose("t1")));
    for kind in [
        HollywoodKind::CardApplied { card: 1 },
        HollywoodKind::CardCancelled {
            card: 1,
            reason: String::new(),
        },
    ] {
        for role in ["hollywood", "producer", "stage"] {
            let e = validate(&state, &nhw(role, kind.clone())).unwrap_err();
            assert!(matches!(e, HRefusal::RoleBound { .. }), "{role}: {e:?}");
        }
        assert!(validate(&state, &nhw("user", kind)).is_ok());
    }
    // A card that does not exist, and one already closed.
    assert_eq!(
        validate(&state, &nhw("user", HollywoodKind::CardApplied { card: 9 })),
        Err(HRefusal::NoCard(9))
    );
    apply(
        &mut state,
        &hev(2, "user", HollywoodKind::CardApplied { card: 1 }),
    );
    let e = validate(&state, &nhw("user", HollywoodKind::CardApplied { card: 1 }));
    assert_eq!(
        e,
        Err(HRefusal::CardClosed {
            card: 1,
            state: "applied"
        })
    );
}

/// `card.done`/`card.failed` are the stage's, on an applied card.
#[test]
fn only_the_stage_closes_an_applied_card() {
    let mut state = HollywoodState::default();
    apply(&mut state, &hev(1, "hollywood", hpropose("t1")));
    // Not applied yet — open is not enough.
    let e = validate(
        &state,
        &nhw(
            "stage",
            HollywoodKind::CardDone {
                card: 1,
                note: String::new(),
            },
        ),
    );
    assert_eq!(
        e,
        Err(HRefusal::CardNotApplied {
            card: 1,
            state: "open"
        })
    );
    apply(
        &mut state,
        &hev(2, "user", HollywoodKind::CardApplied { card: 1 }),
    );
    assert!(
        validate(
            &state,
            &nhw(
                "stage",
                HollywoodKind::CardFailed {
                    card: 1,
                    reason: "merge refused".into()
                }
            )
        )
        .is_ok()
    );
    let e = validate(
        &state,
        &nhw(
            "user",
            HollywoodKind::CardDone {
                card: 1,
                note: String::new(),
            },
        ),
    );
    assert!(matches!(e, Err(HRefusal::RoleBound { .. })), "{e:?}");
    // A failure needs its reason.
    let e = validate(
        &state,
        &nhw(
            "stage",
            HollywoodKind::CardFailed {
                card: 1,
                reason: " ".into(),
            },
        ),
    );
    assert!(matches!(e, Err(HRefusal::BadCard(_))), "{e:?}");
}

/// `card.stale` is the user's or the stage's, on an open card.
#[test]
fn stale_marks_are_user_or_stage() {
    let mut state = HollywoodState::default();
    apply(&mut state, &hev(1, "hollywood", hpropose("t1")));
    for role in ["user", "stage"] {
        assert!(
            validate(
                &state,
                &nhw(
                    role,
                    HollywoodKind::CardStale {
                        card: 1,
                        reason: "t1 moved".into()
                    }
                )
            )
            .is_ok(),
            "{role}"
        );
    }
    let e = validate(
        &state,
        &nhw(
            "hollywood",
            HollywoodKind::CardStale {
                card: 1,
                reason: "x".into(),
            },
        ),
    );
    assert!(matches!(e, Err(HRefusal::RoleBound { .. })), "{e:?}");
}

/// `hollywood.reported` is Hollywood's one-line headline.
#[test]
fn only_hollywood_reports() {
    let state = HollywoodState::default();
    let ok = HollywoodKind::Reported {
        headline: "two studios quiet, one review waiting".into(),
        body: None,
    };
    assert!(validate(&state, &nhw("hollywood", ok.clone())).is_ok());
    for role in ["user", "producer", "stage"] {
        assert!(matches!(
            validate(&state, &nhw(role, ok.clone())),
            Err(HRefusal::RoleBound { .. })
        ));
    }
    for headline in ["", " ", "two\nlines"] {
        let bad = HollywoodKind::Reported {
            headline: headline.into(),
            body: None,
        };
        assert!(matches!(
            validate(&state, &nhw("hollywood", bad)),
            Err(HRefusal::BadReport(_))
        ));
    }
}

/// `message.sent`: user → hollywood, hollywood → user, hollywood →
/// a studio's producer. Nothing else.
#[test]
fn message_directions_are_fixed() {
    let state = HollywoodState::default();
    let msg = |to: &str, studio: Option<&str>| HollywoodKind::MessageSent {
        to: to.into(),
        studio: studio.map(str::to_string),
        text: Some("hi".into()),
        file: None,
    };
    assert!(validate(&state, &nhw("user", msg("hollywood", None))).is_ok());
    assert!(validate(&state, &nhw("hollywood", msg("user", None))).is_ok());
    assert!(
        validate(
            &state,
            &nhw("hollywood", msg("producer", Some("acme-shop")))
        )
        .is_ok()
    );
    for (role, to, studio) in [
        ("producer", "hollywood", None),
        ("hollywood", "director", Some("acme-shop")),
        ("hollywood", "producer", None), // a producer message needs its studio
        ("user", "user", None),
        ("hollywood", "hollywood", None),
        ("stage", "hollywood", None),
        ("user", "hollywood", Some("acme-shop")), // user messages carry no studio
    ] {
        let e = validate(&state, &nhw(role, msg(to, studio)));
        assert!(
            matches!(e, Err(HRefusal::BadMessage(_))),
            "{role}→{to}: {e:?}"
        );
    }
}
// ── The studio-side gate (R26, D16a) ───────────────────────────

/// Hollywood talks *to* studios, never inside them: a purpose-free
/// message or a report to the producer lands; every other kind
/// refuses — a `plan`-purpose message and a report to anyone but
/// the producer included.
#[test]
fn hollywood_only_messages_or_reports_the_producer() {
    let state = running_state();
    assert_eq!(
        studio_validate(
            &state,
            &new_ev(
                "hollywood",
                EventKind::MessageQueued {
                    to: "producer".into(),
                    file: Some("messages/9-m.md".into()),
                    purpose: None,
                    source_seq: None,
                },
            )
        ),
        Ok(())
    );
    assert_eq!(
        studio_validate(
            &state,
            &new_ev(
                "hollywood",
                EventKind::ReportSent {
                    to: "producer".into(),
                    node: "daily".into(),
                    file: "reports/9-r.md".into(),
                },
            )
        ),
        Ok(())
    );

    let why = "message or report a studio's producer, or read";
    for kind in [
        // A `plan`-purpose message is the producer's, not Hollywood's.
        EventKind::MessageQueued {
            to: "producer".into(),
            file: None,
            purpose: Some("plan".into()),
            source_seq: None,
        },
        EventKind::MessageQueued {
            to: "director".into(),
            file: None,
            purpose: None,
            source_seq: None,
        },
        EventKind::ReportSent {
            to: "user".into(),
            node: "daily".into(),
            file: "reports/9.md".into(),
        },
        task_state("t1", TaskStateChange::Paused, "hold"),
        EventKind::TaskPriority {
            task: "t1".into(),
            priority: Priority::High,
        },
        EventKind::RoleReopen {
            role: "writer".into(),
        },
        reviewed("t1", 1, Verdict::Accepted, vec![]),
        EventKind::AttentionRaised {
            role: "producer".into(),
            reason: "x".into(),
            detail: String::new(),
            needs_user: false,
        },
        EventKind::StudioFinished {
            reason: "done".into(),
        },
    ] {
        assert_eq!(
            studio_validate(&state, &new_ev("hollywood", kind)),
            Err(Refusal::RoleBound {
                role: "hollywood".into(),
                who: why,
            })
        );
    }
}
