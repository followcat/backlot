//! Standby role summaries (for unstarted fixed roles) and v0.4.0 wire
//! compatibility.

use std::collections::BTreeMap;

use serde::Deserialize;

use crate::attention::AttentionClass;
use crate::event::{ActorIdentity, EventKind, Priority, StudioMode};
use crate::progress::{RoleProgress, RoleRunning};
use crate::state::{StudioState, replay};
use crate::summary::{Summary, Work, default_true, summary};
use crate::testkit::*;

fn build_summary(state: StudioState) -> Summary {
    summary(
        &[("acme-shop".to_string(), state)],
        &BTreeMap::new(),
        &BTreeMap::new(),
        &BTreeMap::new(),
        &BTreeMap::new(),
        0,
    )
}

fn identity(runner: &str, model: &str, effort: Option<&str>) -> ActorIdentity {
    ActorIdentity {
        runner: runner.into(),
        runner_version: None,
        model: model.into(),
        effort: effort.map(str::to_owned),
        permission_profile: "standard".into(),
    }
}

fn live_role(
    seq: u64,
    role: &str,
    terminal: Option<&str>,
    actor: ActorIdentity,
) -> crate::event::Event {
    ev(
        seq,
        "stage",
        EventKind::RoleStarted {
            role: role.into(),
            terminal: terminal.map(str::to_owned),
            actor: Some(actor),
            session_id: Some(format!("sess-{role}")),
            reason: None,
            prompt_hash: None,
        },
    )
}

#[test]
fn unstarted_recast_writer_is_standby_until_plan_and_start() {
    let mut events = base_events();
    events.push(live_role(
        4,
        "producer",
        Some("bl-acme-shop-producer"),
        actor("prod"),
    ));
    events.push(live_role(
        5,
        "director",
        Some("bl-acme-shop-director"),
        actor("dir"),
    ));
    let recast_actor = identity("cmd", "new-writer", Some("high"));
    events.push(ev(
        6,
        "user",
        EventKind::RoleRecast {
            role: "writer".into(),
            actor: recast_actor.clone(),
            reason: Some("quota".into()),
            card: None,
        },
    ));
    let doc = build_summary(replay(&events));
    let roles = &doc.studios[0].roles;
    let names: Vec<&str> = roles.iter().map(|r| r.role.as_str()).collect();
    assert_eq!(names, vec!["producer", "director", "writer"]);

    let writer = roles
        .iter()
        .find(|r| r.role == "writer")
        .expect("writer standby");
    assert_eq!(writer.name, "writer");
    assert_eq!(writer.state, "standby");
    assert_eq!(writer.terminal, None);
    assert_eq!(writer.runner.as_deref(), Some("cmd"));
    assert_eq!(writer.model.as_deref(), Some("new-writer"));
    assert_eq!(writer.effort.as_deref(), Some("high"));
    assert!(writer.can_answer_prompt);
    assert_eq!(writer.doing, None);
    assert_eq!(writer.work, None);
    assert_eq!(writer.failed, None);
    assert_eq!(writer.attention, None);
    assert_eq!(writer.asks, None);
    assert!(!writer.attention_user);
    assert!(!writer.resumable);
    assert_eq!(writer.since_ms, 0);

    // Once the director asks for a plan and the writer starts, it becomes live.
    events.push(ev(
        7,
        "director",
        EventKind::MessageQueued {
            to: "writer".into(),
            file: Some("messages/m7.md".into()),
            purpose: Some("plan".into()),
            source_seq: None,
        },
    ));
    events.push(live_role(
        8,
        "writer",
        Some("bl-acme-shop-writer"),
        recast_actor,
    ));
    let doc = build_summary(replay(&events));
    let writer = doc.studios[0]
        .roles
        .iter()
        .find(|r| r.role == "writer")
        .expect("live writer");
    assert_eq!(writer.state, "live");
    assert_eq!(writer.terminal.as_deref(), Some("bl-acme-shop-writer"));
    assert_eq!(writer.runner.as_deref(), Some("cmd"));
    assert_eq!(writer.model.as_deref(), Some("new-writer"));
}

#[test]
fn unstarted_fixed_roles_without_recast_keep_cast_identity_and_order() {
    let mut events = base_events();
    if let EventKind::StudioCreated { roles, .. } = &mut events[0].kind {
        roles.writer = identity("opencode", "cast-writer", Some("medium"));
    }
    // Right after studio.new, all three fixed roles are standby in fixed order.
    let early_doc = build_summary(replay(&events));
    let early_roles: Vec<(&str, &str)> = early_doc.studios[0]
        .roles
        .iter()
        .map(|r| (r.role.as_str(), r.state.as_str()))
        .collect();
    assert_eq!(
        early_roles,
        vec![
            ("producer", "standby"),
            ("director", "standby"),
            ("writer", "standby"),
        ]
    );

    // Once producer, director, and an actor run, writer stays standby before actors.
    events.push(live_role(
        4,
        "producer",
        Some("bl-acme-shop-producer"),
        actor("prod"),
    ));
    events.push(live_role(
        5,
        "director",
        Some("bl-acme-shop-director"),
        actor("dir"),
    ));
    events.push(ev(6, "director", cast("t1", "actor-a", "a")));
    events.push(live_role(
        7,
        "actor-t1-c1",
        Some("bl-acme-shop-t1-c1"),
        actor("a"),
    ));
    let doc = build_summary(replay(&events));
    let order: Vec<&str> = doc.studios[0]
        .roles
        .iter()
        .map(|r| r.role.as_str())
        .collect();
    assert_eq!(order, vec!["producer", "director", "writer", "actor-t1-c1"]);

    let writer = &doc.studios[0].roles[2];
    assert_eq!(writer.state, "standby");
    assert_eq!(writer.runner.as_deref(), Some("opencode"));
    assert_eq!(writer.model.as_deref(), Some("cast-writer"));
    assert_eq!(writer.effort.as_deref(), Some("medium"));
    assert!(!writer.can_answer_prompt);
    assert_eq!(writer.terminal, None);
}

// Frozen v0.4.0 wire format (from tag v0.4.0 crates/backlot-model/src/summary.rs).
mod v040_wire {
    use super::*;

    #[derive(Debug, Deserialize)]
    pub struct Summary {
        pub schema: String,
        #[serde(default)]
        pub studios: Vec<StudioSummary>,
    }

    #[derive(Debug, Deserialize)]
    #[allow(dead_code)]
    pub struct StudioSummary {
        pub id: String,
        pub title: String,
        pub state: String,
        pub requirement_version: u32,
        #[serde(default)]
        pub req_file: Option<String>,
        pub mode: StudioMode,
        pub open_reviews: u32,
        pub waiting: u32,
        #[serde(default)]
        pub error: Option<String>,
        #[serde(default)]
        pub next: Option<String>,
        #[serde(default)]
        pub repo: Option<String>,
        #[serde(default)]
        pub ready_to_finish: bool,
        #[serde(default)]
        pub tasks_done: u32,
        #[serde(default)]
        pub tasks_total: u32,
        #[serde(default)]
        pub roles: Vec<RoleSummary>,
    }

    #[derive(Debug, Deserialize)]
    #[allow(dead_code)]
    pub struct RoleSummary {
        pub role: String,
        pub name: String,
        #[serde(default)]
        pub terminal: Option<String>,
        #[serde(default)]
        pub runner: Option<String>,
        #[serde(default)]
        pub model: Option<String>,
        #[serde(default)]
        pub effort: Option<String>,
        pub state: String,
        #[serde(default)]
        pub doing: Option<String>,
        #[serde(default)]
        pub progress: Option<RoleProgress>,
        #[serde(default)]
        pub running: Option<RoleRunning>,
        #[serde(default)]
        pub work: Option<Work>,
        #[serde(default)]
        pub failed: Option<String>,
        #[serde(default)]
        pub workdir: Option<String>,
        #[serde(default)]
        pub task: Option<String>,
        #[serde(default)]
        pub priority: Option<Priority>,
        #[serde(default)]
        pub resumable: bool,
        #[serde(default)]
        pub since_ms: u64,
        #[serde(default)]
        pub attention: Option<String>,
        #[serde(default)]
        pub attention_class: Option<AttentionClass>,
        #[serde(default)]
        pub asks: Option<String>,
        #[serde(default)]
        pub attention_user: bool,
        #[serde(default)]
        pub attention_seq: Option<u64>,
        #[serde(default)]
        pub attention_detail: Option<String>,
        #[serde(default)]
        pub stage_answer_ms: Option<u64>,
        #[serde(default = "default_true")]
        pub can_answer_prompt: bool,
        #[serde(default)]
        pub restarted_ms: Option<u64>,
    }
}

#[test]
fn wire_new_summary_with_standby_parses_into_v040_structs() {
    let mut events = base_events();
    events.push(live_role(
        4,
        "producer",
        Some("bl-acme-shop-producer"),
        actor("prod"),
    ));
    events.push(live_role(
        5,
        "director",
        Some("bl-acme-shop-director"),
        actor("dir"),
    ));
    let doc = build_summary(replay(&events));
    let raw = serde_json::to_string(&doc).expect("serialize new summary");
    let parsed: v040_wire::Summary = serde_json::from_str(&raw).expect("v0.4.0 parse");
    assert_eq!(parsed.schema, "backlot/1");
    assert_eq!(parsed.studios.len(), 1);
    let writer = parsed.studios[0]
        .roles
        .iter()
        .find(|r| r.role == "writer")
        .expect("writer present in v0.4.0 struct");
    assert_eq!(writer.state, "standby");
    assert_eq!(writer.terminal, None);
    assert_eq!(writer.attention, None);
    assert_eq!(writer.asks, None);
    assert_eq!(writer.work, None);
    assert_eq!(writer.failed, None);
    assert!(!writer.attention_user);
}
