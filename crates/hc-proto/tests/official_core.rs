//! Official `hc` speaks the same generation-1 wire — minus every fork
//! addition. These fakes answer like official does: no fork features in
//! `hello`, no `activity`/`repository`/`hostname`/`waiting_for_person`
//! members in records, no `activity` member in `status` event data.
//! Nothing here may error, stall or silently require the fork fields.

mod common;

use common::attach::{joined, opening, params};
use common::fakecore::{ALL_METHODS, attach_result, pair};
use common::minimal_terminal;
use common::watcher::{METHODS, await_view, serve_rpc, serve_subscribe, spawn_fake, watcher};
use hc_proto::client::Client;
use hc_proto::hello::feature;
use hc_proto::watch::Link;
use serde_json::{Value, json};
use tokio::sync::mpsc;

/// An official-core terminal record: every schema-required member plus
/// `turn`/`reported`/`status`, none of the fork additions.
fn official_term(address: &str) -> Value {
    let mut record = minimal_terminal();
    record["address"] = json!(address);
    record["pid"] = json!(100);
    record["reported"] = json!(true);
    record["status"] = json!({"state": "idle", "app": "fixture"});
    record["turn"] = json!(1);
    record["labels"] = json!({"herdr.seen": "0"});
    record
}

/// The same record as a mirrored fork machine reports it.
fn fork_term(address: &str) -> Value {
    let mut record = official_term(address);
    record["repository"] = json!({
        "core_id": "origin-b", "cwd": "/home/user/acme-shop", "state": "ready",
        "auto_label": "acme-shop", "branch": "main",
        "ahead_behind": {"ahead": 0, "behind": 0}, "checkout": null
    });
    record["activity"] = json!({
        "core_id": "origin-b", "epoch": "boot-b", "scope": 1, "revision": 7,
        "records": {"": {"state_ordinal": 7, "completion_ordinal": 7}}
    });
    record["waiting_for_person"] = json!(false);
    record
}

#[tokio::test]
async fn hello_without_fork_features_reports_each_absent() {
    let (mut core, read, write) = pair(64 * 1024);
    let server = tokio::spawn(async move {
        // Official advertises only the attach-negotiation features.
        core.serve_hello_features(ALL_METHODS, &["automatic_input", "resize_notifications"])
            .await;
        core.expect_eof().await;
    });
    let client = Client::open(read, write, None).await.unwrap();
    let caps = client.capabilities();
    for fork_only in [
        feature::REPOSITORY_FACTS,
        feature::RECORD_ACTIVITY,
        feature::OS_HOSTNAME,
        feature::QUIET_HUMAN_RELEASE,
    ] {
        assert!(!caps.has_feature(fork_only), "{fork_only} must read absent");
    }
    assert!(caps.has_feature(feature::AUTOMATIC_INPUT));
    assert!(caps.has_feature(feature::RESIZE_NOTIFICATIONS));
    assert!(client.core().hostname.is_none());
    drop(client);
    joined(server).await;
}

#[tokio::test]
async fn attach_without_the_feature_never_asks_for_quiet_release() {
    let (mut core, read, write) = pair(64 * 1024);
    let server = tokio::spawn(async move {
        // Official's reply carries no `quiet_human_release` member at all.
        let attach = core
            .serve_attach(
                ALL_METHODS,
                &["automatic_input", "resize_notifications"],
                attach_result(1, minimal_terminal(), true, true),
            )
            .await;
        assert_ne!(attach["params"]["quiet_human_release"], true);
        core.expect_eof().await;
    });
    let mut p = params();
    p.quiet_human_release = true; // the pane asks; this core never offered
    let attach = opening(read, write, &p).await;
    assert!(!attach.quiet_human_release());
    assert!(attach.automatic_input());
    drop(attach);
    joined(server).await;
}

/// An official snapshot (mixed with one mirrored fork record) parses; a
/// `status` event without an `activity` member applies; an unknown event
/// kind never stalls the stream; `turn`/`labels` updates land.
#[tokio::test]
async fn official_snapshot_and_events_parse_and_apply() {
    let (watcher, accepted) = watcher(vec![]);
    let (events_tx, events_rx) = mpsc::channel::<(String, Value)>(16);
    let mut events_rx = Some(events_rx);
    let fake = spawn_fake(accepted, move |core, framed| {
        let events_rx = if framed {
            None
        } else {
            Some(events_rx.take().expect("one sub conn"))
        };
        Box::pin(async move {
            if framed {
                serve_rpc(core, METHODS, |req| {
                    match req["method"].as_str().unwrap() {
                        // A mixed machine set: official records carry no
                        // `hostname`; a fork machine may still report one.
                        "machine.list" => Some(Ok(json!({"machines": [
                            {"name": "host-b", "target": "alice@host-b",
                             "id": "core-b", "host": "host-b",
                             "version": "0.1.19", "state": "connected"},
                            {"name": "host-c", "target": "alice@host-c",
                             "id": "core-c", "host": "host-c",
                             "version": "0.1.18-followcat.1", "state": "connected",
                             "hostname": "host-c"},
                        ]}))),
                        _ => Some(Ok(Value::Null)),
                    }
                })
                .await;
            } else {
                let mut core = core;
                let snapshot = json!({"seq": 1, "terminals": [
                    official_term("t1"),
                    fork_term("host-b/t9"),
                ]});
                serve_subscribe(&mut core, METHODS, snapshot).await;
                let mut events_rx = events_rx.unwrap();
                while let Some((method, params)) = events_rx.recv().await {
                    core.notify(&method, params).await;
                }
            }
        })
    });

    let view = await_view(&watcher, |v| {
        v.link == Link::Online
            && v.machines.as_ref().is_some_and(|m| m.len() == 2)
            && v.inventory.get("t1").is_some()
            && v.inventory.get("host-b/t9").is_some()
    })
    .await;

    // The official record parses with every fork fact absent.
    let t1 = view.inventory.get("t1").unwrap();
    assert!(t1.activity.is_none() && t1.repository.is_none());
    assert_eq!(t1.turn, 1);
    // The mirrored fork record beside it keeps its facts — per-record,
    // never per-connection.
    let remote = view.inventory.get("host-b/t9").unwrap();
    assert!(remote.activity.is_some() && remote.repository.is_some());
    // Mixed machine rows: hostname optional, core hostname absent.
    let machines = view.machines.as_deref().unwrap();
    assert!(machines[0].hostname.is_none());
    assert_eq!(machines[1].hostname.as_deref(), Some("host-c"));
    assert!(view.core.as_ref().unwrap().hostname.is_none());

    events_tx
        .send((
            "event".into(),
            json!({"seq": 2, "type": "status", "terminal": "t1",
                   "data": {"status": {"state": "working"}, "records": {},
                            "reported": true}}),
        ))
        .await
        .unwrap();
    // An unknown kind without `data` must not stall later events.
    events_tx
        .send((
            "event".into(),
            json!({"seq": 3, "type": "future-kind", "terminal": "t1"}),
        ))
        .await
        .unwrap();
    events_tx
        .send((
            "event".into(),
            json!({"seq": 4, "type": "turn", "terminal": "t1",
                   "data": {"turn": 2}}),
        ))
        .await
        .unwrap();
    events_tx
        .send((
            "event".into(),
            json!({"seq": 5, "type": "labels", "terminal": "t1",
                   "data": {"labels": {"herdr.seen": "2"}}}),
        ))
        .await
        .unwrap();

    let view = await_view(&watcher, |v| {
        v.inventory.get("t1").is_some_and(|t| {
            t.turn == 2 && t.labels.get("herdr.seen").map(String::as_str) == Some("2")
        })
    })
    .await;
    let t1 = view.inventory.get("t1").unwrap();
    assert_eq!(t1.status.as_ref().unwrap().state, "working");
    fake.abort();
}
