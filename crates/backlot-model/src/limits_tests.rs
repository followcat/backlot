//! Runner limits (S9e): `held` names the wanted roles whose start
//! would pass their runner's cap — live roles hold slots, a
//! being-started one counts too, held starts go producer, director,
//! writer, then actors by task priority and seq.

use std::collections::BTreeMap;

use crate::event::*;
use crate::limits::{Held, held};
use crate::refusal::Refusal;
use crate::state::replay;
use crate::testkit::*;
use crate::wanted::wanted_roles;

/// A studio whose roles and candidates all run on `runner`.
fn all_on(runner: &str) -> EventKind {
    let mut kind = studio_kind();
    let EventKind::StudioCreated {
        roles, candidates, ..
    } = &mut kind
    else {
        unreachable!()
    };
    for runner_ref in [
        &mut roles.producer.runner,
        &mut roles.director.runner,
        &mut roles.writer.runner,
    ]
    .into_iter()
    .chain(candidates.iter_mut().map(|c| &mut c.runner))
    {
        *runner_ref = runner.into();
    }
    kind
}

/// `all_on` plus a cap of `cap` live roles on `runner`.
fn limited(runner: &str, cap: u32) -> EventKind {
    let mut kind = all_on(runner);
    let EventKind::StudioCreated { limits, .. } = &mut kind else {
        unreachable!()
    };
    *limits = BTreeMap::from([(runner.to_string(), cap)]);
    kind
}

/// A cast whose actor runs on `runner` — the cast records the
/// candidate's identity at cast time, so the runner is set here.
fn cast_on(task: &str, name: &str, runner: &str) -> EventKind {
    let mut kind = cast(task, name, "a");
    let EventKind::TaskCast { actor, .. } = &mut kind else {
        unreachable!()
    };
    actor.runner = runner.into();
    kind
}

fn started(seq: u64, role: &str, runner: &str) -> Event {
    ev(
        seq,
        "stage",
        EventKind::RoleStarted {
            role: role.into(),
            terminal: Some(format!("bl-acme-shop-{role}")),
            actor: Some(ActorIdentity {
                runner: runner.into(),
                model: "m".into(),
                effort: Some("high".into()),
                permission_profile: "standard".into(),
                runner_version: None,
            }),
            session_id: Some(format!("s-{role}")),
            reason: None,
            prompt_hash: None,
        },
    )
}

fn actor_started(seq: u64, role: &str, runner: &str) -> Event {
    started(seq, role, runner)
}

/// producer + director live fill `fake = 2`; the wanted writer and
/// actor name `fake` as the runner they wait on.
#[test]
fn held_roles_name_the_runner_they_wait_on() {
    let events = vec![
        ev(1, "user", limited("fake", 2)),
        ev(
            2,
            "producer",
            EventKind::ReqVersion {
                version: 1,
                file: "req/v1.md".into(),
                summary: "s".into(),
            },
        ),
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
        ev(4, "director", task_created("t1", "feature", 1)),
        ev(5, "director", cast_on("t1", "actor-a", "fake")),
        started(6, "producer", "fake"),
        started(7, "director", "fake"),
    ];
    let state = replay(&events);
    let held = held(&state);
    assert_eq!(
        held,
        BTreeMap::from([
            ("writer".to_string(), Held::Runner("fake".to_string())),
            ("actor-t1-c1".to_string(), Held::Runner("fake".to_string())),
        ])
    );
}

/// A missing runner is unlimited: caps on `claude` never hold a
/// `fake` start.
#[test]
fn a_runner_with_no_entry_is_unlimited() {
    let mut kind = all_on("fake");
    let EventKind::StudioCreated { limits, .. } = &mut kind else {
        unreachable!()
    };
    *limits = BTreeMap::from([("claude".to_string(), 1u32)]);
    let events = vec![
        ev(1, "user", kind),
        ev(
            2,
            "producer",
            EventKind::ReqVersion {
                version: 1,
                file: "req/v1.md".into(),
                summary: "s".into(),
            },
        ),
    ];
    let state = replay(&events);
    assert!(held(&state).is_empty());
}

/// A being-started role counts: producer live + a cast actor on
/// `fake = 2` — the writer is held before anyone records
/// `role.started` for the actor.
#[test]
fn a_wanted_role_counts_before_it_is_recorded() {
    let events = vec![
        ev(1, "user", limited("fake", 2)),
        ev(
            2,
            "producer",
            EventKind::ReqVersion {
                version: 1,
                file: "req/v1.md".into(),
                summary: "s".into(),
            },
        ),
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
        ev(4, "director", task_created("t1", "feature", 1)),
        ev(5, "director", cast_on("t1", "actor-a", "fake")),
        started(6, "producer", "fake"),
    ];
    let state = replay(&events);
    // Slot order: producer(live), director, writer, actor — director
    // takes slot 2, writer and actor wait.
    let held = held(&state);
    assert_eq!(held.len(), 2, "{held:?}");
    assert!(held.contains_key("writer"), "{held:?}");
    assert!(held.contains_key("actor-t1-c1"), "{held:?}");
}

/// A slot frees when a live role leaves the wanted set — the writer
/// on `dispatch`, an actor on a terminal task. A closed role that is
/// still wanted retakes its own slot, so the test frees the
/// writer's: cap 3 seats the three fixed roles, the actor waits, the
/// dispatch + close hands it the writer's slot.
#[test]
fn a_closed_role_frees_the_next_in_order() {
    let events = vec![
        ev(1, "user", limited("fake", 3)),
        ev(
            2,
            "producer",
            EventKind::ReqVersion {
                version: 1,
                file: "req/v1.md".into(),
                summary: "s".into(),
            },
        ),
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
        ev(4, "director", task_created("t1", "feature", 1)),
        ev(5, "director", cast_on("t1", "actor-a", "fake")),
        started(6, "producer", "fake"),
        started(7, "director", "fake"),
        started(8, "writer", "fake"),
        ev(
            9,
            "producer",
            EventKind::MessageQueued {
                to: "writer".into(),
                file: None,
                purpose: Some("dispatch".into()),
                source_seq: None,
            },
        ),
        ev(
            10,
            "stage",
            EventKind::RoleClosed {
                role: "writer".into(),
                reason: Some("no longer desired".into()),
            },
        ),
    ];
    let state = replay(&events);
    // Before the dispatch the actor waited on the full house; after
    // the writer's close its slot is the actor's.
    let held = held(&state);
    assert!(held.is_empty(), "the actor takes the freed slot: {held:?}");
    let order: Vec<String> = wanted_roles(&state).into_iter().map(|w| w.role).collect();
    assert_eq!(order, ["producer", "director", "actor-t1-c1"]);
}

/// Actor starts order by task priority high first, then the task's
/// seq — two open actors under one free slot: the high-priority one
/// goes, the normal one waits.
#[test]
fn actors_order_by_priority_then_task_seq() {
    let events = vec![
        ev(1, "user", limited("fake", 3)),
        ev(
            2,
            "producer",
            EventKind::ReqVersion {
                version: 1,
                file: "req/v1.md".into(),
                summary: "s".into(),
            },
        ),
        ev(3, "director", task_created("t1", "feature", 1)),
        ev(4, "director", task_created("t2", "feature", 1)),
        ev(5, "director", cast_on("t2", "actor-b", "fake")),
        ev(6, "director", cast_on("t1", "actor-a", "fake")),
        ev(
            7,
            "director",
            EventKind::TaskPriority {
                task: "t1".into(),
                priority: Priority::High,
            },
        ),
        started(8, "producer", "fake"),
        started(9, "director", "fake"),
    ];
    let state = replay(&events);
    let order: Vec<String> = wanted_roles(&state).into_iter().map(|w| w.role).collect();
    assert_eq!(
        order,
        ["producer", "director", "actor-t1-c1", "actor-t2-c1"]
    );
    let held = held(&state);
    assert_eq!(
        held,
        BTreeMap::from([("actor-t2-c1".to_string(), Held::Runner("fake".to_string()))])
    );
}

/// The panel's summary shows a held wanted role `waiting` with the
/// slot line — a fixed role's recorded row is overridden, an actor
/// never started gets a synthetic one.
#[test]
fn held_roles_show_waiting_in_the_summary() {
    let events = vec![
        ev(1, "user", limited("fake", 2)),
        ev(
            2,
            "producer",
            EventKind::ReqVersion {
                version: 1,
                file: "req/v1.md".into(),
                summary: "s".into(),
            },
        ),
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
        ev(4, "director", task_created("t1", "feature", 1)),
        ev(5, "director", cast_on("t1", "actor-a", "fake")),
        started(6, "producer", "fake"),
        started(7, "director", "fake"),
        // A writer that started and closed still waits the same way.
        started(8, "writer", "fake"),
        ev(
            9,
            "stage",
            EventKind::RoleClosed {
                role: "writer".into(),
                reason: None,
            },
        ),
    ];
    let state = replay(&events);
    let s = crate::summary::summary(
        &[("acme-shop".into(), state)],
        &BTreeMap::new(),
        &BTreeMap::new(),
        &BTreeMap::new(),
        &BTreeMap::new(),
        1,
    );
    let roles = &s.studios[0].roles;
    let writer = roles.iter().find(|r| r.role == "writer").unwrap();
    assert_eq!(writer.state, "waiting");
    assert_eq!(writer.doing.as_deref(), Some("waiting for a fake slot"));
    assert_eq!(writer.running, None);
    let actor = roles
        .iter()
        .find(|r| r.role == "actor-t1-c1")
        .unwrap_or_else(|| panic!("no synthetic actor row: {roles:?}"));
    assert_eq!(actor.state, "waiting");
    assert_eq!(actor.doing.as_deref(), Some("waiting for a fake slot"));
    assert_eq!(actor.running, None);
    assert_eq!(actor.name.as_str(), "actor-a");
    assert_eq!(actor.runner.as_deref(), Some("fake"));
    // The live two are untouched.
    assert_eq!(
        roles.iter().find(|r| r.role == "producer").unwrap().state,
        "live"
    );
}

/// Live roles on `fake` in ANOTHER studio never touch this one's
/// count — `held` only ever reads the state it is given.
#[test]
fn limits_are_per_studio() {
    // This studio's own ledger: only producer live on fake.
    let events = vec![
        ev(1, "user", limited("fake", 2)),
        ev(
            2,
            "producer",
            EventKind::ReqVersion {
                version: 1,
                file: "req/v1.md".into(),
                summary: "s".into(),
            },
        ),
        actor_started(3, "producer", "fake"),
    ];
    let state = replay(&events);
    assert!(held(&state).is_empty(), "one free slot remains");
}

/// `studio_kind()` plus a studio-wide actor cap — every runner stays
/// unlimited, so only the actor cap can hold a start.
fn capped(max: u32) -> EventKind {
    let mut kind = all_on("fake");
    let EventKind::StudioCreated { max_actors, .. } = &mut kind else {
        unreachable!()
    };
    *max_actors = max;
    kind
}

fn dispatch(seq: u64) -> Event {
    ev(
        seq,
        "producer",
        EventKind::MessageQueued {
            to: "writer".into(),
            file: None,
            purpose: Some("dispatch".into()),
            source_seq: None,
        },
    )
}

/// Two candidates cannot play more than the cap: the fifth wanted
/// actor waits on an actor slot and takes it when one actor closes.
#[test]
fn the_actor_cap_holds_the_fifth_wanted_actor_and_frees_it() {
    let mut events = vec![ev(1, "user", capped(4)), dispatch(2)];
    let mut seq = 2;
    for i in 1..=5 {
        seq += 1;
        events.push(ev(
            seq,
            "director",
            task_created(&format!("t{i}"), "feature", 1),
        ));
        seq += 1;
        events.push(ev(
            seq,
            "director",
            cast_on(&format!("t{i}"), "actor-a", "fake"),
        ));
    }
    for role in ["producer", "director"] {
        seq += 1;
        events.push(started(seq, role, "fake"));
    }
    for i in 1..=4 {
        seq += 1;
        events.push(started(seq, &format!("actor-t{i}-c1"), "fake"));
    }
    let state = replay(&events);
    assert_eq!(
        held(&state).get("actor-t5-c1"),
        Some(&Held::Actor),
        "the fifth actor waits on the cap: {:?}",
        held(&state)
    );
    assert!(!held(&state).contains_key("actor-t1-c1"));

    // One actor finishes: its task is abandoned and the stage closes
    // the role, freeing the slot the fifth takes.
    seq += 1;
    events.push(ev(
        seq,
        "director",
        task_state("t1", TaskStateChange::Abandoned, "done"),
    ));
    seq += 1;
    events.push(ev(
        seq,
        "stage",
        EventKind::RoleClosed {
            role: "actor-t1-c1".into(),
            reason: Some("no longer desired".into()),
        },
    ));
    let state = replay(&events);
    assert!(
        held(&state).is_empty(),
        "the fifth actor takes the freed slot: {:?}",
        held(&state)
    );
}

/// Fixed roles never fill an actor slot: with cap 1 and the producer,
/// director and writer all live, the studio's one candidate still
/// starts.
#[test]
fn fixed_roles_do_not_count_against_the_actor_cap() {
    let events = vec![
        ev(1, "user", capped(1)),
        ev(
            2,
            "producer",
            EventKind::ReqVersion {
                version: 1,
                file: "req/v1.md".into(),
                summary: "s".into(),
            },
        ),
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
        ev(4, "director", task_created("t1", "feature", 1)),
        ev(5, "director", cast_on("t1", "actor-a", "fake")),
        started(6, "producer", "fake"),
        started(7, "director", "fake"),
        started(8, "writer", "fake"),
    ];
    let state = replay(&events);
    assert!(
        held(&state).is_empty(),
        "the fixed three fill no actor slot: {:?}",
        held(&state)
    );
}

/// A `studio.cast` setting `max_actors` below one is refused; the cap
/// travels in the event and moves the wanted set.
#[test]
fn a_cast_below_one_refuses_and_a_new_cap_moves_the_wanted_set() {
    let state = replay(&base_events());
    let mut next = crate::cast::CastSettings::of(&state).unwrap();
    next.max_actors = 0;
    assert_eq!(
        crate::cast::check(&state, &next),
        Err(Refusal::CastBadMaxActors)
    );
    next.max_actors = 7;
    assert!(crate::cast::check(&state, &next).is_ok());
}
