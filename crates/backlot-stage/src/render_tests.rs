//! `backlot status` rendering for the S9e runner limits: a held
//! wanted role — fixed or actor — shows `waiting for a <runner>
//! slot` instead of its lifecycle word.

use std::collections::BTreeMap;

use backlot_model::event::*;
use backlot_model::state::replay;
use backlot_model::testkit::*;

use crate::render::status;

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

fn started(seq: u64, role: &str) -> Event {
    ev(
        seq,
        "stage",
        EventKind::RoleStarted {
            role: role.into(),
            terminal: Some(format!("bl-acme-shop-{role}")),
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

/// cap 2 seats producer and director live; the wanted writer shows
/// `waiting for a fake slot` in place and the cast actor gets its
/// own waiting line after the fixed three.
#[test]
fn held_roles_show_the_waiting_line() {
    let mut cast_kind = cast("t1", "actor-a", "a");
    let EventKind::TaskCast { actor, .. } = &mut cast_kind else {
        unreachable!()
    };
    actor.runner = "fake".into();
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
        ev(5, "director", cast_kind),
        started(6, "producer"),
        started(7, "director"),
    ];
    let out = status("acme-shop", &replay(&events), 10_000);
    let writer = out
        .lines()
        .find(|l| l.trim_start().starts_with("writer"))
        .unwrap_or_else(|| panic!("no writer line:\n{out}"));
    assert!(writer.contains("waiting for a fake slot"), "{writer}");
    let actor = out
        .lines()
        .find(|l| l.trim_start().starts_with("actor-t1-c1"))
        .unwrap_or_else(|| panic!("no actor line:\n{out}"));
    assert!(actor.contains("waiting for a fake slot"), "{actor}");
}
