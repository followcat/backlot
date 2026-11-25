//! `role.recast`: the event, the reduction, the append-time rule, and
//! the card base it pairs with.

use std::collections::BTreeMap;

use crate::event::*;
use crate::hollywood::{
    CardAction, CardBase, HollywoodKind, HollywoodState, NewHollywoodEvent, stale,
};
use crate::recast;
use crate::state::{RoleLifecycle, apply, replay};
use crate::testkit::*;
use crate::validate::{Refusal, validate};

/// A fixed role's new identity on a real runner the model's rule knows.
fn cmd(tag: &str) -> ActorIdentity {
    ActorIdentity {
        runner: "cmd".into(),
        runner_version: None,
        model: format!("model-{tag}"),
        effort: None,
        permission_profile: "standard".into(),
    }
}

fn recast_event(role: &str, actor: ActorIdentity) -> EventKind {
    EventKind::RoleRecast {
        role: role.into(),
        actor,
        reason: Some("quota".into()),
        card: None,
    }
}

/// The reduced studio uses the newest identity for the role from then
/// on, while the lifecycle record keeps the identity it was started
/// with.
#[test]
fn a_recast_changes_the_fixed_identity_and_leaves_the_past_alone() {
    let mut state = replay(&base_events());
    apply(
        &mut state,
        &ev(
            10,
            "stage",
            EventKind::RoleStarted {
                role: "director".into(),
                terminal: Some("bl-acme-shop-director".into()),
                actor: Some(actor("dir")),
                session_id: Some("s1".into()),
                reason: None,
            },
        ),
    );
    apply(
        &mut state,
        &ev(11, "user", recast_event("director", cmd("d"))),
    );
    assert_eq!(state.fixed_identity("director"), Some(&cmd("d")));
    assert_eq!(state.recasts.len(), 1);
    // The started identity and its session are untouched: only the
    // newest event changes what `desired` launches.
    let role = &state.roles["director"];
    assert_eq!(role.actor, Some(actor("dir")));
    assert_eq!(role.session_id.as_deref(), Some("s1"));
    assert_eq!(role.lifecycle(), RoleLifecycle::Live);
    // The other fixed roles are unchanged.
    assert_eq!(state.fixed_identity("producer"), Some(&actor("prod")));
}

/// An older ledger with no recast replays exactly as before.
#[test]
fn a_ledger_without_a_recast_is_unchanged() {
    let state = replay(&base_events());
    assert!(state.recasts.is_empty());
    assert_eq!(state.fixed_identity("producer"), Some(&actor("prod")));
    assert_eq!(state.fixed_identity("director"), Some(&actor("dir")));
    assert_eq!(state.fixed_identity("writer"), Some(&actor("wri")));
    assert_eq!(state.fixed_identity("actor-t1-c1"), None);
}

/// A recast is user-only at the append boundary — a role terminal
/// cannot record one.
#[test]
fn a_role_terminal_cannot_recast() {
    let state = replay(&base_events());
    let event = new_ev("producer", recast_event("director", cmd("d")));
    assert_eq!(
        validate(&state, &event),
        Err(Refusal::RoleBound {
            role: "producer".into(),
            who: "the user",
        })
    );
    assert_eq!(
        validate(&state, &new_ev("user", recast_event("director", cmd("d")))),
        Ok(())
    );
}

/// An actor key names a person once the task is cast — its newest
/// cast's key. A key that mints no cast yet is not a person.
#[test]
fn an_actor_key_becomes_a_person_once_cast() {
    // No cast on t1: the key names nobody.
    let state = replay(&base_events());
    let err = recast::check(&state, "actor-t1-c1", &cmd("d")).unwrap_err();
    assert_eq!(err, Refusal::NotCurrentActor("actor-t1-c1".into()));
    // A cast of t1 makes `actor-t1-c1` the person to recast.
    let state = state_with(
        &base_events(),
        vec![("director", cast("t1", "actor-a", "a"))],
    );
    assert!(recast::check(&state, "actor-t1-c1", &cmd("d")).is_ok());
}

/// A finished studio takes no recast.
#[test]
fn a_finished_studio_refuses_a_recast() {
    let state = state_with(
        &base_events(),
        vec![(
            "user",
            EventKind::StudioFinished {
                reason: "done".into(),
            },
        )],
    );
    let err = recast::check(&state, "director", &cmd("d")).unwrap_err();
    assert_eq!(err, Refusal::AlreadyFinished);
}

/// The effort refusals read in plain words from the caps: a runner
/// with no effort setting says to drop it, a required runner names
/// its levels and the flag to add.
#[test]
fn the_effort_refusals_read_in_plain_words() {
    let state = replay(&base_events());
    let mut a = cmd("d");
    a.runner = "cursor".into();
    a.effort = Some("high".into());
    assert_eq!(
        recast::check(&state, "director", &a)
            .unwrap_err()
            .to_string(),
        "cursor has no effort setting; drop --effort"
    );
    let mut a = cmd("d");
    a.runner = "claude".into();
    a.effort = None;
    assert_eq!(
        recast::check(&state, "director", &a)
            .unwrap_err()
            .to_string(),
        "claude takes an effort (low, medium, high, xhigh, max); add --effort"
    );
}

/// A runner with no adapter, an effort the runner does not take (or
/// needs), a cline model, and an unknown profile each refuse with
/// their own line.
#[test]
fn the_identity_rules_refuse() {
    let state = replay(&base_events());
    let mut a = cmd("d");
    a.runner = "runner-x".into();
    assert_eq!(
        recast::check(&state, "director", &a),
        Err(Refusal::RunnerNotSupported("runner-x".into()))
    );

    let mut a = cmd("d");
    a.runner = "cursor".into();
    a.effort = Some("high".into());
    assert_eq!(
        recast::check(&state, "director", &a),
        Err(Refusal::EffortNotAllowed {
            runner: "cursor".into()
        })
    );

    let mut a = cmd("d");
    a.runner = "claude".into();
    a.effort = None;
    assert_eq!(
        recast::check(&state, "director", &a),
        Err(Refusal::EffortRequired {
            runner: "claude".into()
        })
    );

    let mut a = cmd("d");
    a.runner = "cline".into();
    a.model = "deepseek".into();
    a.effort = Some("low".into());
    assert_eq!(
        recast::check(&state, "director", &a),
        Err(Refusal::ModelNotSupported {
            runner: "cline".into(),
            model: "deepseek".into(),
        })
    );

    let mut a = cmd("d");
    a.permission_profile = "sandbox".into();
    assert_eq!(
        recast::check(&state, "director", &a),
        Err(Refusal::ProfileNotSupported("sandbox".into()))
    );
}

/// The cap must seat the fixed roles after the change: two fixed
/// roles on `claude` at a cap of two leave the writer (also on
/// `claude`) nowhere to sit.
#[test]
fn a_cap_that_leaves_no_fixed_slot_refuses() {
    let mut kind = studio_kind();
    if let EventKind::StudioCreated {
        roles,
        limits,
        candidates,
        ..
    } = &mut kind
    {
        let claude = |model: &str| ActorIdentity {
            runner: "claude".into(),
            runner_version: None,
            model: model.into(),
            effort: Some("high".into()),
            permission_profile: "standard".into(),
        };
        roles.producer = claude("a");
        roles.director = claude("b");
        roles.writer = claude("c");
        candidates.clear();
        *limits = BTreeMap::from([("claude".to_string(), 2)]);
    }
    let state = replay(&[ev(1, "user", kind)]);
    let mut a = cmd("d");
    a.runner = "claude".into();
    a.effort = Some("high".into());
    a.model = "d".into();
    assert_eq!(
        recast::check(&state, "director", &a),
        Err(Refusal::LimitsUnseated {
            runner: "claude".into(),
            cap: 2,
        })
    );
}

/// A recast that changes nothing refuses.
#[test]
fn an_unchanged_identity_refuses() {
    let state = replay(&base_events());
    assert_eq!(
        recast::check(&state, "director", &actor("dir")),
        Err(Refusal::RecastUnchanged("director".into()))
    );
    // `runner_version` is the stage's reading, not a choice.
    let mut same = actor("dir");
    same.runner_version = Some("9.9.9".into());
    assert_eq!(
        recast::check(&state, "director", &same),
        Err(Refusal::RecastUnchanged("director".into()))
    );
}

/// A replying `role.started` under a new identity drops the old
/// session id — it belongs to the runner that left.
#[test]
fn a_start_under_a_new_identity_clears_the_old_session() {
    let mut state = replay(&base_events());
    apply(
        &mut state,
        &ev(
            10,
            "stage",
            EventKind::RoleStarted {
                role: "director".into(),
                terminal: Some("t1".into()),
                actor: Some(actor("dir")),
                session_id: Some("s-old".into()),
                reason: None,
            },
        ),
    );
    apply(
        &mut state,
        &ev(
            11,
            "stage",
            EventKind::RoleStarted {
                role: "director".into(),
                terminal: Some("t1".into()),
                actor: Some(cmd("d")),
                session_id: None,
                reason: None,
            },
        ),
    );
    assert_eq!(state.roles["director"].session_id, None);
}

/// The card's base is the role's current identity: another recast
/// moves it, and the card goes stale.
#[test]
fn a_recast_card_goes_stale_when_the_role_moves() {
    let mut state = replay(&base_events());
    let action = CardAction::Recast {
        studio: "acme-shop".into(),
        person: "director".into(),
        actor: cmd("d"),
    };
    let base = CardBase::RoleIdentity {
        actor: actor("dir"),
    };
    assert_eq!(stale(&action, &base, Some(&state)), None);
    apply(
        &mut state,
        &ev(10, "user", recast_event("director", cmd("e"))),
    );
    assert!(stale(&action, &base, Some(&state)).is_some());
}

/// A proposed recast card's base must match its action at append.
#[test]
fn a_recast_card_proposal_needs_the_identity_base() {
    let action = CardAction::Recast {
        studio: "acme-shop".into(),
        person: "director".into(),
        actor: cmd("d"),
    };
    let good = NewHollywoodEvent::new(
        by("hollywood"),
        HollywoodKind::CardProposed {
            action: action.clone(),
            why: "runner keeps failing".into(),
            base: CardBase::RoleIdentity {
                actor: actor("dir"),
            },
        },
    );
    assert_eq!(
        crate::hollywood::validate(&HollywoodState::default(), &good),
        Ok(())
    );
    let wrong = NewHollywoodEvent::new(
        by("hollywood"),
        HollywoodKind::CardProposed {
            action,
            why: "runner keeps failing".into(),
            base: CardBase::TaskStatus {
                was: crate::state::TaskStatus::Running,
            },
        },
    );
    assert!(crate::hollywood::validate(&HollywoodState::default(), &wrong).is_err());
}
