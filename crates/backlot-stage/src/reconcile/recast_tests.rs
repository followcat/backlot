//! A recast on the tick: a live role is closed on purpose and started
//! fresh under the new identity; a role never started simply starts
//! with it; the role key and terminal name never move.

use std::collections::BTreeMap;

use backlot_model::event::*;
use backlot_model::state::*;
use backlot_model::testkit::*;

use super::*;

const STUDIO: &str = "acme-shop";
const NOW: u64 = 0;

/// A fixed role's new identity.
fn newer(tag: &str) -> ActorIdentity {
    ActorIdentity {
        runner: "fake".into(),
        runner_version: None,
        model: format!("new-{tag}"),
        effort: Some("high".into()),
        permission_profile: "standard".into(),
    }
}

fn started(seq: u64, role: &str) -> Event {
    ev(
        seq,
        "stage",
        EventKind::RoleStarted {
            role: role.into(),
            terminal: Some(format!("bl-{STUDIO}-{role}")),
            actor: Some(newer("old")),
            session_id: Some("s-old".into()),
            reason: None,
            prompt_hash: None,
        },
    )
}

fn recast(seq: u64, role: &str) -> Event {
    ev(
        seq,
        "user",
        EventKind::RoleRecast {
            role: role.into(),
            actor: newer("d"),
            reason: Some("quota".into()),
            card: None,
        },
    )
}

fn live(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
    pairs
        .iter()
        .map(|(n, a)| (n.to_string(), a.to_string()))
        .collect()
}

/// A live director with a recast: one deliberate stop with reason
/// `recast`, then the fresh start under the new identity — no resume.
#[test]
fn a_live_role_is_closed_and_started_fresh() {
    let mut events = vec![ev(1, "user", studio_kind()), started(2, "director")];
    events.push(recast(3, "director"));
    let state = replay(&events);
    let actions = reconcile(
        STUDIO,
        &state,
        &live(&[("bl-acme-shop-director", "t7")]),
        NOW,
    );
    let stop = actions
        .iter()
        .find_map(|a| match a {
            Action::Stop {
                role,
                terminal,
                close,
                reason,
                ..
            } => Some((role.as_str(), terminal.as_str(), *close, reason.as_str())),
            _ => None,
        })
        .expect("a recast closes the live terminal");
    assert_eq!(stop, ("director", "bl-acme-shop-director", true, "recast"));
    assert!(
        !actions
            .iter()
            .any(|a| matches!(a, Action::Start(s) if s.role == "director")),
        "the fresh start follows the close, not the same tick: {actions:?}"
    );

    // The stage records the close; the next tick starts fresh.
    events.push(ev(
        4,
        "stage",
        EventKind::RoleClosed {
            role: "director".into(),
            reason: Some("recast".into()),
        },
    ));
    let state = replay(&events);
    let actions = reconcile(STUDIO, &state, &live(&[]), NOW);
    let start = actions
        .iter()
        .find_map(|a| match a {
            Action::Start(s) if s.role == "director" => Some(s),
            _ => None,
        })
        .expect("the recast director starts");
    assert_eq!(start.actor, newer("d"));
    assert!(start.resume.is_none(), "a recast never resumes: {start:?}");
    assert_eq!(start.terminal, "bl-acme-shop-director");
}

/// A fixed role never started starts with the new identity.
#[test]
fn a_never_started_role_starts_with_the_new_identity() {
    let events = vec![ev(1, "user", studio_kind()), recast(2, "director")];
    let state = replay(&events);
    let actions = reconcile(STUDIO, &state, &live(&[]), NOW);
    let start = actions
        .iter()
        .find_map(|a| match a {
            Action::Start(s) if s.role == "director" => Some(s),
            _ => None,
        })
        .expect("the director starts");
    assert_eq!(start.actor, newer("d"));
    assert!(start.resume.is_none());
}

/// A lost role with a recast starts fresh instead of resuming the old
/// runner's session.
#[test]
fn a_lost_recast_role_never_resumes_the_old_session() {
    let events = vec![
        ev(1, "user", studio_kind()),
        started(2, "director"),
        ev(
            3,
            "stage",
            EventKind::RoleLost {
                role: "director".into(),
                reason: Some("terminal gone".into()),
            },
        ),
        recast(4, "director"),
    ];
    let state = replay(&events);
    let actions = reconcile(STUDIO, &state, &live(&[]), NOW);
    let start = actions
        .iter()
        .find_map(|a| match a {
            Action::Start(s) if s.role == "director" => Some(s),
            _ => None,
        })
        .expect("the recast director starts");
    assert_eq!(start.actor, newer("d"));
    assert!(start.resume.is_none(), "{start:?}");
}

/// Messages queued for the role stay queued, and the new terminal is
/// the same `bl-<studio>-<role>` the delivery pass sends to.
#[test]
fn a_queued_message_keeps_targeting_the_roles_terminal() {
    let mut events = vec![ev(1, "user", studio_kind()), started(2, "director")];
    events.push(ev(
        3,
        "producer",
        EventKind::MessageQueued {
            to: "director".into(),
            file: Some("messages/3-plan.md".into()),
            purpose: None,
            source_seq: None,
        },
    ));
    events.push(recast(4, "director"));
    let state = replay(&events);
    assert_eq!(
        state.messages.last().map(|m| m.to.as_str()),
        Some("director")
    );
    assert!(!state.messages.last().unwrap().delivered);

    let actions = reconcile(
        STUDIO,
        &state,
        &live(&[("bl-acme-shop-director", "t7")]),
        NOW,
    );
    // The close frees the name; the next tick's start claims exactly
    // the name the delivery pass looks up for `director`.
    assert!(
        actions
            .iter()
            .any(|a| matches!(a, Action::Stop { role, .. } if role == "director"))
    );
    let events = {
        let mut e = events.clone();
        e.push(ev(
            5,
            "stage",
            EventKind::RoleClosed {
                role: "director".into(),
                reason: Some("recast".into()),
            },
        ));
        e
    };
    let state = replay(&events);
    let actions = reconcile(STUDIO, &state, &live(&[]), NOW);
    let start = actions
        .iter()
        .find_map(|a| match a {
            Action::Start(s) if s.role == "director" => Some(s),
            _ => None,
        })
        .expect("the recast director starts");
    assert_eq!(
        start.terminal,
        terminal_name(STUDIO, "director").unwrap(),
        "the queued message addresses the role, and this is its terminal"
    );
}
