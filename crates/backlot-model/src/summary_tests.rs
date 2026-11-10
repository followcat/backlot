//! Summary document tests: shape, caps, closed roles staying listed
//! and the finished-studio drop-off.

use std::collections::BTreeMap;

use crate::event::{Event, EventKind, Priority, Verdict};
use crate::state::{StudioState, replay};
use crate::summary::*;
use crate::testkit::*;

fn started(role: &str, terminal: &str, tag: &str) -> EventKind {
    EventKind::RoleStarted {
        role: role.into(),
        terminal: Some(terminal.into()),
        actor: Some(actor(tag)),
        session_id: Some(format!("sess-{role}")),
        reason: None,
    }
}

fn closed(role: &str) -> EventKind {
    EventKind::RoleClosed {
        role: role.into(),
        reason: Some("no longer desired".into()),
    }
}

fn live_view() -> BTreeMap<String, String> {
    BTreeMap::from([
        ("t1".into(), "writing the plan".into()),
        ("t2".into(), "reading the repo".into()),
    ])
}

/// The document carries the schema and per-studio fields.
#[test]
fn summary_shape_and_live_doing() {
    let state = state_with(
        &base_events(),
        vec![
            ("stage", started("producer", "t1", "prod")),
            ("stage", started("director", "t2", "dir")),
        ],
    );
    let s = summary(
        &[("acme-shop".into(), state)],
        &live_view(),
        &BTreeMap::new(),
        &BTreeMap::new(),
        42,
    );
    assert_eq!(s.schema, SUMMARY_SCHEMA);
    assert_eq!(s.updated_ms, 42);
    assert_eq!(s.studios.len(), 1);
    let st = &s.studios[0];
    assert_eq!(st.id, "acme-shop");
    assert_eq!(st.title, "acme-shop");
    assert_eq!(st.state, "live");
    assert_eq!(st.requirement_version, 1);
    assert_eq!(st.open_reviews, 0);
    assert_eq!(st.waiting, 0);
    assert_eq!(st.roles.len(), 2);
    let p = st.roles.iter().find(|r| r.role == "producer").unwrap();
    assert_eq!(p.state, "live");
    assert_eq!(p.terminal.as_deref(), Some("t1"));
    assert_eq!(p.runner.as_deref(), Some("runner-prod"));
    assert_eq!(p.model.as_deref(), Some("model-prod"));
    assert_eq!(p.effort.as_deref(), Some("high"));
    assert_eq!(p.doing.as_deref(), Some("writing the plan"));
}

/// A closed role stays listed, with no `doing`, keeping actor and
/// name — and its session id never reaches the document.
#[test]
fn a_closed_role_stays_listed_without_doing() {
    let state = state_with(
        &base_events(),
        vec![
            ("stage", started("producer", "t1", "prod")),
            ("stage", started("writer", "t9", "wri")),
            ("stage", closed("writer")),
        ],
    );
    let s = summary(
        &[("acme-shop".into(), state)],
        &BTreeMap::new(),
        &BTreeMap::new(),
        &BTreeMap::new(),
        1,
    );
    let w = s.studios[0]
        .roles
        .iter()
        .find(|r| r.role == "writer")
        .unwrap();
    assert_eq!(w.state, "closed");
    assert_eq!(w.doing, None);
    assert_eq!(w.model.as_deref(), Some("model-wri"));
    let json = serde_json::to_string(&s).unwrap();
    assert!(!json.contains("sess-writer"), "{json}");
    assert!(!json.contains("session"), "{json}");
}

/// An actor role names its candidate and falls back to the task as
/// `doing` when the terminal reports nothing.
#[test]
fn an_actor_names_its_candidate_and_task() {
    let mut base = base_events();
    base.push(ev(4, "director", cast("t1", "actor-a", "a")));
    let state = state_with(&base, vec![("stage", started("actor-t1-c1", "t7", "a"))]);
    let s = summary(
        &[("acme-shop".into(), state)],
        &BTreeMap::new(),
        &BTreeMap::new(),
        &BTreeMap::new(),
        1,
    );
    let a = s.studios[0]
        .roles
        .iter()
        .find(|r| r.role == "actor-t1-c1")
        .unwrap();
    assert_eq!(a.name, "actor-a");
    assert_eq!(a.doing.as_deref(), Some("task t1"));
}

/// A `permission prompt` the director may not answer — flagged
/// `needs_user`, or any prompt on a role that is not an actor — is
/// marked as the user's on the role's `attention` line (S9b); an
/// ordinary actor prompt keeps the plain reason.
#[test]
fn a_user_owned_prompt_marks_the_attention_line() {
    let mut base = base_events();
    base.push(ev(4, "director", cast("t1", "actor-a", "a")));
    let flag = |role: &str, needs_user: bool| {
        (
            "stage",
            EventKind::AttentionRaised {
                role: role.into(),
                reason: crate::event::PERMISSION_PROMPT.into(),
                detail: String::new(),
                needs_user,
            },
        )
    };
    let state = state_with(
        &base,
        vec![
            ("stage", started("actor-t1-c1", "t7", "a")),
            ("stage", started("writer", "t8", "w")),
            flag("actor-t1-c1", true),
            flag("writer", false),
        ],
    );
    let s = summary(
        &[("acme-shop".into(), state)],
        &BTreeMap::new(),
        &BTreeMap::new(),
        &BTreeMap::new(),
        1,
    );
    let role = |name: &str| s.studios[0].roles.iter().find(|r| r.role == name).unwrap();
    assert_eq!(
        role("actor-t1-c1").attention.as_deref(),
        Some("permission prompt — needs the user")
    );
    assert_eq!(
        role("writer").attention.as_deref(),
        Some("permission prompt — needs the user")
    );
    // Unflagged on an actor: the plain reason — the director's.
    let state = state_with(
        &base_events()
            .into_iter()
            .chain([ev(4, "director", cast("t1", "actor-a", "a"))])
            .collect::<Vec<_>>(),
        vec![
            ("stage", started("actor-t1-c1", "t7", "a")),
            flag("actor-t1-c1", false),
        ],
    );
    let s = summary(
        &[("acme-shop".into(), state)],
        &BTreeMap::new(),
        &BTreeMap::new(),
        &BTreeMap::new(),
        1,
    );
    let a = s.studios[0]
        .roles
        .iter()
        .find(|r| r.role == "actor-t1-c1")
        .unwrap();
    assert_eq!(a.attention.as_deref(), Some("permission prompt"));
}

/// An actor whose task `is_behind` the current requirement says so
/// on its `doing` line — the panel draws it verbatim.
#[test]
fn a_behind_actor_marks_its_doing_line() {
    let mut base = base_events();
    base.push(ev(4, "director", cast("t1", "actor-a", "a")));
    let state = state_with(
        &base,
        vec![
            ("stage", started("actor-t1-c1", "t7", "a")),
            (
                "producer",
                EventKind::ReqVersion {
                    version: 2,
                    file: "req/v2.md".into(),
                    summary: "new".into(),
                },
            ),
        ],
    );
    let s = summary(
        &[("acme-shop".into(), state)],
        &BTreeMap::new(),
        &BTreeMap::new(),
        &BTreeMap::new(),
        1,
    );
    let a = s.studios[0]
        .roles
        .iter()
        .find(|r| r.role == "actor-t1-c1")
        .unwrap();
    assert_eq!(a.doing.as_deref(), Some("task t1 · behind v2"));
}

/// Open reviews count delivered tasks; waiting counts open attention.
#[test]
fn counters_follow_the_ledger() {
    let mut base = base_events();
    base.push(ev(4, "director", cast("t1", "actor-a", "a")));
    let state = state_with(
        &base,
        vec![
            ("actor", delivered("t1", 1, "a", 1)),
            (
                "stage",
                EventKind::AttentionRaised {
                    role: "actor-t1-c1".into(),
                    reason: "permission".into(),
                    detail: "wants to write".into(),
                    needs_user: false,
                },
            ),
        ],
    );
    let s = summary(
        &[("acme-shop".into(), state)],
        &BTreeMap::new(),
        &BTreeMap::new(),
        &BTreeMap::new(),
        1,
    );
    assert_eq!(s.studios[0].open_reviews, 1);
    assert_eq!(s.studios[0].waiting, 1);
}

/// The studio's `next` names the first owed step — label and owner —
/// and stays `None` while the ledger owes nothing (S9a). It also
/// keeps out of `same_as` churn: the text holds no clock.
#[test]
fn the_first_owed_step_names_label_and_owner() {
    // base_events leaves t1 created and uncast — the director's step.
    let state = state_with(&base_events(), vec![]);
    let s = summary(
        &[("acme-shop".into(), state)],
        &BTreeMap::new(),
        &BTreeMap::new(),
        &BTreeMap::new(),
        1,
    );
    assert_eq!(s.studios[0].next.as_deref(), Some("cast t1 — director"));

    // Publish both plan documents and the owed step is the
    // producer's dispatch — on its own the only step in this ledger.
    let mut events = vec![
        ev(1, "user", studio_kind()),
        ev(
            2,
            "producer",
            EventKind::ReqVersion {
                version: 1,
                file: "req/v1.md".into(),
                summary: "first".into(),
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
        ev(
            4,
            "writer",
            EventKind::PlanPublished {
                plan_kind: crate::event::PlanKind::Plan,
                file: "plan/4-plan.md".into(),
                requirement_version: 1,
            },
        ),
        ev(
            5,
            "writer",
            EventKind::PlanPublished {
                plan_kind: crate::event::PlanKind::Acceptance,
                file: "plan/5-acc.md".into(),
                requirement_version: 1,
            },
        ),
    ];
    let s = summary(
        &[("acme-shop".into(), state_with(&events, vec![]))],
        &BTreeMap::new(),
        &BTreeMap::new(),
        &BTreeMap::new(),
        1,
    );
    assert_eq!(s.studios[0].next.as_deref(), Some("dispatch — producer"));

    // A finished studio owes nothing: `next` never serializes.
    events.push(ev(
        6,
        "user",
        EventKind::StudioFinished {
            reason: "done".into(),
        },
    ));
    let s = summary(
        &[("acme-shop".into(), state_with(&events, vec![]))],
        &BTreeMap::new(),
        &BTreeMap::new(),
        &BTreeMap::new(),
        1,
    );
    assert_eq!(s.studios[0].next, None);
    let json = serde_json::to_string(&s.studios[0]).unwrap();
    assert!(!json.contains("\"next\""), "{json}");
}

/// `n` casts on t1, each started then closed: `actor-t1-c1..cN` all
/// end up recorded closed people. Appends to `events` in seq order.
fn closed_actors(events: &mut Vec<Event>, tag: &str, n: usize) {
    let mut seq = events.len() as u64;
    for i in 1..=n {
        let role = format!("actor-t1-c{i}");
        seq += 1;
        events.push(ev(
            seq,
            "director",
            cast("t1", &format!("{tag}-cand-{i}"), "a"),
        ));
        seq += 1;
        events.push(ev(seq, "stage", started(&role, "t9", "a")));
        seq += 1;
        events.push(ev(seq, "stage", closed(&role)));
    }
}

/// Fixed roles lead the panel list and every recorded person is in
/// it — forty closed actors do not push the producer out.
#[test]
fn fixed_roles_come_first_and_every_role_is_listed() {
    let mut events = base_events();
    let mut seq = events.len() as u64;
    for kind in [
        started("producer", "t1", "prod"),
        started("director", "t2", "dir"),
    ] {
        seq += 1;
        events.push(ev(seq, "stage", kind));
    }
    closed_actors(&mut events, "many", 40);
    let state = replay(&events);
    let s = summary(
        &[("acme-shop".into(), state)],
        &BTreeMap::new(),
        &BTreeMap::new(),
        &BTreeMap::new(),
        1,
    );
    let roles = &s.studios[0].roles;
    assert_eq!(roles.len(), 42, "{roles:?}");
    assert_eq!(roles[0].role, "producer");
    assert_eq!(roles[1].role, "director");
    assert!(roles[2].role.starts_with("actor-"), "{roles:?}");
    for i in 1..=40 {
        let role = format!("actor-t1-c{i}");
        assert!(roles.iter().any(|r| r.role == role), "missing {role}");
    }
}

/// Over the size target, finished studios drop; a live studio stays
/// — and stays even when it alone is over the target.
#[test]
fn finished_studios_drop_for_size_but_live_stay() {
    let big_finished = |tag: &str| {
        let mut events = base_events();
        closed_actors(&mut events, tag, 80);
        let seq = events.len() as u64 + 1;
        events.push(ev(
            seq,
            "director",
            EventKind::TaskState {
                task: "t1".into(),
                state: crate::event::TaskStateChange::Abandoned,
                reason: "done".into(),
            },
        ));
        replay(&events)
    };
    let mut studios = vec![("live-0".to_string(), base_state())];
    for i in 0..6 {
        studios.push((format!("done-{i}"), big_finished(&format!("d{i}"))));
    }
    let total = studios.len();
    let s = summary(
        &studios,
        &BTreeMap::new(),
        &BTreeMap::new(),
        &BTreeMap::new(),
        1,
    );
    assert!(s.studios.iter().any(|st| st.id == "live-0"), "{s:?}");
    assert!(s.studios.len() < total, "{}", s.studios.len());
    assert!(
        serde_json::to_vec(&s.studios).unwrap().len() <= 60_000,
        "{}",
        serde_json::to_vec(&s.studios).unwrap().len()
    );

    // One live studio far over the target is written anyway.
    let mut events = base_events();
    closed_actors(&mut events, "huge", 350);
    let s = summary(
        &[("big-live".into(), replay(&events))],
        &BTreeMap::new(),
        &BTreeMap::new(),
        &BTreeMap::new(),
        1,
    );
    assert_eq!(s.studios.len(), 1);
    assert_eq!(s.studios[0].id, "big-live");
    assert_eq!(s.studios[0].roles.len(), 350);
}

/// Finished studios fill only the slots live ones leave.
#[test]
fn finished_studios_drop_off_after_live_ones() {
    let mut studios = Vec::new();
    // 2 live studios first.
    for i in 0..2 {
        studios.push((format!("live-{i}"), base_state()));
    }
    // Then many finished ones (a terminal task, no live roles).
    for i in 0..MAX_STUDIOS + 3 {
        let state = state_with(
            &base_events(),
            vec![
                ("director", cast("t1", "actor-a", "a")),
                ("actor", delivered("t1", 1, "a", 1)),
                ("director", reviewed("t1", 1, Verdict::Accepted, Vec::new())),
            ],
        );
        studios.push((format!("done-{i}"), state));
    }
    let s = summary(
        &studios,
        &BTreeMap::new(),
        &BTreeMap::new(),
        &BTreeMap::new(),
        1,
    );
    assert_eq!(s.studios.len(), MAX_STUDIOS);
    assert_eq!(s.studios[0].id, "live-0");
    assert_eq!(s.studios[1].id, "live-1");
    assert!(s.studios[2].id.starts_with("done-"));
    assert_eq!(s.studios[2].state, "finished");
    assert_eq!(s.studios[2].open_reviews, 0);
}

/// The watch pass's `(studio, role)` sample shows only on a live
/// role — a stale entry for a closed one is dropped.
#[test]
fn work_is_sampled_into_live_roles_only() {
    let state = state_with(
        &base_events(),
        vec![
            ("stage", started("producer", "t1", "prod")),
            ("stage", started("writer", "t9", "wri")),
            ("stage", closed("writer")),
        ],
    );
    let work = BTreeMap::from([
        (("acme-shop".into(), "producer".into()), Work::Working),
        (("acme-shop".into(), "writer".into()), Work::Blocked),
    ]);
    let s = summary(
        &[("acme-shop".into(), state)],
        &BTreeMap::new(),
        &work,
        &BTreeMap::new(),
        1,
    );
    let find = |role: &str| s.studios[0].roles.iter().find(|r| r.role == role).unwrap();
    assert_eq!(find("producer").work, Some(Work::Working));
    assert_eq!(find("writer").work, None);
    // Serialization is `working`/`waiting`/`blocked`.
    let json = serde_json::to_string(&s).unwrap();
    assert!(json.contains("\"work\":\"working\""), "{json}");
}

/// An actor carries its task and the task's priority; fixed roles
/// carry neither.
#[test]
fn actors_carry_task_and_priority() {
    let mut base = base_events();
    base.push(ev(
        4,
        "user",
        EventKind::TaskPriority {
            task: "t1".into(),
            priority: Priority::High,
        },
    ));
    base.push(ev(5, "director", cast("t1", "actor-a", "a")));
    let state = state_with(
        &base,
        vec![
            ("stage", started("actor-t1-c1", "t7", "a")),
            ("stage", started("producer", "t1", "prod")),
        ],
    );
    let s = summary(
        &[("acme-shop".into(), state)],
        &BTreeMap::new(),
        &BTreeMap::new(),
        &BTreeMap::new(),
        1,
    );
    let find = |role: &str| s.studios[0].roles.iter().find(|r| r.role == role).unwrap();
    let a = find("actor-t1-c1");
    assert_eq!(a.task.as_deref(), Some("t1"));
    assert_eq!(a.priority, Some(Priority::High));
    let p = find("producer");
    assert_eq!(p.task, None);
    assert_eq!(p.priority, None);
}

/// `resumable` is a closed person with a session id that nothing
/// holds — live people are not it, session-less closed people are
/// not it, and a held person is already taken.
#[test]
fn resumable_is_a_closed_person_with_a_session_and_no_hold() {
    let state = state_with(
        &base_events(),
        vec![
            ("stage", started("writer", "t9", "wri")),
            ("stage", closed("writer")),
            (
                "stage",
                EventKind::RoleStarted {
                    role: "director".into(),
                    terminal: Some("t2".into()),
                    actor: Some(actor("dir")),
                    session_id: None,
                    reason: None,
                },
            ),
            ("stage", closed("director")),
            ("stage", started("producer", "t8", "prod")),
            ("stage", started("actor-t1-c1", "t7", "a")),
            ("stage", closed("actor-t1-c1")),
            (
                "user",
                EventKind::RoleReopen {
                    role: "actor-t1-c1".into(),
                },
            ),
        ],
    );
    let s = summary(
        &[("acme-shop".into(), state)],
        &BTreeMap::new(),
        &BTreeMap::new(),
        &BTreeMap::new(),
        1,
    );
    let find = |role: &str| s.studios[0].roles.iter().find(|r| r.role == role).unwrap();
    assert!(find("writer").resumable); // closed with a session id
    assert!(!find("director").resumable); // closed without one
    assert!(!find("actor-t1-c1").resumable); // held — already taken
    assert!(!find("producer").resumable); // live
    // And no session id reaches the document at all.
    let json = serde_json::to_string(&s).unwrap();
    assert!(!json.contains("sess-"), "{json}");
    assert!(!json.contains("session"), "{json}");
}

/// Open actors sort by priority high-first, then role key; closed
/// people follow, ordered the same way. Fixed roles still lead.
#[test]
fn actors_order_by_priority_then_key_with_closed_last() {
    let mut events = base_events();
    for (seq, t) in [(4, "t2"), (5, "t3"), (6, "t4")] {
        events.push(ev(seq, "director", task_created(t, "fix", 1)));
    }
    let state = state_with(
        &events,
        vec![
            (
                "user",
                EventKind::TaskPriority {
                    task: "t2".into(),
                    priority: Priority::High,
                },
            ),
            (
                "user",
                EventKind::TaskPriority {
                    task: "t3".into(),
                    priority: Priority::Low,
                },
            ),
            ("director", cast("t1", "actor-a", "a")), // t1 normal
            ("director", cast("t2", "actor-b", "b")), // t2 high
            ("director", cast("t3", "actor-a", "a")), // t3 low, c1
            ("director", cast("t3", "actor-b", "b")), // t3 low, c2
            ("director", cast("t4", "actor-b", "b")), // t4 normal
            ("stage", started("actor-t3-c1", "t7", "a")),
            ("stage", started("actor-t1-c1", "t8", "a")),
            ("stage", started("actor-t2-c1", "t9", "b")),
            ("stage", started("producer", "t1", "prod")),
            ("stage", started("actor-t4-c1", "t10", "b")),
            ("stage", closed("actor-t4-c1")),
            ("stage", started("actor-t3-c2", "t11", "a")),
            ("stage", closed("actor-t3-c2")),
        ],
    );
    let s = summary(
        &[("acme-shop".into(), state)],
        &BTreeMap::new(),
        &BTreeMap::new(),
        &BTreeMap::new(),
        1,
    );
    let order: Vec<&str> = s.studios[0].roles.iter().map(|r| r.role.as_str()).collect();
    assert_eq!(
        order,
        [
            "producer",
            "actor-t2-c1", // open, high
            "actor-t1-c1", // open, normal
            "actor-t3-c1", // open, low
            "actor-t4-c1", // closed, normal
            "actor-t3-c2", // closed, low
        ],
        "{order:?}"
    );
}

/// A studio in the errors map carries it on the row — capped — and
/// a studio whose state never loaded still gets a stub row so the
/// panel can say why.
#[test]
fn a_broken_studio_shows_its_error_even_unloaded() {
    let long = "e".repeat(200);
    let errors = BTreeMap::from([
        ("good".to_string(), "act failed".to_string()),
        ("dead".to_string(), long),
    ]);
    let s = summary(
        &[
            ("good".into(), base_state()),
            ("dead".into(), StudioState::default()),
        ],
        &BTreeMap::new(),
        &BTreeMap::new(),
        &errors,
        1,
    );
    assert_eq!(s.studios.len(), 2);
    let good = s.studios.iter().find(|st| st.id == "good").unwrap();
    assert_eq!(good.error.as_deref(), Some("act failed"));
    assert_eq!(good.state, "live");
    let dead = s.studios.iter().find(|st| st.id == "dead").unwrap();
    assert_eq!(dead.error.as_ref().map(String::len), Some(96));
    assert_eq!(dead.state, "live");
    assert!(dead.roles.is_empty());
}

/// Long fields are capped and the document stays small.
#[test]
fn strings_are_capped() {
    let long = "x".repeat(500);
    let mut state = base_state();
    state.studio.as_mut().unwrap().title = long.clone();
    let s = summary(
        &[("acme-shop".into(), state)],
        &BTreeMap::new(),
        &BTreeMap::new(),
        &BTreeMap::new(),
        1,
    );
    assert!(s.studios[0].title.len() <= 96);
    let bytes = serde_json::to_vec(&s).unwrap().len();
    assert!(bytes < 64 * 1024, "{bytes}");
}
