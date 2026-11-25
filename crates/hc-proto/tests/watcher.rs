//! The reconnecting watcher's lifecycle: subscribe, snapshot, initial
//! document/machine reads, event-driven updates and their ordering.

mod common;

use common::attach::DEADLINE;
use common::fakecore::DuplexCore;
use common::minimal_terminal;
use common::watcher::{
    await_view, held_fixture, reply, serve_rpc, serve_subscribe, spawn_fake, watcher,
};
use hc_proto::watch::{BoxFuture, DocKey, Link};
use serde_json::{Value, json};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use tokio::sync::mpsc;

/// Method lists for both conns (no `rpc.discover`: no background call
/// to script around).
const METHODS: &[&str] = &[
    "hello",
    "terminal.list",
    "machine.list",
    "doc.get",
    "events.subscribe",
];

/// Scripted `doc.get`/`machine.list` replies for a healthy core.
fn rpc_answer(req: &Value) -> Option<Result<Value, (String, String)>> {
    match req["method"].as_str().unwrap() {
        "machine.list" => Some(Ok(json!({
            "machines": [{
                "name": "host-b", "target": "alice@host-b", "id": "core-b",
                "host": "host-b", "version": "0.9", "state": "connected",
            }],
        }))),
        "doc.get" => Some(Ok(json!({
            "value": {"doc": req["params"]["name"], "on": req["params"]["on"]},
            "version": 3,
        }))),
        _ => Some(Ok(Value::Null)),
    }
}

/// The common healthy fake: a subscribe snapshot then parked, RPCs
/// answered by `rpc_answer`.
fn healthy_fake() -> impl Fn(DuplexCore, bool) -> BoxFuture<()> {
    move |core, framed| {
        Box::pin(async move {
            if framed {
                serve_rpc(core, METHODS, rpc_answer).await;
            } else {
                let mut core = core;
                let snapshot = json!({"seq": 1, "terminals": [minimal_terminal()]});
                serve_subscribe(&mut core, METHODS, snapshot).await;
                std::future::pending::<()>().await; // park until killed
            }
        })
    }
}

#[tokio::test]
async fn snapshot_then_documents_on_local_and_machines() {
    let (watcher, accepted) = watcher(vec!["herdr-app".into(), "hc-tui".into()]);
    let fake = spawn_fake(accepted, healthy_fake());

    let view = await_view(&watcher, |v| {
        v.link == Link::Online
            && v.generation == 1
            && v.documents.len() == 4
            && v.documents
                .values()
                .all(|d| !d.fetching && d.error.is_none())
            && v.machines.as_ref().is_some_and(|m| m.len() == 1)
    })
    .await;

    assert!(view.inventory.get("t7").is_some());
    assert_eq!(view.machines.as_ref().unwrap()[0].name, "host-b");
    for machine in [None, Some("host-b".to_string())] {
        for name in ["herdr-app", "hc-tui"] {
            let doc = &view.documents[&DocKey {
                machine: machine.clone(),
                name: name.into(),
            }];
            assert_eq!(doc.version, 3);
            assert_eq!(doc.value["doc"], name);
        }
    }
    assert_eq!(
        view.documents[&DocKey {
            machine: Some("host-b".into()),
            name: "hc-tui".into()
        }]
            .value["on"],
        "host-b"
    );
    fake.abort();
}

/// A `doc` event refetches only the matching watched document on the
/// named machine; unwatched names and other docs stay untouched.
#[tokio::test]
async fn doc_events_refetch_only_the_matching_document() {
    let (watcher, accepted) = watcher(vec!["hc-tui".into()]);
    let (events_tx, events_rx) = mpsc::channel::<(String, Value)>(16);
    let mut events_rx = Some(events_rx);
    let (doc_gets, mut got_doc) = mpsc::channel::<Value>(16);
    let doc_gets = std::sync::Arc::new(doc_gets);
    let fake = spawn_fake(accepted, move |core, framed| {
        let events_rx = if framed {
            None
        } else {
            Some(events_rx.take().expect("one sub conn"))
        };
        let doc_gets = std::sync::Arc::clone(&doc_gets);
        Box::pin(async move {
            if framed {
                serve_rpc(core, METHODS, move |req| {
                    if req["method"] == "doc.get" {
                        doc_gets.try_send(req["params"].clone()).ok();
                        Some(Ok(json!({"value": {"fresh": 1}, "version": 9})))
                    } else if req["method"] == "machine.list" {
                        Some(Ok(json!({"machines": [{
                            "name": "host-b", "target": "t", "id": "b",
                            "host": "host-b", "version": "0", "state": "connected",
                        }]})))
                    } else {
                        Some(Ok(Value::Null))
                    }
                })
                .await;
            } else {
                let mut core = core;
                serve_subscribe(&mut core, METHODS, json!({"seq": 1, "terminals": []})).await;
                let mut events_rx = events_rx.unwrap();
                while let Some((method, params)) = events_rx.recv().await {
                    core.notify(&method, params).await;
                }
            }
        })
    });

    await_view(&watcher, |v| {
        v.link == Link::Online
            && v.documents.len() == 2
            && v.documents.values().all(|d| !d.fetching)
    })
    .await;
    while got_doc.try_recv().is_ok() {} // settle the initial fetch log

    // A watched doc on a machine: refetches that machine's doc only.
    // (`!d.fetching` above guarantees the initial fetches logged before
    // the drain.)
    events_tx
        .send((
            "event".into(),
            json!({"seq": 2, "type": "doc", "machine": "host-b",
                   "data": {"name": "hc-tui", "version": 9}}),
        ))
        .await
        .unwrap();
    let params = tokio::time::timeout(DEADLINE, got_doc.recv())
        .await
        .expect("the doc event refetched nothing")
        .unwrap();
    assert_eq!(params["name"], "hc-tui");
    assert_eq!(params["on"], "host-b");
    await_view(&watcher, |v| v.documents.values().all(|d| !d.fetching)).await;
    assert!(got_doc.try_recv().is_err(), "another doc refetched");

    // An unwatched doc name triggers no fetch at all.
    events_tx
        .send((
            "event".into(),
            json!({"seq": 3, "type": "doc", "data": {"name": "other", "version": 1}}),
        ))
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert!(got_doc.try_recv().is_err(), "unwatched doc was fetched");
    fake.abort();
}

/// A `missed` notification — even at the same seq — replaces the
/// inventory and refetches every watched document.
#[tokio::test]
async fn missed_replaces_and_refetches_documents() {
    let (watcher, accepted) = watcher(vec!["hc-tui".into()]);
    let (events_tx, events_rx) = mpsc::channel::<(String, Value)>(16);
    let mut events_rx = Some(events_rx);
    let (doc_gets, mut got_doc) = mpsc::channel::<Value>(16);
    let doc_gets = std::sync::Arc::new(doc_gets);
    let fake = spawn_fake(accepted, move |core, framed| {
        let events_rx = if framed {
            None
        } else {
            Some(events_rx.take().expect("one sub conn"))
        };
        let doc_gets = std::sync::Arc::clone(&doc_gets);
        Box::pin(async move {
            if framed {
                serve_rpc(core, METHODS, move |req| {
                    if req["method"] == "doc.get" {
                        doc_gets.try_send(req["params"].clone()).ok();
                        Some(Ok(json!({"value": null, "version": 7})))
                    } else {
                        Some(Ok(json!({"machines": []})))
                    }
                })
                .await;
            } else {
                let mut core = core;
                let mut t = minimal_terminal();
                t["address"] = "t7".into();
                serve_subscribe(&mut core, METHODS, json!({"seq": 5, "terminals": [t]})).await;
                let mut events_rx = events_rx.unwrap();
                while let Some((method, params)) = events_rx.recv().await {
                    core.notify(&method, params).await;
                }
            }
        })
    });

    await_view(&watcher, |v| {
        v.link == Link::Online && v.inventory.seq() == 5
    })
    .await;
    while got_doc.try_recv().is_ok() {}

    let mut gone = minimal_terminal();
    gone["address"] = "t9".into();
    events_tx
        .send(("missed".into(), json!({"seq": 5, "terminals": [gone]})))
        .await
        .unwrap();
    let view = await_view(&watcher, |v| v.inventory.get("t9").is_some()).await;
    assert!(view.inventory.get("t7").is_none(), "snapshot replaced");
    let params = tokio::time::timeout(DEADLINE, got_doc.recv())
        .await
        .expect("missed did not refetch hc-tui")
        .unwrap();
    assert!(params["on"].is_null(), "refetched on the local core");
    assert!(got_doc.try_recv().is_err(), "extra refetches");
    fake.abort();
}

/// Consecutive publishes share unchanged state through `Arc`: an
/// inventory event replaces only the inventory — the whole documents
/// map keeps its pointer; a `DocDone` replaces only the fetched
/// document — the inventory and every untouched document keep theirs.
#[tokio::test]
async fn publishes_share_unchanged_inventory_and_documents() {
    // `doc_a` fetches park only once `hold` is armed — the first
    // epoch's document reads must complete before the lane freezes.
    let hold_armed = Arc::new(AtomicBool::new(false));
    let hold: Arc<dyn Fn(&Value) -> bool + Send + Sync> = {
        let armed = Arc::clone(&hold_armed);
        Arc::new(move |params| armed.load(Ordering::SeqCst) && params["name"] == "doc_a")
    };
    let (watcher, mut held, replies, events_tx, _machines, fake) =
        held_fixture(vec!["doc_a".into(), "doc_b".into()], vec![], hold);
    let key_a = DocKey {
        machine: None,
        name: "doc_a".into(),
    };
    let key_b = DocKey {
        machine: None,
        name: "doc_b".into(),
    };
    let v1 = await_view(&watcher, |v| {
        v.link == Link::Online
            && v.documents.len() == 2
            && v.documents.values().all(|d| !d.fetching)
    })
    .await;

    // An inventory-only event replaces the inventory `Arc` while the
    // whole documents map — outer map included — keeps its pointer.
    events_tx
        .send((
            "event".into(),
            json!({"seq": 2, "type": "title", "terminal": "t7", "data": {"title": "built"}}),
        ))
        .await
        .unwrap();
    let v2 = await_view(&watcher, |v| v.inventory.seq() == 2).await;
    assert!(!Arc::ptr_eq(&v1.inventory, &v2.inventory));
    assert!(Arc::ptr_eq(&v1.documents, &v2.documents));

    // A `doc` event publishes before the refetch it queues: the shared
    // documents map still keeps its pointer.
    hold_armed.store(true, Ordering::SeqCst);
    events_tx
        .send((
            "event".into(),
            json!({"seq": 3, "type": "doc", "data": {"name": "doc_a", "version": 4}}),
        ))
        .await
        .unwrap();
    let v3 = await_view(&watcher, |v| v.inventory.seq() == 3).await;
    assert!(!Arc::ptr_eq(&v2.inventory, &v3.inventory));
    assert!(Arc::ptr_eq(&v2.documents, &v3.documents));
    let req = tokio::time::timeout(DEADLINE, held.recv())
        .await
        .expect("the doc event refetched nothing")
        .unwrap();
    assert_eq!(req["params"]["name"], "doc_a");

    // The `DocDone` publish replaces only `doc_a`'s `Document`: the
    // inventory and `doc_b` keep their exact `Arc`s.
    replies
        .send(reply(&req, json!({"value": {"fresh": 1}, "version": 4})))
        .await
        .unwrap();
    let v4 = await_view(&watcher, |v| {
        v.version > v3.version && v.documents[&key_a].version == 4 && !v.documents[&key_a].fetching
    })
    .await;
    assert!(Arc::ptr_eq(&v3.inventory, &v4.inventory));
    assert!(Arc::ptr_eq(&v3.documents[&key_b], &v4.documents[&key_b]));
    assert!(!Arc::ptr_eq(&v3.documents[&key_a], &v4.documents[&key_a]));
    assert!(!Arc::ptr_eq(&v3.documents, &v4.documents));
    assert_eq!(v4.documents[&key_a].value["fresh"], 1);
    fake.abort();
}

/// Events flow through to the view; heartbeats refresh `machine.list`
/// (bounded freshness without a client timer).
#[tokio::test]
async fn events_update_and_heartbeat_refreshes_machines() {
    let (watcher, accepted) = watcher(vec![]);
    let (events_tx, events_rx) = mpsc::channel::<(String, Value)>(16);
    let mut events_rx = Some(events_rx);
    let (list_calls, mut lists) = mpsc::channel::<()>(16);
    let list_calls = std::sync::Arc::new(list_calls);
    let fake = spawn_fake(accepted, move |core, framed| {
        let events_rx = if framed {
            None
        } else {
            Some(events_rx.take().expect("one sub conn"))
        };
        let list_calls = std::sync::Arc::clone(&list_calls);
        Box::pin(async move {
            if framed {
                serve_rpc(core, METHODS, move |req| {
                    if req["method"] == "machine.list" {
                        list_calls.try_send(()).ok();
                    }
                    Some(Ok(json!({"machines": []})))
                })
                .await;
            } else {
                let mut core = core;
                let mut t = minimal_terminal();
                t["address"] = "t7".into();
                serve_subscribe(&mut core, METHODS, json!({"seq": 1, "terminals": [t]})).await;
                let mut events_rx = events_rx.unwrap();
                while let Some((method, params)) = events_rx.recv().await {
                    core.notify(&method, params).await;
                }
            }
        })
    });

    await_view(&watcher, |v| {
        v.link == Link::Online && v.inventory.seq() == 1
    })
    .await;
    lists.try_recv().unwrap(); // the initial machine.list

    events_tx
        .send((
            "event".into(),
            json!({"seq": 2, "type": "title", "terminal": "t7", "data": {"title": "built"}}),
        ))
        .await
        .unwrap();
    await_view(&watcher, |v| {
        v.inventory.get("t7").is_some_and(|t| t.title == "built")
    })
    .await;
    tokio::time::timeout(DEADLINE, lists.recv())
        .await
        .expect("a created/title event did not queue machine.list");

    events_tx
        .send(("heartbeat".into(), json!({"seq": 2})))
        .await
        .unwrap();
    tokio::time::timeout(DEADLINE, lists.recv())
        .await
        .expect("heartbeat did not refresh machine.list");
    fake.abort();
}

/// A document-only subscription (`docs` without a terminal filter) gets
/// empty terminal snapshots and omits terminal events, so `seq` gaps on
/// `doc` events and higher `heartbeat` sequences must keep the watcher
/// `Online` without triggering a resync reconnect.
#[tokio::test]
async fn document_only_subscription_stays_online_across_empty_snapshots_and_seq_gaps() {
    let mut params = common::watcher::reconnect_params();
    params.doc_patterns = vec!["hc-tui".into()];
    let (watcher, accepted) = common::watcher::watcher_with(vec!["hc-tui".into()], params);
    let (events_tx, events_rx) = mpsc::channel::<(String, Value)>(16);
    let mut events_rx = Some(events_rx);
    let version = Arc::new(std::sync::atomic::AtomicU64::new(1));
    let fake = {
        let version = Arc::clone(&version);
        spawn_fake(accepted, move |core, framed| {
            let events_rx = if framed {
                None
            } else {
                Some(events_rx.take().expect("one sub conn"))
            };
            let version = Arc::clone(&version);
            Box::pin(async move {
                if framed {
                    serve_rpc(core, METHODS, move |req| {
                        match req["method"].as_str().unwrap() {
                            "machine.list" => Some(Ok(json!({"machines": []}))),
                            "doc.get" => {
                                let v = version.load(std::sync::atomic::Ordering::SeqCst);
                                Some(Ok(json!({"value": {"v": v}, "version": v})))
                            }
                            _ => Some(Ok(Value::Null)),
                        }
                    })
                    .await;
                } else {
                    let mut core = core;
                    let req =
                        serve_subscribe(&mut core, METHODS, json!({"seq": 1, "terminals": []}))
                            .await;
                    assert_eq!(req["params"]["docs"], json!(["hc-tui"]));
                    let mut events_rx = events_rx.unwrap();
                    while let Some((method, params)) = events_rx.recv().await {
                        core.notify(&method, params).await;
                    }
                }
            })
        })
    };

    let key = DocKey {
        machine: None,
        name: "hc-tui".into(),
    };
    await_view(&watcher, |v| {
        v.link == Link::Online
            && v.generation == 1
            && v.documents
                .get(&key)
                .is_some_and(|d| d.version == 1 && !d.fetching)
    })
    .await;

    version.store(2, std::sync::atomic::Ordering::SeqCst);
    events_tx
        .send((
            "event".into(),
            json!({"seq": 5, "type": "doc", "data": {"name": "hc-tui", "version": 2}}),
        ))
        .await
        .unwrap();
    let v2 = await_view(&watcher, |v| {
        v.documents
            .get(&key)
            .is_some_and(|d| d.version == 2 && !d.fetching)
    })
    .await;
    assert_eq!(v2.link, Link::Online);
    assert_eq!(v2.generation, 1);
    assert!(!v2.inventory.is_stale());

    events_tx
        .send(("heartbeat".into(), json!({"seq": 8})))
        .await
        .unwrap();
    events_tx
        .send(("missed".into(), json!({"seq": 9, "terminals": []})))
        .await
        .unwrap();
    let v3 = await_view(&watcher, |v| v.inventory.seq() == 9).await;
    assert_eq!(v3.link, Link::Online);
    assert_eq!(v3.generation, 1);
    assert!(!v3.inventory.is_stale());
    fake.abort();
}
