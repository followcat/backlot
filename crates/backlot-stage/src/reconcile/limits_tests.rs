//! Runner limits at reconcile (S9e): a wanted role whose runner is
//! full gets no `Start` — nothing is recorded for the wait — and a
//! freed slot starts the next held role in order on the next pass.

use std::collections::BTreeMap;

use backlot_model::event::*;
use backlot_model::state::*;
use backlot_model::testkit::*;

use super::*;

const STUDIO: &str = "acme-shop";

/// All roles and candidates on `fake`, capped at `cap` live roles.
fn limited_kind(cap: u32) -> EventKind {
    let mut kind = studio_kind();
    let EventKind::StudioCreated {
        roles,
        candidates,
        limits,
        ..
    } = &mut kind
    else {
        unreachable!()
    };
    for r in [
        &mut roles.producer.runner,
        &mut roles.director.runner,
        &mut roles.writer.runner,
    ]
    .into_iter()
    .chain(candidates.iter_mut().map(|c| &mut c.runner))
    {
        *r = "fake".into();
    }
    *limits = BTreeMap::from([("fake".to_string(), cap)]);
    kind
}

/// A cast whose actor runs on `fake` — the cast records the
/// candidate's identity, so the runner is set at cast time.
fn cast_fake(task: &str, name: &str) -> EventKind {
    let mut kind = cast(task, name, "a");
    let EventKind::TaskCast { actor, .. } = &mut kind else {
        unreachable!()
    };
    actor.runner = "fake".into();
    kind
}

fn started(seq: u64, role: &str) -> Event {
    ev(
        seq,
        "stage",
        EventKind::RoleStarted {
            role: role.into(),
            terminal: Some(format!(
                "bl-{STUDIO}-{}",
                role.trim_start_matches("actor-")
                    .rsplit_once("-c")
                    .map(|(t, _)| t)
                    .unwrap_or(role)
            )),
            actor: Some(ActorIdentity {
                runner: "fake".into(),
                runner_version: None,
                model: "m".into(),
                effort: "high".into(),
                permission_profile: "standard".into(),
            }),
            session_id: Some(format!("s-{role}")),
            reason: None,
        },
    )
}

fn msg(seq: u64, to: &str, purpose: &str) -> Event {
    ev(
        seq,
        "producer",
        EventKind::MessageQueued {
            to: to.into(),
            file: None,
            purpose: Some(purpose.into()),
            source_seq: None,
        },
    )
}

fn starts(actions: &[Action]) -> Vec<&str> {
    actions
        .iter()
        .filter_map(|a| match a {
            Action::Start(s) => Some(s.role.as_str()),
            _ => None,
        })
        .collect()
}

/// cap 2 seats producer and director; the wanted writer and cast
/// actor get no Start and record nothing.
#[test]
fn held_starts_produce_no_action() {
    let events = vec![
        ev(1, "user", limited_kind(2)),
        ev(
            2,
            "producer",
            EventKind::ReqVersion {
                version: 1,
                file: "req/v1.md".into(),
                summary: "s".into(),
            },
        ),
        msg(3, "writer", "plan"),
        ev(4, "director", task_created("t1", "feature", 1)),
        ev(5, "director", cast_fake("t1", "actor-a")),
        started(6, "producer"),
        started(7, "director"),
    ];
    let state = replay(&events);
    let live = BTreeMap::from([
        (format!("bl-{STUDIO}-producer"), "a1".to_string()),
        (format!("bl-{STUDIO}-director"), "a2".to_string()),
    ]);
    let actions = reconcile(STUDIO, &state, &live, 10_000);
    assert!(starts(&actions).is_empty(), "{actions:?}");
    // And nothing else: no record, no attention — the hold is silent.
    assert!(actions.is_empty(), "{actions:?}");
}

/// Slot order decides who starts: producer, director, writer, then
/// actors. cap 3 seats the three fixed roles — the actor waits.
#[test]
fn starts_follow_slot_order() {
    let events = vec![
        ev(1, "user", limited_kind(3)),
        ev(
            2,
            "producer",
            EventKind::ReqVersion {
                version: 1,
                file: "req/v1.md".into(),
                summary: "s".into(),
            },
        ),
        msg(3, "writer", "plan"),
        ev(4, "director", task_created("t1", "feature", 1)),
        ev(5, "director", cast_fake("t1", "actor-a")),
    ];
    let state = replay(&events);
    let live = BTreeMap::new();
    let actions = reconcile(STUDIO, &state, &live, 10_000);
    assert_eq!(
        starts(&actions),
        ["producer", "director", "writer"],
        "the actor is held under cap 3"
    );
}

/// A released slot starts the held role next pass: writer live,
/// `dispatch` + `role.closed` frees it, the actor starts.
#[test]
fn a_freed_slot_starts_the_held_role() {
    let base = vec![
        ev(1, "user", limited_kind(3)),
        ev(
            2,
            "producer",
            EventKind::ReqVersion {
                version: 1,
                file: "req/v1.md".into(),
                summary: "s".into(),
            },
        ),
        msg(3, "writer", "plan"),
        ev(4, "director", task_created("t1", "feature", 1)),
        ev(5, "director", cast_fake("t1", "actor-a")),
        started(6, "producer"),
        started(7, "director"),
        started(8, "writer"),
    ];
    // Still held while the writer sits: no actor start.
    let state = replay(&base);
    let live = BTreeMap::from([
        (format!("bl-{STUDIO}-producer"), "a1".to_string()),
        (format!("bl-{STUDIO}-director"), "a2".to_string()),
        (format!("bl-{STUDIO}-writer"), "a3".to_string()),
    ]);
    assert!(starts(&reconcile(STUDIO, &state, &live, 10_000)).is_empty());

    let mut events = base;
    events.push(msg(9, "writer", "dispatch"));
    events.push(ev(
        10,
        "stage",
        EventKind::RoleClosed {
            role: "writer".into(),
            reason: Some("no longer desired".into()),
        },
    ));
    let state = replay(&events);
    let live = BTreeMap::from([
        (format!("bl-{STUDIO}-producer"), "a1".to_string()),
        (format!("bl-{STUDIO}-director"), "a2".to_string()),
    ]);
    let actions = reconcile(STUDIO, &state, &live, 11_000);
    assert_eq!(starts(&actions), ["actor-t1-c1"]);
}
