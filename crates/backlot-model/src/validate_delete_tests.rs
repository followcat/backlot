//! Validation and replay tests for `studio.deleted`.

use crate::event::*;
use crate::state::replay;
use crate::testkit::*;
use crate::validate::{Refusal, validate};

#[test]
fn studio_deleted_requires_finished_and_closes_all_roles() {
    let mut events = base_events();
    events.push(ev(
        4,
        "stage",
        EventKind::RoleStarted {
            role: "producer".into(),
            terminal: Some("t1".into()),
            actor: None,
            session_id: Some("s1".into()),
            reason: None,
        },
    ));
    events.push(ev(
        5,
        "director",
        task_state("t1", TaskStateChange::Abandoned, "cut"),
    ));
    events.push(ev(
        6,
        "director",
        task_state("t2", TaskStateChange::Abandoned, "cut"),
    ));
    let live_state = replay(&events);
    let kind = EventKind::StudioDeleted {
        archive: "archive/acme-20261123T060000Z".into(),
    };
    assert_eq!(
        validate(&live_state, &new_ev("user", kind.clone())),
        Err(Refusal::NotFinished)
    );
    events.push(ev(
        7,
        "user",
        EventKind::StudioFinished {
            reason: "deleted".into(),
        },
    ));
    let finished = replay(&events);
    assert_eq!(validate(&finished, &new_ev("user", kind.clone())), Ok(()));

    events.push(ev(8, "user", kind));
    let deleted = replay(&events);
    let p = &deleted.roles["producer"];
    assert!(!p.live);
    assert!(p.closed);
    assert!(!p.held);
    assert_eq!(
        deleted.targets.get(&8),
        Some(&crate::correct::SeqTarget::Studio)
    );
}
