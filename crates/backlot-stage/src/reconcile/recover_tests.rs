//! Recovering lost roles (S6b): a wanted lost role resumes into
//! its recorded session — `role.resumed` reason `recovered` — and
//! without one, or once the loss guard trips, a `cannot resume`
//! attention says why. The loss times live in `RoleState`, so the
//! count survives a stage restart.

use std::collections::BTreeMap;

use backlot_model::event::*;
use backlot_model::state::*;
use backlot_model::testkit::*;

use super::*;

const STUDIO: &str = "acme-shop";

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

fn started(seq: u64, role: &str, sess: Option<&str>) -> Event {
    ev(
        seq,
        "stage",
        EventKind::RoleStarted {
            role: role.into(),
            terminal: Some(format!("t{seq}")),
            actor: Some(actor("a")),
            session_id: sess.map(String::from),
            reason: None,
        },
    )
}

fn lost(seq: u64, role: &str) -> Event {
    ev(
        seq,
        "stage",
        EventKind::RoleLost {
            role: role.into(),
            reason: None,
        },
    )
}

/// A `role.lost` at an exact time — the guard reads `ts_ms`, so a
/// loss's place in the window is a test input, not its seq.
fn lost_at(seq: u64, role: &str, ts_ms: u64) -> Event {
    let mut e = lost(seq, role);
    e.ts_ms = ts_ms;
    e
}

fn attention(seq: u64, role: &str, reason: &str) -> Event {
    ev(
        seq,
        "stage",
        EventKind::AttentionRaised {
            role: role.into(),
            reason: reason.into(),
            detail: "d".into(),
            needs_user: false,
        },
    )
}

fn live(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
    pairs
        .iter()
        .map(|(n, a)| (n.to_string(), a.to_string()))
        .collect()
}

fn start_for<'a>(actions: &'a [Action], role: &str) -> Option<&'a Start> {
    actions.iter().find_map(|a| match a {
        Action::Start(s) if s.role == role => Some(s),
        _ => None,
    })
}

/// A wanted lost role with a session comes back into it — the
/// start is a resume and its reason is `recovered`, not `reopened`.
#[test]
fn a_lost_wanted_role_resumes_its_session_as_recovered() {
    let mut events = studio();
    events.push(started(3, "producer", Some("sess-1")));
    events.push(lost(4, "producer"));
    let actions = reconcile(STUDIO, &log(events), &live(&[]), 10_000);
    let start = start_for(&actions, "producer").expect("a resume start");
    assert_eq!(
        start
            .resume
            .as_ref()
            .map(|r| (r.session.as_str(), r.reason)),
        Some(("sess-1", RECOVERED)),
        "{start:?}"
    );
    assert_eq!(start.terminal, "bl-acme-shop-producer");
    assert_eq!(start.kind, RoleKind::Producer);
}

/// A lost role the ledger no longer wants gets nothing — not a
/// resume, not an attention.
#[test]
fn a_lost_role_not_wanted_needs_nothing() {
    let mut events = studio();
    // The writer wanted between `plan` and `dispatch`, released after.
    events.push(ev(
        3,
        "producer",
        EventKind::MessageQueued {
            to: "writer".into(),
            file: None,
            purpose: Some("plan".into()),
            source_seq: None,
        },
    ));
    events.push(started(4, "writer", Some("sess-w")));
    events.push(ev(
        5,
        "producer",
        EventKind::MessageQueued {
            to: "director".into(),
            file: Some("messages/d.md".into()),
            purpose: Some("dispatch".into()),
            source_seq: None,
        },
    ));
    events.push(lost(6, "writer"));
    let actions = reconcile(STUDIO, &log(events), &live(&[]), 10_000);
    assert!(
        !actions.iter().any(|a| matches!(a,
            Action::Start(Start { role, .. })
            | Action::Attention { role, .. }
            | Action::Lost { role, .. } if role == "writer")),
        "an unwanted lost role needs nothing: {actions:?}"
    );
}

/// No recorded session: the wanted lost role cannot come back —
/// one `cannot resume` attention says why, with no screen file.
#[test]
fn a_lost_role_without_a_session_raises_cannot_resume() {
    let mut events = studio();
    events.push(started(3, "producer", None));
    events.push(lost(4, "producer"));
    let actions = reconcile(STUDIO, &log(events), &live(&[]), 10_000);
    assert!(
        actions.iter().any(|a| matches!(a,
            Action::Attention { role, reason, detail }
            if role == "producer"
                && reason == CANNOT_RESUME
                && detail == "no session id to resume")),
        "{actions:?}"
    );
    assert!(start_for(&actions, "producer").is_none(), "{actions:?}");
}

/// Three losses inside ten minutes stop the resume: the third
/// `role.lost` raises `cannot resume` naming the count.
#[test]
fn the_third_loss_in_ten_minutes_raises_cannot_resume() {
    let mut events = studio();
    events.push(started(3, "producer", Some("sess-1")));
    events.push(lost_at(4, "producer", 100_000));
    events.push(started(5, "producer", Some("sess-1")));
    events.push(lost_at(6, "producer", 200_000));
    events.push(started(7, "producer", Some("sess-1")));
    events.push(lost_at(8, "producer", 300_000));
    let actions = reconcile(STUDIO, &log(events), &live(&[]), 310_000);
    assert!(
        actions.iter().any(|a| matches!(a,
            Action::Attention { role, reason, detail }
            if role == "producer"
                && reason == CANNOT_RESUME
                && detail == "lost 3 times in 10 minutes")),
        "{actions:?}"
    );
    assert!(start_for(&actions, "producer").is_none(), "{actions:?}");
}

/// A loss older than the window does not count: two recent losses
/// and an old one still resume.
#[test]
fn a_loss_outside_the_window_does_not_count() {
    let mut events = studio();
    events.push(started(3, "producer", Some("sess-1")));
    events.push(lost_at(4, "producer", 1_000));
    events.push(started(5, "producer", Some("sess-1")));
    events.push(lost_at(6, "producer", 650_000));
    events.push(started(7, "producer", Some("sess-1")));
    events.push(lost_at(8, "producer", 660_000));
    // Only the last two losses are inside ten minutes of now.
    let actions = reconcile(STUDIO, &log(events), &live(&[]), 700_000);
    let start = start_for(&actions, "producer").expect("still resumable");
    assert_eq!(
        start
            .resume
            .as_ref()
            .map(|r| (r.session.as_str(), r.reason)),
        Some(("sess-1", RECOVERED)),
        "{start:?}"
    );
}

/// `role.lost` times replay from the ledger into `RoleState`, so a
/// stage restart keeps the guard's count.
#[test]
fn the_losses_replay_from_the_ledger() {
    let mut events = studio();
    events.push(started(3, "producer", Some("sess-1")));
    events.push(lost_at(4, "producer", 100_000));
    events.push(started(5, "producer", Some("sess-1")));
    events.push(lost_at(6, "producer", 200_000));
    let state = log(events);
    assert_eq!(state.roles["producer"].losses, [100_000, 200_000]);
}

/// An open `cannot resume` clears when the role is live again —
/// `role live` — and when it is no longer wanted —
/// `no longer wanted`.
#[test]
fn an_open_cannot_resume_clears() {
    // Live again: a recovered producer with its attention still open.
    let mut events = studio();
    events.push(started(3, "producer", Some("sess-1")));
    events.push(lost(4, "producer"));
    events.push(attention(5, "producer", CANNOT_RESUME));
    events.push(ev(
        6,
        "stage",
        EventKind::RoleResumed {
            role: "producer".into(),
            terminal: Some("t6".into()),
            actor: Some(actor("a")),
            session_id: Some("sess-1".into()),
            reason: Some(RECOVERED.into()),
        },
    ));
    let view = live(&[("bl-acme-shop-producer", "t6")]);
    let actions = reconcile(STUDIO, &log(events), &view, 10_000);
    assert!(
        actions.iter().any(|a| matches!(a,
            Action::ClearAttention { target: 5, reason } if reason == "role live")),
        "{actions:?}"
    );

    // No longer wanted: the writer's attention outlives its window.
    let mut events = studio();
    events.push(started(3, "writer", Some("sess-w")));
    events.push(attention(4, "writer", CANNOT_RESUME));
    events.push(lost(5, "writer"));
    let actions = reconcile(STUDIO, &log(events), &live(&[]), 10_000);
    assert!(
        actions.iter().any(|a| matches!(a,
            Action::ClearAttention { target: 4, reason } if reason == "no longer wanted")),
        "{actions:?}"
    );
}

/// `role.lost` clears the role's open attention in the same pass —
/// the prompt or stall it named is gone with the terminal. Another
/// role's attention is untouched.
#[test]
fn role_lost_clears_the_roles_open_attention() {
    let mut events = studio();
    events.push(started(3, "producer", Some("sess-1")));
    events.push(started(4, "director", Some("sess-2")));
    events.push(attention(5, "producer", PERMISSION_PROMPT));
    events.push(attention(6, "producer", STALLED));
    events.push(attention(7, "director", PERMISSION_PROMPT));
    // The producer's terminal is gone; the director's is still up.
    let view = live(&[("bl-acme-shop-director", "t4")]);
    let actions = reconcile(STUDIO, &log(events), &view, 10_000);
    let mut clears: Vec<(u64, &str)> = actions
        .iter()
        .filter_map(|a| match a {
            Action::ClearAttention { target, reason } => Some((*target, reason.as_str())),
            _ => None,
        })
        .collect();
    clears.sort();
    assert_eq!(
        clears,
        [(5, "role lost"), (6, "role lost")],
        "producer's open attention clears with its loss, the director's stays open: {actions:?}"
    );
}

/// A held person's reopen stays `reopened` — recovery is for lost
/// roles, not deliberate holds.
#[test]
fn a_held_reopen_stays_reopened() {
    let mut events = studio();
    events.push(ev(3, "director", task_created("t1", "feature", 1)));
    events.push(ev(4, "director", cast("t1", "actor-a", "a")));
    events.push(started(5, "actor-t1-c1", Some("sess-1")));
    events.push(ev(
        6,
        "stage",
        EventKind::RoleClosed {
            role: "actor-t1-c1".into(),
            reason: Some("no longer desired".into()),
        },
    ));
    events.push(ev(
        7,
        "user",
        EventKind::RoleReopen {
            role: "actor-t1-c1".into(),
        },
    ));
    let actions = reconcile(STUDIO, &log(events), &live(&[]), 10_000);
    let start = start_for(&actions, "actor-t1-c1").expect("held actor desired");
    assert_eq!(
        start
            .resume
            .as_ref()
            .map(|r| (r.session.as_str(), r.reason)),
        Some(("sess-1", REOPENED)),
        "{start:?}"
    );
}

/// A lost role whose terminal is already up needs no `terminal.run`:
/// the pass records `role.resumed` with that terminal's address and
/// the recorded session, and no `Start` runs.
#[test]
fn a_lost_role_with_its_terminal_up_records_resumed() {
    let mut events = studio();
    events.push(started(3, "producer", Some("sess-1")));
    events.push(lost(4, "producer"));
    // The terminal came back without the stage — another core took
    // the role's name before this pass ran.
    let view = live(&[("bl-acme-shop-producer", "t9")]);
    let actions = reconcile(STUDIO, &log(events), &view, 10_000);
    assert!(
        actions.iter().any(|a| matches!(a,
            Action::Resumed { role, address, session }
            if role == "producer" && address == "t9" && session == "sess-1")),
        "{actions:?}"
    );
    assert!(start_for(&actions, "producer").is_none(), "{actions:?}");
}

/// The live-terminal record is checked before the guard: even a
/// tripped loss guard cannot leave a live terminal `cannot resume`.
#[test]
fn a_live_terminal_recovers_past_the_guard() {
    let mut events = studio();
    events.push(started(3, "producer", Some("sess-1")));
    events.push(lost_at(4, "producer", 100_000));
    events.push(started(5, "producer", Some("sess-1")));
    events.push(lost_at(6, "producer", 200_000));
    events.push(started(7, "producer", Some("sess-1")));
    events.push(lost_at(8, "producer", 300_000));
    let view = live(&[("bl-acme-shop-producer", "t9")]);
    let actions = reconcile(STUDIO, &log(events), &view, 310_000);
    assert!(
        actions.iter().any(|a| matches!(a,
            Action::Resumed { role, address, session }
            if role == "producer" && address == "t9" && session == "sess-1")),
        "{actions:?}"
    );
    assert!(
        !actions.iter().any(|a| matches!(a,
            Action::Attention { role, reason, .. }
            if role == "producer" && reason == CANNOT_RESUME)),
        "{actions:?}"
    );
    assert!(start_for(&actions, "producer").is_none(), "{actions:?}");
}

/// No recorded session stays `cannot resume` even when the terminal
/// name is live — the record would carry no session to rejoin.
#[test]
fn a_terminal_up_without_a_session_stays_cannot_resume() {
    let mut events = studio();
    events.push(started(3, "producer", None));
    events.push(lost(4, "producer"));
    let view = live(&[("bl-acme-shop-producer", "t9")]);
    let actions = reconcile(STUDIO, &log(events), &view, 10_000);
    assert!(
        actions.iter().any(|a| matches!(a,
            Action::Attention { role, reason, detail }
            if role == "producer"
                && reason == CANNOT_RESUME
                && detail == "no session id to resume")),
        "{actions:?}"
    );
    assert!(start_for(&actions, "producer").is_none(), "{actions:?}");
}
