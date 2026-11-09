//! Summary document tests: shape, caps, closed roles staying listed
//! and the finished-studio drop-off.

use std::collections::BTreeMap;

use crate::event::{EventKind, Verdict};
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
    let s = summary(&[("acme-shop".into(), state)], &live_view(), 42);
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
    let s = summary(&[("acme-shop".into(), state)], &BTreeMap::new(), 1);
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
    let s = summary(&[("acme-shop".into(), state)], &BTreeMap::new(), 1);
    let a = s.studios[0]
        .roles
        .iter()
        .find(|r| r.role == "actor-t1-c1")
        .unwrap();
    assert_eq!(a.name, "actor-a");
    assert_eq!(a.doing.as_deref(), Some("task t1"));
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
                },
            ),
        ],
    );
    let s = summary(&[("acme-shop".into(), state)], &BTreeMap::new(), 1);
    assert_eq!(s.studios[0].open_reviews, 1);
    assert_eq!(s.studios[0].waiting, 1);
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
    let s = summary(&studios, &BTreeMap::new(), 1);
    assert_eq!(s.studios.len(), MAX_STUDIOS);
    assert_eq!(s.studios[0].id, "live-0");
    assert_eq!(s.studios[1].id, "live-1");
    assert!(s.studios[2].id.starts_with("done-"));
    assert_eq!(s.studios[2].state, "finished");
    assert_eq!(s.studios[2].open_reviews, 0);
}

/// Long fields are capped and the document stays small.
#[test]
fn strings_are_capped() {
    let long = "x".repeat(500);
    let mut state = base_state();
    state.studio.as_mut().unwrap().title = long.clone();
    let s = summary(&[("acme-shop".into(), state)], &BTreeMap::new(), 1);
    assert!(s.studios[0].title.len() <= 96);
    let bytes = serde_json::to_vec(&s).unwrap().len();
    assert!(bytes < 64 * 1024, "{bytes}");
}
