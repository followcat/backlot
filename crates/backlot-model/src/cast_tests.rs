//! `studio.cast` — a studio's cast after creation: the reducer takes the
//! newest settings, the append rule is the cast file's runnable half, and
//! the diff and the printed cast file round-trip.

use std::collections::BTreeMap;

use crate::cast::{self, CastDiff, CastSettings};
use crate::event::*;
use crate::state::{StudioState, replay};
use crate::testkit::*;
use crate::validate::{Refusal, validate};

/// The cast a `studio_kind()` studio plays, as settings.
fn base() -> CastSettings {
    let state = replay(&base_events());
    CastSettings::of(&state).expect("a studio")
}

/// A copy of the base cast with the director on a new identity.
fn director_moved() -> CastSettings {
    let mut s = base();
    s.roles.director = actor("dir2");
    s
}

/// The newest `studio.cast` wins: roles, candidates, limits and timings
/// move, and earlier events keep what they recorded.
#[test]
fn the_newest_cast_settings_win_and_the_past_is_untouched() {
    let mut events = base_events();
    let mut next = base();
    next.candidates = vec![candidate("actor-c", "c")];
    next.rework_cap = 4;
    next.task_budget_s = Some(60);
    next.limits = BTreeMap::from([("runner-a".to_string(), 2)]);
    events.push(ev(10, "user", next.event_kind(None)));
    let state = replay(&events);
    let cfg = state.studio.as_ref().unwrap();
    assert_eq!(cfg.candidates, vec![candidate("actor-c", "c")]);
    assert_eq!(cfg.rework_cap, 4);
    assert_eq!(cfg.task_budget_s, Some(60));
    assert_eq!(cfg.limits["runner-a"], 2);
    // The studio's binding is not cast: the repo stays as created.
    assert_eq!(cfg.repo, "/home/user/acme-shop");
    assert_eq!(cfg.mode, StudioMode::Standard);
    // Earlier events keep what they recorded — the `studio.created`
    // line still carries the original cast.
    match &events[0].kind {
        EventKind::StudioCreated { rework_cap, .. } => assert_eq!(*rework_cap, 2),
        k => panic!("expected studio.created, got {k:?}"),
    }
    // The settings event's seq is the newest cast.
    assert_eq!(state.cast_seq, Some(10));
}

/// A `studio.cast` from a role terminal is refused: it is the person's.
#[test]
fn a_role_terminal_cannot_change_the_cast() {
    let state = replay(&base_events());
    let kind = base().event_kind(None);
    for role in ["producer", "director", "writer", "actor-t1-c1"] {
        assert_eq!(
            validate(&state, &new_ev(role, kind.clone())),
            Err(Refusal::RoleBound {
                role: role.into(),
                who: "the user",
            }),
            "{role}"
        );
    }
}

/// Nothing changes is refused, and the effective cast — a recast role
/// included — is what "nothing" means.
#[test]
fn an_unchanged_cast_is_refused() {
    let state = replay(&base_events());
    assert_eq!(cast::check(&state, &base()), Err(Refusal::CastUnchanged));
    // A fixed role the file spells as its newest recast changed nothing.
    let mut events = base_events();
    events.push(ev(
        10,
        "user",
        EventKind::RoleRecast {
            role: "director".into(),
            actor: actor("dir2"),
            reason: None,
            card: None,
        },
    ));
    let state = replay(&events);
    let mut same = base();
    same.roles.director = actor("dir2");
    assert_eq!(
        cast::check(&state, &same),
        Err(Refusal::CastUnchanged),
        "the file spells the identity the role already plays"
    );
}

/// A zero limit, a bad effort and an unseated cap are the runnable
/// refusals — nothing else stops a cast change.
#[test]
fn the_runnable_refusals_hold() {
    let state = replay(&base_events());
    let mut zero = director_moved();
    zero.limits = BTreeMap::from([("runner-a".to_string(), 0)]);
    assert_eq!(
        cast::check(&state, &zero),
        Err(Refusal::CastBadLimit {
            runner: "runner-a".into()
        })
    );

    let mut no_effort = director_moved();
    no_effort.roles.director.effort = None;
    assert!(matches!(
        cast::check(&state, &no_effort),
        Err(Refusal::EffortRequired { .. })
    ));

    // caps at the fixed count while a candidate shares the runner.
    let mut unseated = base();
    unseated.candidates = vec![candidate("actor-c", "c")];
    unseated.roles.producer.runner = "fake".into();
    unseated.roles.director.runner = "fake".into();
    unseated.roles.director.effort = Some("high".into());
    unseated.candidates[0].runner = "fake".into();
    unseated.limits = BTreeMap::from([("fake".to_string(), 2)]);
    assert_eq!(
        cast::check(&state, &unseated),
        Err(Refusal::LimitsUnseated {
            runner: "fake".into(),
            cap: 2
        })
    );
}

/// A finished studio takes no cast change.
#[test]
fn a_finished_studio_refuses_a_cast_change() {
    let state = state_with(
        &base_events(),
        vec![(
            "user",
            EventKind::StudioFinished {
                reason: "done".into(),
            },
        )],
    );
    assert_eq!(
        cast::check(&state, &director_moved()),
        Err(Refusal::AlreadyFinished)
    );
}

/// The diff names the moved role, the added and removed candidate, the
/// limit and the timing — in a stable order a person reads.
#[test]
fn the_diff_reads_the_changes() {
    let mut next = director_moved();
    next.candidates = vec![candidate("actor-a", "a"), candidate("actor-c", "c")];
    next.limits = BTreeMap::from([("runner-a".to_string(), 3)]);
    next.rework_cap = 5;
    let d = cast::diff(&base(), &next);
    let lines = d.lines();
    assert!(
        lines
            .iter()
            .any(|l| l.starts_with("director: ") && l.contains("→ runner-dir2/model-dir2/high")),
        "{lines:?}"
    );
    assert!(
        lines.iter().any(|l| l.starts_with("candidate - actor-b")),
        "{lines:?}"
    );
    assert!(
        lines.iter().any(|l| l.starts_with("candidate + actor-c")),
        "{lines:?}"
    );
    assert!(
        lines.iter().any(|l| l == "limits + runner-a 3"),
        "{lines:?}"
    );
    assert!(lines.iter().any(|l| l == "rework_cap 2 → 5"), "{lines:?}");
    assert_eq!(
        d.moved_roles().collect::<Vec<_>>(),
        vec![("director", &actor("dir2"))]
    );
    assert!(CastDiff::default().is_empty());
}

/// The printed cast file carries every field, and printing twice is the
/// same text. (The round trip through `studio new`'s parser is the
/// stage's test — the model crate has no TOML reader.)
#[test]
fn the_printed_cast_carries_every_field() {
    let mut s = base();
    s.candidates = vec![candidate("actor-c", "c")];
    s.limits = BTreeMap::from([("runner-a".to_string(), 2)]);
    s.task_budget_s = Some(30);
    s.check_env = vec!["CI".into()];
    s.checks = vec![check_decl("units", "cargo test --lib", 120)];
    s.push.protected_branches = vec!["main".into(), "release*".into()];
    let text = cast::to_toml(&s);
    for needle in [
        "rework_cap = 2",
        "task_budget_s = 30",
        "check_env = [\"CI\"]",
        "[limits]",
        "runner-a = 2",
        "[producer]",
        "[director]",
        "[writer]",
        "[[candidates]]",
        "name = \"actor-c\"",
        "[[checks]]",
        "run = \"cargo test --lib\"",
        "timeout_s = 120",
        "[push]",
        "protected_branches = [\"main\", \"release*\"]",
    ] {
        assert!(text.contains(needle), "missing {needle:?} in:\n{text}");
    }
    assert_eq!(cast::to_toml(&s), text, "print is stable");
}

/// An actor-key recast keeps the key and moves the task's cast identity;
/// the deliveries and the budget clock stay.
#[test]
fn an_actor_recast_moves_the_cast_identity_and_keeps_the_record() {
    let mut events = base_events();
    events.push(ev(10, "director", cast("t1", "actor-a", "a")));
    events.push(ev(11, "actor-t1-c1", delivered("t1", 1, "a", 1)));
    let key = "actor-t1-c1".to_string();
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
            role: key.clone(),
            actor: new.clone(),
            reason: Some("quota".into()),
            card: None,
        },
    ));
    let state = replay(&events);
    let t = &state.tasks["t1"];
    assert_eq!(
        t.cast.as_ref().unwrap().actor,
        new,
        "the current cast moved"
    );
    assert_eq!(t.casts.len(), 1, "no new cast record — the key stays");
    assert_eq!(
        t.deliveries[0].actor,
        actor("a"),
        "the delivery keeps its actor"
    );
    assert_eq!(t.rework_rounds, 0);
}

/// An actor key that is not the task's newest cast is refused.
#[test]
fn a_stale_actor_key_is_refused() {
    let mut events = base_events();
    events.push(ev(10, "director", cast("t1", "actor-a", "a")));
    events.push(ev(11, "director", cast("t1", "actor-b", "b")));
    let state = replay(&events);
    let mut new = actor("x");
    new.runner = "cmd".into();
    new.effort = None;
    let err = crate::recast::check(&state, "actor-t1-c1", &new).unwrap_err();
    assert_eq!(err, Refusal::NotCurrentActor("actor-t1-c1".into()));
    // The current key works.
    assert!(crate::recast::check(&state, "actor-t1-c2", &new).is_ok());
}

/// An old log with no `studio.cast` loads exactly as before.
#[test]
fn an_old_ledger_without_a_cast_change_loads() {
    let state: StudioState = replay(&base_events());
    assert_eq!(state.cast_seq, None);
    assert_eq!(state.studio.as_ref().unwrap().rework_cap, 2);
}
