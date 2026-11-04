//! Terminal record types against neutral fixtures: nullable fields, defaults
//! on sparse records, unknown states and extension fields.

mod common;

use common::minimal_terminal;
use hc_proto::terminal::{
    AttachResult, ListResult, ReadResult, RunParams, RunResult, Status, Terminal,
};
use serde_json::json;

#[test]
fn minimal_terminal_record_parses() {
    let t: Terminal = serde_json::from_value(minimal_terminal()).unwrap();
    assert_eq!(t.address, "t7");
    assert_eq!(t.cwd, "/home/user");
    assert!(!t.keep);
    assert!(t.status.is_none());
    assert!(t.foreground.is_none());
    assert!(t.extra.is_empty());
}

#[test]
fn full_terminal_record_keeps_nulls_fields_and_extras() {
    let t: Terminal = serde_json::from_value(json!({
        "address": "t7",
        "name": "agent-1",
        "labels": {"herdr.space": "acme-shop", "herdr.tab": "api"},
        "argv": ["agent", "--task", "fix"],
        "cwd": "/home/user/acme-shop",
        "keep": true,
        "state": "paused",
        "pid": 4242,
        "exit_code": null,
        "signal": null,
        "reason": null,
        "created_ms": 1700000000000u64,
        "finished_ms": null,
        "screen": "exact",
        "cols": 132,
        "rows": 43,
        "identity": "agent-shell 1.0",
        "status": {"state": "blocked", "kind": "permission", "progress": 40, "app": "agent", "title": "deploy", "msg": "allow?"},
        "status_ms": 1700000005000u64,
        "records": {"sub-1": {"state": "working"}},
        "turn": 3,
        "reported": true,
        "title": "deploy the shop",
        "parent": "t1",
        "notify": true,
        "foreground": {"pid": 4242, "name": "agent", "argv": ["agent", "--task", "fix"], "cwd": "/home/user/acme-shop"},
        "fork_field": {"a_fork": true},
    }))
    .unwrap();
    assert_eq!(t.name.as_deref(), Some("agent-1"));
    assert_eq!(t.labels["herdr.space"], "acme-shop");
    assert_eq!(t.state, "paused"); // a state generation 1 does not define still parses
    assert_eq!(t.pid, Some(4242));
    assert_eq!(t.exit_code, None);
    let status = t.status.as_ref().unwrap();
    assert_eq!(status.state, "blocked");
    assert_eq!(status.kind.as_deref(), Some("permission"));
    assert_eq!(status.progress, Some(40));
    assert_eq!(t.records["sub-1"].state, "working");
    assert_eq!(t.turn, 3);
    assert_eq!(t.parent.as_deref(), Some("t1"));
    assert_eq!(t.foreground.as_ref().unwrap().cwd, "/home/user/acme-shop");
    // Fields beyond the schema are retained, not dropped.
    assert_eq!(t.extra["fork_field"], json!({"a_fork": true}));
}

#[test]
fn status_extra_fields_round_trip() {
    // A patched core may tag records, e.g. `source: screen`. Retained, never
    // interpreted.
    let status: Status = serde_json::from_value(json!({
        "state": "working",
        "source": "screen",
        "tracker": {"eta_ms": 900}
    }))
    .unwrap();
    assert_eq!(status.state, "working");
    assert_eq!(status.extra["source"], "screen");
    let back = serde_json::to_value(&status).unwrap();
    assert_eq!(back["source"], "screen");
    assert_eq!(back["tracker"], json!({"eta_ms": 900}));
    assert_eq!(back["state"], "working");
}

#[test]
fn attach_result_defaults_capability_flags() {
    // An older reply without the new flags must not enable the features.
    let attach: AttachResult = serde_json::from_value(json!({
        "channel": 4,
        "terminal": minimal_terminal(),
    }))
    .unwrap();
    assert_eq!(attach.channel, 4);
    assert!(!attach.resize_notifications);
    assert!(!attach.automatic_input);

    let attach: AttachResult = serde_json::from_value(json!({
        "channel": 4,
        "terminal": minimal_terminal(),
        "resize_notifications": true,
        "automatic_input": true,
    }))
    .unwrap();
    assert!(attach.resize_notifications && attach.automatic_input);
}

#[test]
fn list_result_defaults_missing_machines() {
    // The core skips `machines` when empty; older replies may lack the key.
    let list: ListResult = serde_json::from_value(json!({
        "terminals": [minimal_terminal()],
        "hidden_finished": 2,
    }))
    .unwrap();
    assert_eq!(list.terminals.len(), 1);
    assert_eq!(list.hidden_finished, 2);
    assert!(list.machines.is_empty());
}

#[test]
fn read_result_optional_why() {
    let read: ReadResult = serde_json::from_value(json!({
        "text": "screen text", "complete": true, "how": "screen",
    }))
    .unwrap();
    assert!(read.why.is_none());
    let read: ReadResult = serde_json::from_value(json!({
        "text": "screen text", "complete": false, "how": "screen", "why": "working",
    }))
    .unwrap();
    assert_eq!(read.why.as_deref(), Some("working"));
    assert!(!read.complete);
}

#[test]
fn run_result_flattens_terminal_output_and_wait() {
    let mut value = minimal_terminal();
    let obj = value.as_object_mut().unwrap();
    obj.insert("output".into(), json!("did the thing\n"));
    obj.insert(
        "wait".into(),
        json!({
            "terminal": "t7", "ended": true, "reason": "exit", "turn": 1,
            "children": [], "exit_code": 0,
        }),
    );
    let run: RunResult = serde_json::from_value(value).unwrap();
    assert_eq!(run.terminal.address, "t7");
    assert_eq!(run.output.as_deref(), Some("did the thing\n"));
    let wait = run.wait.as_ref().unwrap();
    assert!(wait.ended);
    assert_eq!(wait.exit_code, Some(0));
}

#[test]
fn run_params_serialize_only_what_is_set() {
    assert_eq!(
        serde_json::to_value(RunParams::default()).unwrap(),
        json!({})
    );
    let params = RunParams {
        argv: vec!["agent".into(), "--task".into(), "fix".into()],
        cwd: Some("/home/user/acme-shop".into()),
        name: Some("agent-1".into()),
        labels: [("herdr.space".to_string(), "acme-shop".to_string())]
            .into_iter()
            .collect(),
        ..RunParams::default()
    };
    assert_eq!(
        serde_json::to_value(&params).unwrap(),
        json!({
            "argv": ["agent", "--task", "fix"],
            "cwd": "/home/user/acme-shop",
            "name": "agent-1",
            "labels": {"herdr.space": "acme-shop"},
        })
    );
}
