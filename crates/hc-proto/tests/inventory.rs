//! The pure inventory reducer's record mutations: snapshot replacement
//! and every documented event kind, plus the label filter's view. No I/O,
//! no sequence policy — that lives in `inventory_seq.rs`.

mod common;

use common::inventory::{end_data, ev, term, terminal};
use common::minimal_terminal;
use hc_proto::events::{EventParams, SubscribeParams};
use hc_proto::inventory::{DocRef, Inventory};
use serde_json::{Value, json};

#[test]
fn a_snapshot_initializes_and_reports_created() {
    let mut inv = Inventory::unfiltered();
    let effect = inv.replace(10, vec![term("t7"), term("t8")]);
    assert!(effect.snapshot);
    assert_eq!(effect.created, ["t7", "t8"]);
    assert_eq!(inv.seq(), 10);
    assert_eq!(inv.terminals().len(), 2);
}

#[test]
fn created_and_restarted_replace_whole_records() {
    let mut inv = Inventory::unfiltered();
    inv.replace(1, vec![term("t7")]);
    let mut new_record = minimal_terminal();
    new_record["address"] = json!("t9");
    new_record["title"] = json!("fresh");
    let effect = inv.apply(&ev(2, "created", Some("t9"), new_record));
    assert_eq!(effect.created, ["t9"]);
    assert_eq!(inv.get("t9").unwrap().title, "fresh");
    // `restarted` is a whole-record replacement flagged for reattach.
    let mut record = terminal(minimal_terminal());
    record.address = "t7".into();
    record.pid = Some(555);
    let effect = inv.apply(&ev(
        3,
        "restarted",
        Some("t7"),
        serde_json::to_value(&record).unwrap(),
    ));
    assert_eq!(effect.restarted, ["t7"]);
    assert_eq!(inv.get("t7").unwrap().pid, Some(555));
}

#[test]
fn lost_keeps_live_clears_pid_and_foreground() {
    let mut inv = Inventory::unfiltered();
    let mut kept = terminal(minimal_terminal());
    kept.keep = true;
    kept.pid = Some(4242);
    kept.foreground = Some(
        serde_json::from_value(json!({
            "pid": 7, "name": "sh", "argv": ["sh"], "cwd": "/home/user",
        }))
        .unwrap(),
    );
    inv.replace(1, vec![kept]);
    let effect = inv.apply(&ev(
        2,
        "lost",
        Some("t7"),
        json!({"reason": "signaled", "exit_code": null, "signal": 9, "finished_ms": null}),
    ));
    assert_eq!(effect.changed, ["t7"]);
    let record = inv.get("t7").unwrap();
    assert_eq!(record.state, "live");
    assert_eq!(record.pid, None);
    assert_eq!(record.foreground, None);
    assert_eq!(record.reason.as_deref(), Some("signaled"));
    assert_eq!(record.signal, Some(9));
    assert_eq!(record.finished_ms, None);
}

#[test]
fn finished_keeps_its_pid_but_drops_foreground() {
    let mut inv = Inventory::unfiltered();
    let mut running = terminal(minimal_terminal());
    running.pid = Some(4242);
    running.foreground = Some(
        serde_json::from_value(json!({
            "pid": 7, "name": "sh", "argv": ["sh"], "cwd": "/home/user",
        }))
        .unwrap(),
    );
    inv.replace(1, vec![running]);
    inv.apply(&ev(2, "finished", Some("t7"), end_data()));
    let record = inv.get("t7").unwrap();
    assert_eq!(record.state, "finished");
    // Documented: finished drops foreground but invents no pid update.
    assert_eq!(record.pid, Some(4242));
    assert_eq!(record.foreground, None);
    assert_eq!(record.exit_code, Some(0));
}

#[test]
fn status_replaces_root_records_and_reported() {
    let mut inv = Inventory::unfiltered();
    inv.replace(1, vec![term("t7")]);
    let effect = inv.apply(&ev(
        2,
        "status",
        Some("t7"),
        json!({
            "status": {"state": "working", "source": "tagged-agent"},
            "records": {"sub": {"state": "done"}},
            "reported": true,
        }),
    ));
    assert_eq!(effect.changed, ["t7"]);
    let record = inv.get("t7").unwrap();
    assert_eq!(record.status.as_ref().unwrap().state, "working");
    // Source-tagged extensions survive in `extra`, never interpreted.
    assert_eq!(
        record.status.as_ref().unwrap().extra["source"],
        "tagged-agent"
    );
    assert_eq!(record.records["sub"].state, "done");
    assert!(record.reported);
    // A null root clears it.
    inv.apply(&ev(
        3,
        "status",
        Some("t7"),
        json!({"status": null, "records": {}, "reported": true}),
    ));
    assert!(inv.get("t7").unwrap().status.is_none());
}

#[test]
fn turn_title_size_keep_and_foreground_apply() {
    let mut inv = Inventory::unfiltered();
    inv.replace(1, vec![term("t7")]);
    let effect = inv.apply(&ev(
        2,
        "turn",
        Some("t7"),
        json!({"turn": 4, "state": "done"}),
    ));
    assert_eq!(inv.get("t7").unwrap().turn, 4);
    assert_eq!(effect.changed, ["t7"]);
    inv.apply(&ev(3, "title", Some("t7"), json!({"title": "renamed"})));
    assert_eq!(inv.get("t7").unwrap().title, "renamed");
    let effect = inv.apply(&ev(4, "size", Some("t7"), json!({"cols": 132, "rows": 43})));
    assert_eq!(effect.resized, ["t7"]);
    assert_eq!(
        (inv.get("t7").unwrap().cols, inv.get("t7").unwrap().rows),
        (132, 43)
    );
    inv.apply(&ev(5, "keep", Some("t7"), json!({"keep": true})));
    assert!(inv.get("t7").unwrap().keep);
    let fg = json!({"pid": 9, "name": "vim", "argv": ["vim"], "cwd": "/home/user"});
    inv.apply(&ev(6, "foreground", Some("t7"), fg));
    assert_eq!(
        inv.get("t7").unwrap().foreground.as_ref().unwrap().name,
        "vim"
    );
    inv.apply(&ev(7, "foreground", Some("t7"), Value::Null));
    assert!(inv.get("t7").unwrap().foreground.is_none());
}

#[test]
fn removed_drops_the_record() {
    let mut inv = Inventory::unfiltered();
    inv.replace(1, vec![term("t7")]);
    let effect = inv.apply(&ev(2, "removed", Some("t7"), json!({})));
    assert_eq!(effect.removed, ["t7"]);
    assert!(inv.get("t7").is_none());
}

#[test]
fn labels_update_carry_record_and_filter() {
    let mut inv = Inventory::unfiltered();
    inv.replace(1, vec![term("t7")]);
    inv.apply(&ev(
        2,
        "labels",
        Some("t7"),
        json!({"labels": {"herdr.space": "work"}}),
    ));
    assert_eq!(inv.get("t7").unwrap().labels["herdr.space"], "work");
    // A `record` member adds the whole terminal when it newly matched.
    let mut record = minimal_terminal();
    record["address"] = json!("t8");
    let effect = inv.apply(&ev(
        3,
        "labels",
        Some("t8"),
        json!({"labels": {"x": "y"}, "record": record}),
    ));
    assert_eq!(effect.created, ["t8"]);
    assert!(inv.get("t8").is_some());
}

#[test]
fn a_label_filter_drops_a_terminal_that_stops_matching() {
    let params = SubscribeParams {
        labels: [("herdr.space".to_string(), "work".to_string())].into(),
        ..SubscribeParams::default()
    };
    let mut inv = Inventory::for_subscription(&params);
    let mut t = term("t7");
    t.labels.insert("herdr.space".into(), "work".into());
    inv.replace(1, vec![t]);
    // The labels event still arrives; the terminal left the view.
    let effect = inv.apply(&ev(
        2,
        "labels",
        Some("t7"),
        json!({"labels": {"herdr.space": "play"}}),
    ));
    assert_eq!(effect.removed, ["t7"]);
    assert!(inv.get("t7").is_none());
    // Matching again returns it as `record`.
    let mut record = term("t7");
    record.labels.insert("herdr.space".into(), "work".into());
    let effect = inv.apply(&ev(
        3,
        "labels",
        Some("t7"),
        json!({"labels": {"herdr.space": "work"}, "record": serde_json::to_value(&record).unwrap()}),
    ));
    assert_eq!(effect.created, ["t7"]);
    assert!(inv.get("t7").is_some());
}

#[test]
fn doc_events_signal_a_refetch_with_machine_and_name() {
    let mut inv = Inventory::unfiltered();
    let effect = inv.apply(&EventParams {
        seq: 2,
        kind: "doc".into(),
        terminal: None,
        machine: Some("host-b".into()),
        data: json!({"name": "herdr-app", "version": 2}),
    });
    assert_eq!(
        effect.docs,
        [DocRef {
            machine: Some("host-b".into()),
            name: "herdr-app".into()
        }]
    );
    // Local doc events carry no machine.
    let effect = inv.apply(&EventParams {
        seq: 3,
        kind: "doc".into(),
        terminal: None,
        machine: None,
        data: json!({"name": "mine", "version": 1}),
    });
    assert_eq!(effect.docs[0].machine, None);
}

#[test]
fn an_events_view_of_a_remote_terminal_uses_its_address() {
    let mut inv = Inventory::unfiltered();
    inv.replace(1, vec![]);
    let mut record = minimal_terminal();
    record["address"] = json!("host-b/t7");
    inv.apply(&ev(2, "created", Some("host-b/t7"), record));
    assert!(inv.get("host-b/t7").is_some());
}
