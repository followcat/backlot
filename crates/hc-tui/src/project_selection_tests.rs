use super::*;
use crate::sidebar::fixture::agent;

fn record(state: &str, app: &str, title: &str) -> Status {
    Status {
        state: state.into(),
        app: app.into(),
        title: title.into(),
        ..Status::default()
    }
}

fn selected(term: &Terminal) -> (Option<&str>, AgentStatus, &Status) {
    let chosen = agent_record(term).expect("present record");
    (chosen.id, chosen.state, chosen.status(term).unwrap())
}

#[test]
fn unknown_only_remains_a_present_agent() {
    let mut term = agent("t1", "error");
    term.status = Some(record("error", "root-app", "root-title"));
    term.records.insert(
        "child".into(),
        record("future-state", "child-app", "child-title"),
    );
    let (id, state, status) = selected(&term);
    assert_eq!(
        (id, state, status.app.as_str()),
        (None, AgentStatus::Unknown, "root-app")
    );
    let snapshot = endpoint_snapshot(&[&term], Some("t1"), 1, 1);
    assert_eq!(snapshot.agents.len(), 1);
    assert_eq!(snapshot.agents[0].agent_status, AgentStatus::Unknown);
    assert_eq!(snapshot.agents[0].title.as_deref(), Some("root-title"));

    term.status = None;
    let (id, state, _) = selected(&term);
    assert_eq!((id, state), (Some("child"), AgentStatus::Unknown));
    assert_eq!(
        endpoint_snapshot(&[&term], Some("t1"), 1, 1).agents.len(),
        1
    );
}

#[test]
fn known_winner_supplies_state_metadata_and_its_record_identity() {
    for (root, child, id, state, raw_id, ordinal) in [
        (
            "working",
            "error",
            None,
            AgentStatus::Working,
            Some("child"),
            6,
        ),
        (
            "error",
            "working",
            Some("child"),
            AgentStatus::Working,
            None,
            7,
        ),
        ("idle", "error", None, AgentStatus::Idle, Some("child"), 6),
        (
            "future-state",
            "working",
            Some("child"),
            AgentStatus::Working,
            Some("child"),
            7,
        ),
    ] {
        let mut term = agent("t1", root);
        term.status = Some(record(root, "root-app", "root-title"));
        term.records
            .insert("child".into(), record(child, "child-app", "child-title"));
        term.title = "terminal-title".into();
        term.activity = Some(
            serde_json::from_value(serde_json::json!({
                "core_id": "origin", "epoch": "boot", "scope": 1, "revision": 7,
                "records": {
                    "": {"state_ordinal": 6, "completion_ordinal": null},
                    "child": {"state_ordinal": 7, "completion_ordinal": null}
                }
            }))
            .unwrap(),
        );
        let choice = agent_record(&term).unwrap();
        assert_eq!((choice.id, choice.state), (id, state));
        assert_eq!(
            crate::sidebar::effective(&term).unwrap().record.as_deref(),
            raw_id
        );
        let key = id.unwrap_or("");
        assert_eq!(
            term.activity.as_ref().unwrap().records[key].state_ordinal,
            Some(ordinal)
        );
        let expected = choice.status(&term).unwrap();
        let snapshot = endpoint_snapshot(&[&term], Some("t1"), 1, 1);
        let row = &snapshot.agents[0];
        assert_eq!(row.agent_status, state);
        assert_eq!(row.agent.as_deref(), Some(expected.app.as_str()));
        assert_eq!(row.display_agent.as_deref(), Some(expected.app.as_str()));
        assert_eq!(row.title.as_deref(), Some(expected.title.as_str()));
        assert_eq!(row.terminal_title.as_deref(), Some("terminal-title"));
    }
}

#[test]
fn equal_urgency_prefers_root_then_sorted_named_record() {
    let mut term = agent("t1", "working");
    term.records
        .insert("z".into(), record("working", "z", "z-title"));
    term.records
        .insert("a".into(), record("working", "a", "a-title"));
    assert_eq!(selected(&term).0, None);
    term.status = None;
    assert_eq!(selected(&term).0, Some("a"));
    term.records.get_mut("a").unwrap().state = "error".into();
    term.records.get_mut("z").unwrap().state = "error".into();
    assert_eq!(selected(&term).0, Some("a"));
}

#[test]
fn known_words_keep_raw_sidebar_order_including_working_before_done() {
    let words = ["blocked", "working", "ready", "finished", "done", "idle"];
    for root in words {
        for child in words {
            let mut term = agent("t1", root);
            term.records
                .insert("child".into(), record(child, "child", "child-title"));
            let raw = crate::sidebar::effective(&term).unwrap();
            assert_eq!(
                selected(&term).0,
                raw.record.as_deref(),
                "{root} versus {child}"
            );
        }
    }
    let mut term = agent("t1", "done");
    term.records
        .insert("child".into(), record("working", "child", "child-title"));
    assert_eq!(selected(&term).0, Some("child"));
}
