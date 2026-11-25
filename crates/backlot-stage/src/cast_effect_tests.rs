//! What a cast change does to the stage: a live actor is untouched by a
//! candidate leaving, a new limit applies to the next start only, a new
//! check runs on the next delivery, and an actor recast keeps the key
//! and the worktree while it swaps the conversation.

use std::collections::BTreeMap;

use backlot_model::event::*;
use backlot_model::state::*;
use backlot_model::testkit::*;

use crate::checks;

const STUDIO: &str = "acme-shop";

/// A `studio.cast` that drops every candidate, by: user.
fn drop_candidates() -> Event {
    ev(
        20,
        "user",
        EventKind::StudioCast {
            roles: Box::new(RoleConfigs {
                producer: actor("prod"),
                director: actor("dir"),
                writer: actor("wri"),
            }),
            candidates: Vec::new(),
            rework_cap: 2,
            stall_limit_s: 600,
            nudge_after_s: 180,
            escalate_after_s: 600,
            check_env: Vec::new(),
            checks: Vec::new(),
            task_budget_s: None,
            limits: BTreeMap::new(),
            push: PushPolicy::default(),
            card: None,
        },
    )
}

/// Dropping the candidate a running actor came from leaves the actor
/// wanted, live, on the same key and identity — it only stops future
/// picks.
#[test]
fn a_removed_candidate_does_not_touch_a_live_actor() {
    let mut events = base_events();
    events.push(ev(10, "director", cast("t1", "actor-a", "a")));
    events.push(ev(
        11,
        "stage",
        EventKind::RoleStarted {
            role: "actor-t1-c1".into(),
            terminal: Some("bl-acme-shop-t1".into()),
            actor: Some(actor("a")),
            session_id: Some("s-a".into()),
            reason: None,
        },
    ));
    events.push(drop_candidates());
    let state = replay(&events);
    // The studio has no candidates left …
    assert!(state.studio.as_ref().unwrap().candidates.is_empty());
    // … but the task's actor is unchanged, and the wanted set still has
    // the live key.
    let t = &state.tasks["t1"];
    assert_eq!(t.cast.as_ref().unwrap().actor, actor("a"));
    let wanted = backlot_model::wanted::wanted_roles(&state);
    let w = wanted
        .iter()
        .find(|w| w.role == "actor-t1-c1")
        .expect("the live actor stays wanted");
    assert_eq!(w.actor, actor("a"));
    // The role stays live — no close is desired for it.
    assert!(state.roles["actor-t1-c1"].live);
}

/// A new cap applies to the next start: a role already live above the
/// new cap is not stopped, and a wanted start past the cap waits.
#[test]
fn a_new_limit_applies_only_to_the_next_start() {
    // Two actors live, both on `fake`; the new cast caps `fake` at 1.
    let mut events = base_events();
    events.push(ev(10, "director", task_created("t2", "fix", 1)));
    events.push(ev(11, "director", cast("t1", "actor-a", "a")));
    events.push(ev(12, "director", cast("t2", "actor-b", "b")));
    // The candidates share the runner `fake` so a cap can apply.
    let mut cast_kind = studio_kind();
    if let EventKind::StudioCreated {
        candidates, roles, ..
    } = &mut cast_kind
    {
        for c in candidates.iter_mut() {
            c.runner = "fake".into();
        }
        for r in [&mut roles.producer, &mut roles.director, &mut roles.writer] {
            r.runner = "fake".into();
        }
    }
    events[0] = ev(1, "user", cast_kind);
    events.push(ev(
        13,
        "stage",
        EventKind::RoleStarted {
            role: "actor-t1-c1".into(),
            terminal: Some("bl-acme-shop-t1".into()),
            actor: Some(actor("a")),
            session_id: Some("s-a".into()),
            reason: None,
        },
    ));
    events.push(ev(
        14,
        "stage",
        EventKind::RoleStarted {
            role: "actor-t2-c1".into(),
            terminal: Some("bl-acme-shop-t2".into()),
            actor: Some(actor("b")),
            session_id: Some("s-b".into()),
            reason: None,
        },
    ));
    let state = replay(&events);
    // Both actors are live and hold the whole cap; a *new* cast caps
    // `fake` at 1 without stopping either.
    let mut next = backlot_model::cast::CastSettings::of(&state).unwrap();
    next.limits = BTreeMap::from([("fake".to_string(), 1)]);
    // A cap at 1 leaves the fixed roles unseated, so use the smallest
    // legal set: all fixed on `claude`, candidates on `fake`.
    let mut roles = next.roles.clone();
    for r in [&mut roles.producer, &mut roles.director, &mut roles.writer] {
        r.runner = "claude".into();
        r.effort = Some("high".into());
        r.model = "m".into();
    }
    next.roles = roles;
    next.limits = BTreeMap::from([("fake".to_string(), 1)]);
    events.push(ev(15, "user", next.event_kind(None)));
    let state = replay(&events);
    // Neither live actor was closed by the reducer.
    assert!(state.roles["actor-t1-c1"].live);
    assert!(state.roles["actor-t2-c1"].live);
    assert_eq!(state.studio.as_ref().unwrap().limits["fake"], 1);
    // The next wanted starts read the new cap: a fresh `t3` actor on
    // `fake` would be held. The live ones still count.
    let held = backlot_model::limits::held(&state);
    assert!(
        !held.contains_key("actor-t1-c1") && !held.contains_key("actor-t2-c1"),
        "a live actor is not held: {held:?}"
    );
}

/// A new check is owed on the delivery made after the change; the
/// delivery before it keeps only what it was delivered under.
#[test]
fn a_new_check_runs_on_the_next_delivery() {
    let mut events = base_events();
    events.push(ev(10, "director", cast("t1", "actor-a", "a")));
    events.push(ev(11, "actor-t1-c1", delivered("t1", 1, "a", 1)));
    // Delivery 1 is reviewed back for changes — the task runs again, and
    // no check ran on delivery 1 (the cast declared none).
    events.push(ev(
        12,
        "director",
        reviewed("t1", 1, Verdict::ChangesRequested, vec![]),
    ));
    // The studio's first check arrives.
    let mut next = {
        let state = replay(&events);
        backlot_model::cast::CastSettings::of(&state).unwrap()
    };
    next.checks = vec![check_decl("units", "cargo test", 60)];
    events.push(ev(13, "user", next.event_kind(None)));
    let state = replay(&events);
    // Delivery 1 predates the check: nothing is owed for it.
    assert!(
        checks::owed(&state).is_none(),
        "the check must not reach back to delivery 1"
    );
    let t = &state.tasks["t1"];
    assert!(
        t.checked_of(1).next().is_none(),
        "delivery 1 gained a check"
    );
    // Delivery 2 owes it.
    events.push(ev(14, "actor-t1-c1", delivered("t1", 2, "a", 1)));
    let state = replay(&events);
    let owed = checks::owed(&state).expect("delivery 2 owes the new check");
    assert_eq!(owed.check.name, "units");
    assert_eq!(owed.delivery, 2);
}

/// An actor recast keeps the key, its terminal name and its work area
/// (the task's `work/<task>`), and starts a fresh conversation under the
/// new identity.
#[test]
fn an_actor_recast_keeps_the_key_and_worktree_and_starts_fresh() {
    let mut events = base_events();
    events.push(ev(10, "director", cast("t1", "actor-a", "a")));
    events.push(ev(
        11,
        "stage",
        EventKind::RoleStarted {
            role: "actor-t1-c1".into(),
            terminal: Some("bl-acme-shop-t1".into()),
            actor: Some(actor("a")),
            session_id: Some("s-a".into()),
            reason: None,
        },
    ));
    let new = ActorIdentity {
        runner: "fake".into(),
        runner_version: None,
        model: "new".into(),
        effort: Some("high".into()),
        permission_profile: "worktree".into(),
    };
    events.push(ev(
        12,
        "user",
        EventKind::RoleRecast {
            role: "actor-t1-c1".into(),
            actor: new.clone(),
            reason: Some("quota".into()),
            card: None,
        },
    ));
    let state = replay(&events);
    // The task's identity moved; the key and its terminal name do not.
    assert_eq!(state.tasks["t1"].cast.as_ref().unwrap().actor, new);
    // The reconcile tick closes the old terminal on purpose and starts
    // it fresh under the same key. With the terminal gone the start
    // carries the new identity and no resume.
    let after_close = {
        let mut e = events.clone();
        e.push(ev(
            13,
            "stage",
            EventKind::RoleClosed {
                role: "actor-t1-c1".into(),
                reason: Some("recast".into()),
            },
        ));
        replay(&e)
    };
    let actions = crate::reconcile::reconcile(STUDIO, &after_close, &BTreeMap::new(), 0);
    let start = actions
        .iter()
        .find_map(|a| match a {
            crate::reconcile::Action::Start(s) if s.role == "actor-t1-c1" => Some(s),
            _ => None,
        })
        .expect("the recast actor starts");
    assert_eq!(start.actor, new);
    assert_eq!(start.terminal, "bl-acme-shop-t1");
    assert!(
        start.resume.is_none(),
        "a recast starts a fresh conversation"
    );
    // Its work area is the task's own worktree, the same as before.
    let tmp = crate::test_support::TestDir::new("backlot-cast-effect");
    let dir = tmp.as_ref();
    assert_eq!(
        crate::workareas::role_dir(dir, &start.repo, "actor-t1-c1", false),
        dir.join("work").join("t1")
    );
    // The start's prompt says it takes the task over.
    let text = crate::prompts::prepare(dir, STUDIO, start, &after_close).unwrap();
    let text = std::fs::read_to_string(text).unwrap();
    assert!(text.contains("Taking over this role"), "{text}");
    assert!(text.contains("commits already on"), "{text}");
}
