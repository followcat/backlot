//! A `role.restart` in reconcile (L2): a live role is stopped on
//! purpose and started again into its recorded session; a lost one is
//! started again without waiting on the loss guard; `fresh` asks for a
//! new session.

use std::collections::BTreeMap;

use backlot_model::event::*;
use backlot_model::state::*;
use backlot_model::testkit::*;

use super::*;

const STUDIO: &str = "acme-shop";
/// The clock these tests hand the resume guard.
const NOW: u64 = 0;

fn log(events: Vec<Event>) -> StudioState {
    replay(&events)
}

fn studio() -> Vec<Event> {
    vec![
        ev(1, "user", studio_kind()),
        ev(
            2,
            "producer",
            EventKind::ReqVersion {
                version: 1,
                file: "req/v1.md".into(),
                summary: "s".into(),
            },
        ),
    ]
}

fn started(seq: u64, role: &str, term: &str, sess: &str) -> Event {
    ev(
        seq,
        "stage",
        EventKind::RoleStarted {
            role: role.into(),
            terminal: Some(term.into()),
            actor: Some(actor("a")),
            session_id: Some(sess.into()),
            reason: None,
            prompt_hash: None,
        },
    )
}

fn closed(seq: u64, role: &str, reason: &str) -> Event {
    ev(
        seq,
        "stage",
        EventKind::RoleClosed {
            role: role.into(),
            reason: Some(reason.into()),
        },
    )
}

fn restart(seq: u64, role: &str, fresh: bool) -> Event {
    ev(
        seq,
        "user",
        EventKind::RoleRestart {
            role: role.into(),
            fresh,
            reason: None,
        },
    )
}

fn live(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
    pairs
        .iter()
        .map(|(n, a)| (n.to_string(), a.to_string()))
        .collect()
}

fn stops(actions: &[Action]) -> Vec<(&str, &str, bool, &str)> {
    actions
        .iter()
        .filter_map(|a| match a {
            Action::Stop {
                role,
                terminal,
                close,
                reason,
                ..
            } => Some((role.as_str(), terminal.as_str(), *close, reason.as_str())),
            _ => None,
        })
        .collect()
}

fn starts(actions: &[Action]) -> Vec<&Start> {
    actions
        .iter()
        .filter_map(|a| match a {
            Action::Start(s) => Some(s),
            _ => None,
        })
        .collect()
}

/// A live producer with a restart request is stopped on purpose —
/// `role.closed`, reason `restarted`, never a loss — and the fresh
/// start lands next tick once the role reads closed.
#[test]
fn a_live_restart_stops_then_starts() {
    let mut events = studio();
    events.push(started(3, "producer", "bl-acme-shop-producer", "sess-p"));
    events.push(restart(4, "producer", false));
    let state = log(events.clone());
    let view = live(&[("bl-acme-shop-producer", "t1")]);
    let actions = reconcile(STUDIO, &state, &view, NOW);
    assert_eq!(
        stops(&actions),
        [("producer", "bl-acme-shop-producer", true, RESTARTED)],
        "{actions:?}"
    );
    assert!(
        !actions.iter().any(|a| matches!(a, Action::Lost { .. })),
        "a restart's stop is never a loss: {actions:?}"
    );

    // The close lands; the start resumes the recorded session.
    events.push(closed(5, "producer", RESTARTED));
    let state = log(events);
    let actions = reconcile(STUDIO, &state, &live(&[]), NOW);
    let start = starts(&actions)
        .into_iter()
        .find(|s| s.role == "producer")
        .expect("the restart's start");
    assert_eq!(
        start
            .resume
            .as_ref()
            .map(|r| (r.session.as_str(), r.reason)),
        Some(("sess-p", RESTARTED))
    );
    assert_eq!(start.terminal, "bl-acme-shop-producer");
}

/// `--fresh` starts a new session: the resume is not set.
#[test]
fn a_fresh_restart_starts_without_a_resume() {
    let mut events = studio();
    events.push(started(3, "producer", "bl-acme-shop-producer", "sess-p"));
    events.push(restart(4, "producer", true));
    events.push(closed(5, "producer", RESTARTED));
    let actions = reconcile(STUDIO, &log(events), &live(&[]), NOW);
    let start = starts(&actions)
        .into_iter()
        .find(|s| s.role == "producer")
        .expect("the restart's start");
    assert!(start.resume.is_none(), "{start:?}");
}

/// A lost role a restart names comes back at once — the loss guard
/// does not hold it up, and it resumes its session.
#[test]
fn a_lost_restart_starts_without_the_guard() {
    let mut events = studio();
    events.push(started(3, "producer", "bl-acme-shop-producer", "sess-p"));
    // Three losses inside the window would trip the guard's
    // `cannot resume`; a restart ignores it.
    for seq in 4..=6 {
        events.push(ev(
            seq,
            "stage",
            EventKind::RoleLost {
                role: "producer".into(),
                reason: None,
            },
        ));
    }
    events.push(restart(7, "producer", false));
    let actions = reconcile(STUDIO, &log(events), &live(&[]), NOW);
    let start = starts(&actions)
        .into_iter()
        .find(|s| s.role == "producer")
        .expect("the restart's start");
    assert_eq!(start.resume.as_ref().map(|r| r.reason), Some(RESTARTED));
    assert!(
        !actions.iter().any(|a| matches!(
            a,
            Action::Attention { reason, .. } if reason == CANNOT_RESUME
        )),
        "a restart is the user's deliberate way on: {actions:?}"
    );
}
