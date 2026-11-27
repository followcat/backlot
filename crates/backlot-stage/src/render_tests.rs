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
                effort: Some("high".into()),
                permission_profile: "standard".into(),
            }),
            session_id: Some(format!("s-{role}")),
            reason: None,
            prompt_hash: None,
        },
    )
}

/// A producer stall or `cannot resume` is the first line under the
/// studio header — a glance at `backlot status` says why nothing
/// moves, before repo, roles or attention.
#[test]
fn a_producer_alert_leads_the_status_block() {
    let mut events = base_events();
    events.push(ev(
        4,
        "stage",
        EventKind::AttentionRaised {
            role: "producer".into(),
            reason: "stalled".into(),
            detail: "screen unchanged for 600s".into(),
            needs_user: false,
            account: None,
            reset_at_ms: None,
        },
    ));
    let out = status(
        "acme-shop",
        &replay(&events),
        &Default::default(),
        10_000,
        &Default::default(),
    );
    let alert = out
        .lines()
        .position(|l| l.contains("producer alert: stalled"))
        .unwrap_or_else(|| panic!("no producer alert:\n{out}"));
    assert!(
        out.lines()
            .nth(alert)
            .unwrap()
            .contains("screen unchanged for 600s"),
        "{out}"
    );
    let repo = out.lines().position(|l| l.contains("repo ")).unwrap();
    assert!(alert < repo, "the alert is not at the top:\n{out}");

    // The same for an unresumable producer.
    let mut events = base_events();
    events.push(ev(
        4,
        "stage",
        EventKind::AttentionRaised {
            role: "producer".into(),
            reason: "cannot resume".into(),
            detail: "no session id to resume".into(),
            needs_user: false,
            account: None,
            reset_at_ms: None,
        },
    ));
    let out = status(
        "acme-shop",
        &replay(&events),
        &Default::default(),
        10_000,
        &Default::default(),
    );
    assert!(
        out.contains("producer alert: cannot resume"),
        "missing alert:\n{out}"
    );
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
    let out = status(
        "acme-shop",
        &replay(&events),
        &Default::default(),
        10_000,
        &Default::default(),
    );
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

/// `backlot status` shows the studio-wide actor count `actors
/// <live>/<cap>` under the studio header; fixed roles never count.
#[test]
fn the_status_line_shows_the_actor_count() {
    let mut cast_kind = cast("t1", "actor-a", "a");
    let EventKind::TaskCast { actor, .. } = &mut cast_kind else {
        unreachable!()
    };
    actor.runner = "fake".into();
    let mut kind = limited_kind(10);
    let EventKind::StudioCreated { max_actors, .. } = &mut kind else {
        unreachable!()
    };
    *max_actors = 4;
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
        ev(4, "director", task_created("t1", "feature", 1)),
        ev(5, "director", cast_kind),
        started(6, "producer"),
        started(7, "director"),
        started(8, "actor-t1-c1"),
    ];
    let out = status(
        "acme-shop",
        &replay(&events),
        &Default::default(),
        10_000,
        &Default::default(),
    );
    let line = out
        .lines()
        .find(|l| l.trim_start().starts_with("actors "))
        .unwrap_or_else(|| panic!("no actors line:\n{out}"));
    assert!(line.trim() == "actors 1/4", "{line:?}");
}

#[test]
fn status_shows_account_badges_and_accounts_footer() {
    use backlot_model::summary::{AccountRoleSummary, AccountSummary, QuotaWindow};

    let t0 = {
        let ts = ((86_400_000u64 * 20_000) / 1000) as libc::time_t;
        let mut local: libc::tm = unsafe { std::mem::zeroed() };
        unsafe { libc::localtime_r(&ts, &mut local) };
        local.tm_hour = 9;
        local.tm_min = 10;
        local.tm_sec = 0;
        local.tm_isdst = -1;
        (unsafe { libc::mktime(&mut local) } as u64) * 1000
    };
    let reset = t0 + 5 * 3_600_000;

    let mut events = base_events();
    events.push(started(4, "producer"));
    events.push(started(5, "director"));
    let state = replay(&events);

    let live = crate::live::LiveView {
        running: true,
        accounts: vec![
            AccountSummary {
                id: "claude:default".into(),
                runner: "claude".into(),
                source: "default".into(),
                active: 3,
                working: 1,
                max_active: None,
                room: None,
                queued: 0,
                stalled: true,
                last_stall_ms: Some(t0),
                reset_at_ms: Some(reset),
                window: Some(QuotaWindow::Short),
                roles: vec![
                    AccountRoleSummary {
                        studio: "hollywood".into(),
                        role: "hollywood".into(),
                        model: "opus-5-5".into(),
                        active: true,
                        working: false,
                        held: false,
                        stalled: false,
                    },
                    AccountRoleSummary {
                        studio: "acme-shop".into(),
                        role: "producer".into(),
                        model: "opus-5-5".into(),
                        active: true,
                        working: false,
                        held: false,
                        stalled: true,
                    },
                    AccountRoleSummary {
                        studio: "acme-web".into(),
                        role: "actor-t1-c1".into(),
                        model: "sonnet-5".into(),
                        active: true,
                        working: true,
                        held: false,
                        stalled: false,
                    },
                ],
            },
            AccountSummary {
                id: "claude:work".into(),
                runner: "claude".into(),
                source: "config-dir (~/.claude-work)".into(),
                active: 1,
                working: 1,
                max_active: None,
                room: None,
                queued: 0,
                stalled: false,
                last_stall_ms: None,
                reset_at_ms: None,
                window: None,
                roles: vec![AccountRoleSummary {
                    studio: "acme-shop".into(),
                    role: "director".into(),
                    model: "sonnet-5".into(),
                    active: true,
                    working: true,
                    held: false,
                    stalled: false,
                }],
            },
        ],
        ..Default::default()
    };

    let out = status("acme-shop", &state, &Default::default(), t0, &live);
    let prod_line = out
        .lines()
        .find(|l| l.trim_start().starts_with("producer"))
        .expect("producer line");
    assert!(prod_line.contains("[claude:default]"), "{prod_line}");
    let dir_line = out
        .lines()
        .find(|l| l.trim_start().starts_with("director"))
        .expect("director line");
    assert!(dir_line.contains("[claude:work]"), "{dir_line}");
    assert!(out.contains("accounts:"), "{out}");
    assert!(
        out.contains("STALLED") && out.contains("14:10") && out.contains("1 here + 2 elsewhere"),
        "{out}"
    );
}
