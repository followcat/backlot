//! The card gate: a card's one life (open, exactly one close), the
//! headline cap, action/base coherence and the pure `stale` check
//! `card apply` runs against the studio's current state.

use crate::event::Priority;
use crate::hollywood::*;
use crate::state::TaskStatus;
use crate::testkit::*;

/// The card's life: open on propose, exactly one close after.
#[test]
fn a_card_opens_then_closes_once() {
    let mut state = HollywoodState::default();
    apply(&mut state, &hev(1, "hollywood", hpropose("t1")));
    assert_eq!(state.open_cards().count(), 1);
    apply(
        &mut state,
        &hev(2, "user", HollywoodKind::CardApplied { card: 1 }),
    );
    assert_eq!(state.cards[&1].state, CardState::Applied);
    assert_eq!(state.open_cards().count(), 0);
    apply(
        &mut state,
        &hev(
            3,
            "stage",
            HollywoodKind::CardDone {
                card: 1,
                note: "paused t1".into(),
            },
        ),
    );
    assert_eq!(state.cards[&1].state, CardState::Done);
    assert_eq!(state.cards[&1].detail, "paused t1");
}

/// Headlines keep the newest ten; the oldest fall off.
#[test]
fn headlines_are_capped() {
    let mut state = HollywoodState::default();
    for i in 1..=12 {
        apply(
            &mut state,
            &hev(
                i,
                "hollywood",
                HollywoodKind::Reported {
                    headline: format!("headline {i}"),
                    body: None,
                },
            ),
        );
    }
    assert_eq!(state.reports.len(), HEADLINE_CAP);
    assert_eq!(state.reports[0].headline, "headline 3");
    assert_eq!(state.last_headline(), Some("headline 12"));
}

/// A card needs its why and a base that matches the action —
/// including the status the studio's own transition rules allow.
#[test]
fn a_card_needs_a_coherent_base() {
    let state = HollywoodState::default();
    let no_why = HollywoodKind::CardProposed {
        action: hpause("t1"),
        why: " ".into(),
        base: hpause_base(),
    };
    assert!(matches!(
        validate(&state, &nhw("hollywood", no_why)),
        Err(HRefusal::BadCard(_))
    ));
    for (action, base) in [
        // Wrong base kind for the action.
        (hpause("t1"), CardBase::StudioAbsent),
        (
            CardAction::Close {
                studio: "acme-shop".into(),
                person: "actor-a".into(),
            },
            CardBase::TaskStatus {
                was: TaskStatus::Running,
            },
        ),
        // A pause base must be running; a resume base paused or blocked.
        (
            hpause("t1"),
            CardBase::TaskStatus {
                was: TaskStatus::Delivered,
            },
        ),
        (
            CardAction::Resume {
                studio: "acme-shop".into(),
                task: "t1".into(),
            },
            CardBase::TaskStatus {
                was: TaskStatus::Running,
            },
        ),
        (
            CardAction::Abandon {
                studio: "acme-shop".into(),
                task: "t1".into(),
            },
            CardBase::TaskStatus {
                was: TaskStatus::Accepted,
            },
        ),
    ] {
        let bad = HollywoodKind::CardProposed {
            action,
            why: "w".into(),
            base,
        };
        assert!(matches!(
            validate(&state, &nhw("hollywood", bad)),
            Err(HRefusal::BadCard(_))
        ));
    }
}

// ── Staleness ──────────────────────────────────────────────────

/// `stale` answers `Some(reason)` exactly when the base no longer
/// holds — the pure check `card apply` runs.
#[test]
fn stale_checks_the_base_against_the_studio() {
    let running = running_state(); // t1 running (priority normal), t2 created
    let act = hpause("t1");
    let pbase = hpause_base();
    assert_eq!(stale(&act, &pbase, Some(&running)), None);
    assert_eq!(
        stale(&act, &pbase, None),
        Some("no studio 'acme-shop'".into())
    );

    // Task moved on: paused already → stale.
    let paused = state_with(
        &base_events(),
        vec![
            ("director", cast("t1", "actor-a", "a")),
            (
                "director",
                task_state("t1", crate::event::TaskStateChange::Paused, "hold"),
            ),
        ],
    );
    assert_eq!(
        stale(&act, &pbase, Some(&paused)),
        Some("'t1' is paused now".into())
    );
    // A task the card does not know → stale.
    let unknown = hpause("t9");
    assert_eq!(
        stale(&unknown, &pbase, Some(&running)),
        Some("no task 't9'".into())
    );
}

#[test]
fn priority_and_person_bases() {
    let running = running_state();
    // Priority: stands while t1 is `normal`, stale once it changes.
    let prio = CardAction::Priority {
        studio: "acme-shop".into(),
        task: "t1".into(),
        level: Priority::High,
    };
    let pbase = CardBase::TaskPriority {
        was: Priority::Normal,
    };
    assert_eq!(stale(&prio, &pbase, Some(&running)), None);
    let raised = state_with(
        &base_events(),
        vec![
            ("director", cast("t1", "actor-a", "a")),
            (
                "user",
                crate::event::EventKind::TaskPriority {
                    task: "t1".into(),
                    priority: Priority::High,
                },
            ),
        ],
    );
    assert_eq!(
        stale(&prio, &pbase, Some(&raised)),
        Some("'t1' priority is high now".into())
    );

    // Close needs the person still held.
    let close = CardAction::Close {
        studio: "acme-shop".into(),
        person: "writer".into(),
    };
    let hbase = CardBase::PersonHeld;
    let mut held = running_state();
    held.roles.insert(
        "writer".into(),
        crate::state::RoleState {
            live: false,
            closed: true,
            held: true,
            ..Default::default()
        },
    );
    assert_eq!(stale(&close, &hbase, Some(&held)), None);
    held.roles.get_mut("writer").unwrap().held = false;
    assert_eq!(
        stale(&close, &hbase, Some(&held)),
        Some("'writer' is closed".into())
    );
    held.roles.remove("writer");
    assert_eq!(
        stale(&close, &hbase, Some(&held)),
        Some("no role 'writer'".into())
    );

    // Reopen needs the person still closed and unheld.
    let reopen = CardAction::Reopen {
        studio: "acme-shop".into(),
        person: "writer".into(),
    };
    let cbase = CardBase::PersonClosed;
    // No such person at all is stale the same way.
    assert_eq!(
        stale(&reopen, &cbase, Some(&running)),
        Some("no role 'writer'".into())
    );
    let mut closed = running_state();
    closed.roles.insert(
        "writer".into(),
        crate::state::RoleState {
            live: false,
            closed: true,
            held: false,
            ..Default::default()
        },
    );
    assert_eq!(stale(&reopen, &cbase, Some(&closed)), None);
    closed.roles.get_mut("writer").unwrap().live = true;
    assert_eq!(
        stale(&reopen, &cbase, Some(&closed)),
        Some("'writer' is live".into())
    );
}

/// `studio-new` goes stale the moment its name is taken.
#[test]
fn studio_new_goes_stale_when_the_name_is_taken() {
    let action = CardAction::StudioNew {
        studio: "acme-shop".into(),
        title: "Shop".into(),
        repo: "/home/user/acme-shop".into(),
        cast: "drafts/acme-shop/cast.toml".into(),
        brief: "drafts/acme-shop/brief.md".into(),
        no_git: false,
        snapshot: None,
    };
    assert_eq!(stale(&action, &CardBase::StudioAbsent, None), None);
    assert_eq!(
        stale(&action, &CardBase::StudioAbsent, Some(&base_state())),
        Some("studio 'acme-shop' exists now".into())
    );
    // A mismatched action/base pair is reported, never silently open.
    assert_eq!(
        stale(&hpause("t1"), &CardBase::StudioAbsent, Some(&base_state())),
        Some("studio 'acme-shop' exists now".into())
    );
}
