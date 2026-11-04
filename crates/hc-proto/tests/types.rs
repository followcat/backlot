//! Typed API data against neutral fixtures: hello capabilities, subscribe,
//! events, machines and docs. Fixture names are neutral only: host-a,
//! host-b, alice, /home/user, acme-shop.

mod common;

use common::minimal_terminal;
use hc_proto::doc::DocGetResult;
use hc_proto::events::{
    EventParams, HeartbeatParams, MissedParams, ServerNotification, SubscribeParams,
    SubscribeResult, notification,
};
use hc_proto::hello::{Capabilities, HelloParams, HelloResult};
use hc_proto::jsonrpc::{Message, Notification};
use hc_proto::machine::MachineListResult;
use hc_proto::{PROTOCOL_GENERATION, method};
use serde_json::json;

fn hello_json(generation: u32) -> serde_json::Value {
    json!({
        "generation": generation,
        "version": "0.1.17",
        "core": {"id": "core-1", "name": "host-a"},
        "framed": true,
        "methods": ["hello", "rpc.discover", "terminal.list", "terminal.get",
                    "terminal.run", "terminal.read", "terminal.kill",
                    "terminal.attach", "events.subscribe", "machine.list", "doc.get"],
        "features": ["automatic_input", "resize_notifications"],
    })
}

#[test]
fn hello_reply_gives_checked_capabilities() {
    let hello: HelloResult = serde_json::from_value(hello_json(1)).unwrap();
    let caps = Capabilities::new(&hello).unwrap();
    assert!(caps.has_method(method::TERMINAL_ATTACH));
    assert!(caps.has_method(method::DOC_GET));
    assert!(caps.has_feature(hc_proto::hello::feature::AUTOMATIC_INPUT));
    assert!(caps.has_feature(hc_proto::hello::feature::RESIZE_NOTIFICATIONS));
    assert!(!caps.has_feature("hyperspace"));
    assert_eq!(
        caps.missing_methods(&["terminal.list", "terminal.send"]),
        vec!["terminal.send"]
    );
    assert!(caps.explain_unavailable(method::TERMINAL_LIST).is_none());
    let why = caps.explain_unavailable("terminal.send").unwrap();
    assert!(why.contains("terminal.send"), "{why}");
    assert!(why.contains("host-a"), "{why}");
}

#[test]
fn hello_wrong_generation_is_rejected() {
    for generation in [0, 2, 9] {
        let hello: HelloResult = serde_json::from_value(hello_json(generation)).unwrap();
        let err = Capabilities::new(&hello).unwrap_err();
        assert_eq!(err.found, generation);
    }
}

#[test]
fn old_hello_without_method_or_feature_lists_degrades() {
    let hello: HelloResult = serde_json::from_value(json!({
        "generation": 1,
        "version": "0.0.9",
        "core": {"id": "core-0", "name": "host-a"},
        "framed": false,
    }))
    .unwrap();
    let caps = Capabilities::new(&hello).unwrap();
    assert!(!caps.has_method(method::TERMINAL_LIST));
    assert!(!caps.has_feature(hc_proto::hello::feature::AUTOMATIC_INPUT));
    assert!(caps.explain_unavailable(method::EVENTS_SUBSCRIBE).is_some());
}

#[test]
fn hello_params_serialize_sparse() {
    let value = serde_json::to_value(HelloParams::framed()).unwrap();
    assert_eq!(
        value,
        json!({"generation": PROTOCOL_GENERATION, "framed": true})
    );
    let value = serde_json::to_value(HelloParams::new()).unwrap();
    assert_eq!(value, json!({"generation": PROTOCOL_GENERATION}));
}

#[test]
fn subscribe_result_and_channel() {
    let sub: SubscribeResult = serde_json::from_value(json!({
        "seq": 41,
        "terminals": [minimal_terminal()],
    }))
    .unwrap();
    assert_eq!(sub.seq, 41);
    assert_eq!(sub.channel, None);
    let sub: SubscribeResult = serde_json::from_value(json!({
        "seq": 41, "terminals": [], "channel": 3,
    }))
    .unwrap();
    assert_eq!(sub.channel, Some(3));
}

#[test]
fn subscribe_params_cover_all_filters() {
    let params = SubscribeParams {
        terminal: Some("t7".into()),
        children_of: Some("t1".into()),
        labels: [("herdr.space".to_string(), "acme-shop".to_string())]
            .into_iter()
            .collect(),
    };
    assert_eq!(
        serde_json::to_value(&params).unwrap(),
        json!({"terminal": "t7", "children_of": "t1", "labels": {"herdr.space": "acme-shop"}})
    );
}

#[test]
fn event_notification_decodes_known_and_unknown_kinds() {
    let line = br#"{"jsonrpc":"2.0","method":"event","params":{"seq":42,"type":"status","terminal":"t7","data":{"status":{"state":"done"},"records":{},"reported":true}}}"#;
    let Message::Notification(n) = Message::parse(line).unwrap() else {
        panic!("expected notification");
    };
    let Some(ServerNotification::Event(event)) = ServerNotification::from_notification(&n).unwrap()
    else {
        panic!("expected event");
    };
    assert_eq!(event.seq, 42);
    assert_eq!(event.kind, "status");
    assert_eq!(event.terminal.as_deref(), Some("t7"));
    assert_eq!(event.data["status"]["state"], "done");

    // A kind this protocol version does not define still parses.
    let n = Notification {
        jsonrpc: Default::default(),
        method: "event".into(),
        params: Some(json!({"seq": 43, "type": "hovered", "data": {"x": 1}})),
    };
    let Some(ServerNotification::Event(event)) = ServerNotification::from_notification(&n).unwrap()
    else {
        panic!("expected event");
    };
    assert_eq!(event.kind, "hovered");

    // And a notification method we do not model is ignored, not an error.
    let n = Notification {
        jsonrpc: Default::default(),
        method: "future_signal".into(),
        params: None,
    };
    assert_eq!(ServerNotification::from_notification(&n).unwrap(), None);
}

#[test]
fn event_for_a_document_carries_machine() {
    let event: EventParams = serde_json::from_value(json!({
        "seq": 43, "type": "doc", "machine": "host-b",
        "data": {"name": "herdr-app", "version": 2},
    }))
    .unwrap();
    assert_eq!(event.kind, "doc");
    assert_eq!(event.machine.as_deref(), Some("host-b"));
    assert!(event.terminal.is_none());
}

#[test]
fn missed_and_heartbeat_decode() {
    let missed: MissedParams = serde_json::from_value(json!({
        "seq": 57, "terminals": [minimal_terminal()], "machine": "host-b",
    }))
    .unwrap();
    assert_eq!(missed.seq, 57);
    assert_eq!(missed.terminals.len(), 1);
    assert_eq!(missed.machine.as_deref(), Some("host-b"));

    let hb: HeartbeatParams = serde_json::from_value(json!({"seq": 42})).unwrap();
    assert_eq!(hb.seq, 42);

    let n = Notification {
        jsonrpc: Default::default(),
        method: notification::MISSED.into(),
        params: Some(json!({"seq": 57, "terminals": []})),
    };
    assert!(matches!(
        ServerNotification::from_notification(&n).unwrap(),
        Some(ServerNotification::Missed(_))
    ));
}

#[test]
fn machine_list_with_unknown_state_parses() {
    let list: MachineListResult = serde_json::from_value(json!({
        "machines": [{
            "name": "host-b", "target": "alice@host-b", "id": "core-2",
            "host": "host-b", "version": "0.1.17", "state": "dialing",
            "session": null, "why": "no answer", "since_ms": 1700000000000u64,
        }],
    }))
    .unwrap();
    let m = &list.machines[0];
    assert_eq!(m.name, "host-b");
    assert_eq!(m.target, "alice@host-b");
    assert_eq!(m.state, "dialing");
    assert_eq!(m.why.as_deref(), Some("no answer"));
    assert!(m.session.is_none());
}

#[test]
fn doc_get_unwritten_is_null_version_zero() {
    let doc: DocGetResult = serde_json::from_value(json!({"value": null, "version": 0})).unwrap();
    assert!(doc.value.is_null());
    assert_eq!(doc.version, 0);
    let doc: DocGetResult =
        serde_json::from_value(json!({"value": {"panes": [1]}, "version": 7})).unwrap();
    assert_eq!(doc.value["panes"], json!([1]));
}
