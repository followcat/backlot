use crate::event::{EventKind, PermitKind, PermitScope};
use crate::testkit::{base_state, cast, new_ev, state_with};
use crate::validate::validate;

fn grant(id: &str, scope: PermitScope, role: Option<&str>, rule: &str) -> EventKind {
    EventKind::PermitGranted {
        id: id.into(),
        scope,
        role: role.map(str::to_string),
        permit_kind: PermitKind::Shell,
        rule: rule.into(),
        outward: false,
        from: 7,
        runner: "cmd".into(),
    }
}

#[test]
fn granted_reduces_and_revoked_removes() {
    let state = state_with(
        &crate::testkit::base_events(),
        vec![
            (
                "user",
                grant("p1", PermitScope::Role, Some("actor-a"), "du -sh"),
            ),
            ("user", grant("p2", PermitScope::Studio, None, "git log")),
        ],
    );
    assert_eq!(state.permits.len(), 2);
    assert!(state.permits["p1"].reaches("actor-a"));
    assert!(!state.permits["p1"].reaches("actor-b"));
    assert!(state.permits["p2"].reaches("actor-b"));
    let state = state_with(
        &crate::testkit::base_events(),
        vec![
            (
                "user",
                grant("p1", PermitScope::Role, Some("actor-a"), "du -sh"),
            ),
            ("user", grant("p2", PermitScope::Studio, None, "git log")),
            (
                "user",
                EventKind::PermitRevoked {
                    id: "p1".into(),
                    reason: "no longer wanted".into(),
                },
            ),
        ],
    );
    assert!(!state.permits.contains_key("p1"));
    assert!(state.permits.contains_key("p2"));
}

#[test]
fn grants_validate_scope_and_caller() {
    let state = base_state();
    let mut ev = new_ev(
        "user",
        grant("p1", PermitScope::Studio, Some("actor-a"), "ls"),
    );
    assert!(validate(&state, &ev).is_err());
    ev = new_ev("director", grant("p1", PermitScope::Studio, None, "ls"));
    assert!(validate(&state, &ev).is_err());
    ev = new_ev("user", grant("p1", PermitScope::Studio, None, "ls"));
    assert!(validate(&state, &ev).is_ok());
}

#[test]
fn outward_never_takes_studio_scope() {
    let state = base_state();
    let ev = new_ev(
        "user",
        EventKind::PermitGranted {
            id: "p1".into(),
            scope: PermitScope::Studio,
            role: None,
            permit_kind: PermitKind::Shell,
            rule: "curl".into(),
            outward: true,
            from: 7,
            runner: "cmd".into(),
        },
    );
    assert!(validate(&state, &ev).is_err());
}

#[test]
fn actor_role_key_is_the_candidate() {
    let state = state_with(
        &crate::testkit::base_events(),
        vec![("director", cast("t1", "actor-a", "a"))],
    );
    assert_eq!(crate::permit::role_key(&state, "actor-t1-c1"), "actor-a");
    assert_eq!(crate::permit::role_key(&state, "director"), "director");
}
