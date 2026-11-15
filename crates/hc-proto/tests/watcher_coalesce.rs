//! Publish coalescing contracts: a per-endpoint `doc.get` burst lands in
//! one publish per endpoint (never one per document), the opener pass
//! publishes once, and the resolved-traversal callback only runs when a
//! publish actually carries two or more receipt-boundary entries.

mod common;

use common::watcher::{
    METHODS, await_view, machine, reconnect_params, serve_held, serve_rpc, serve_subscribe,
    spawn_fake, watcher_with,
};
use hc_proto::watch::{DocKey, Link, View, WatchParams};
use serde_json::{Value, json};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;

static TRAVERSAL_CALLS: AtomicU64 = AtomicU64::new(0);

/// Counts every resolved traversal. First sights, link flips and fetch
/// completions never carry two boundary entries, so the count must stay
/// zero across the whole startup burst.
fn counting(_view: &View) -> Vec<String> {
    TRAVERSAL_CALLS.fetch_add(1, Ordering::SeqCst);
    Vec::new()
}

fn key(machine: Option<&str>, name: &str) -> DocKey {
    DocKey {
        machine: machine.map(str::to_string),
        name: name.into(),
    }
}

/// A live reporter whose activity domain/epoch can force a boundary.
fn agent_json(address: &str, epoch: &str, created_ms: u64, n: u64) -> Value {
    json!({
        "address": address, "labels": {}, "argv": ["fixture"],
        "cwd": "/home/user/acme-shop", "state": "live", "pid": 100,
        "created_ms": created_ms, "screen": "exact", "cols": 80, "rows": 24,
        "reported": true, "status": {"state": "idle", "app": "fixture"},
        "activity": {"core_id": "host-a", "epoch": epoch, "scope": 1,
            "revision": n,
            "records": {"": {"state_ordinal": n, "completion_ordinal": null}}},
    })
}

/// Collects every published view until `pred` holds (or the deadline
/// inside `await_view` fires). `changed_since` can skip versions, so the
/// journal is observational — the version bound is the hard assertion.
async fn journal_until(
    watcher: &hc_proto::watch::Watcher,
    journal: &mut Vec<View>,
    pred: impl Fn(&View) -> bool,
) -> View {
    let mut seen = 0u64;
    loop {
        let view = await_view(watcher, |v| v.version > seen || pred(v)).await;
        if pred(&view) {
            return view;
        }
        seen = view.version;
        journal.push(view);
    }
}

#[tokio::test]
async fn the_startup_doc_burst_coalesces_per_endpoint_and_needs_no_traversal() {
    TRAVERSAL_CALLS.store(0, Ordering::SeqCst);
    let params = WatchParams {
        traversal: Some(counting),
        ..reconnect_params()
    };
    let (watcher, accepted) = watcher_with(vec!["herdr-app".into(), "hc-tui".into()], params);
    let machines = Arc::new(Mutex::new(vec![machine("host-a"), machine("host-b")]));
    let (events_tx, events_rx) = mpsc::channel::<(String, Value)>(16);
    let mut events_rx = Some(events_rx);
    let fake = {
        let machines = Arc::clone(&machines);
        spawn_fake(accepted, move |core, framed| {
            let machines = Arc::clone(&machines);
            let events_rx = if framed { None } else { events_rx.take() };
            Box::pin(async move {
                if framed {
                    serve_rpc(core, METHODS, move |req| {
                        match req["method"].as_str().unwrap() {
                            "machine.list" => {
                                let m = machines.lock().unwrap().clone();
                                Some(Ok(json!({"machines": m})))
                            }
                            "doc.get" => Some(Ok(json!({"value": null, "version": 1}))),
                            _ => Some(Ok(Value::Null)),
                        }
                    })
                    .await;
                } else {
                    let mut core = core;
                    serve_subscribe(
                        &mut core,
                        METHODS,
                        json!({"seq": 1, "terminals": [
                            agent_json("t1", "boot", 1, 1),
                            agent_json("host-a/t1", "boot", 1, 1),
                            agent_json("host-b/t1", "boot", 1, 1),
                        ]}),
                    )
                    .await;
                    let mut events_rx = events_rx.expect("one sub conn");
                    while let Some((method, params)) = events_rx.recv().await {
                        core.notify(&method, params).await;
                    }
                }
            })
        })
    };

    let mut journal = Vec::new();
    let settled = journal_until(&watcher, &mut journal, |v| {
        v.link == Link::Online
            && v.documents.len() == 6
            && v.documents.values().all(|d| !d.fetching && d.version == 1)
    })
    .await;

    // Opens + sub install + rpc install + machine.list + at most one
    // publish per endpoint's doc pair. The uncoalesced watcher publishes
    // twice for the openers and once per document: 11 versions for M=2.
    assert!(
        settled.version <= 4 + 3,
        "startup published {} times — the doc burst must coalesce to <= one publish per endpoint",
        settled.version
    );
    // Each endpoint's watched pair lands atomically: no view may show
    // exactly one settled document of the pair.
    for view in journal.iter().chain(std::iter::once(&settled)) {
        for machine in [None, Some("host-a"), Some("host-b")] {
            let half = ["herdr-app", "hc-tui"]
                .iter()
                .filter(|name| {
                    view.documents
                        .get(&key(machine, name))
                        .is_some_and(|d| d.version == 1 && !d.fetching)
                })
                .count();
            assert_eq!(
                half % 2,
                0,
                "endpoint {machine:?} settled half its doc pair in view {}",
                view.version
            );
        }
    }
    // Startup, installs, machine.list and every doc_done: zero boundary
    // pairs, so zero resolved traversals.
    assert_eq!(TRAVERSAL_CALLS.load(Ordering::SeqCst), 0);

    // A re-incarnation snapshot carries three boundary entries in one
    // publish — the resolved traversal is needed exactly there.
    events_tx
        .send((
            "missed".into(),
            json!({"seq": 2, "terminals": [
                agent_json("t1", "boot", 2, 4),
                agent_json("host-a/t1", "boot", 2, 6),
                agent_json("host-b/t1", "boot", 2, 8),
            ]}),
        ))
        .await
        .unwrap();
    await_view(&watcher, |v| {
        v.inventory
            .get("t1")
            .is_some_and(|t| t.created_ms == 2 && v.inventory.get("host-b/t1").is_some())
    })
    .await;
    assert_eq!(
        TRAVERSAL_CALLS.load(Ordering::SeqCst),
        1,
        "a two-boundary publish resolves the traversal exactly once"
    );
    watcher.stop();
    fake.abort();
}

#[tokio::test]
async fn in_flight_doc_publishes_when_sub_drops_with_sibling_pending() {
    let params = WatchParams {
        reconnect_initial: std::time::Duration::from_secs(60),
        reconnect_max: std::time::Duration::from_secs(60),
        ..WatchParams::default()
    };
    let (watcher, accepted) = watcher_with(vec!["herdr-app".into(), "hc-tui".into()], params);
    let doc_seen = Arc::new(tokio::sync::Notify::new());
    let (held_tx, mut held) = mpsc::channel(4);
    let (replies_tx, replies_rx) = mpsc::channel(4);
    let (drop_sub_tx, drop_sub_rx) = tokio::sync::oneshot::channel::<()>();
    let machines = Arc::new(Mutex::new(Vec::new()));
    let mut replies_rx = Some(replies_rx);
    let mut drop_sub_rx = Some(drop_sub_rx);
    let fake = {
        let doc_seen = Arc::clone(&doc_seen);
        spawn_fake(accepted, move |core, framed| {
            if framed {
                let held_tx = held_tx.clone();
                let machines = Arc::clone(&machines);
                let doc_seen = Arc::clone(&doc_seen);
                let replies = replies_rx.take().expect("one rpc conn");
                Box::pin(async move {
                    let hold = move |_params: &Value| {
                        doc_seen.notify_one();
                        true
                    };
                    serve_held(core, held_tx, replies, machines, &hold).await;
                })
            } else {
                let drop_sub = drop_sub_rx.take().expect("one sub conn");
                Box::pin(async move {
                    let mut core = core;
                    serve_subscribe(&mut core, METHODS, json!({"seq": 1, "terminals": []})).await;
                    let _ = drop_sub.await;
                })
            }
        })
    };

    tokio::time::timeout(std::time::Duration::from_secs(5), doc_seen.notified())
        .await
        .expect("first doc.get never reached fake core");
    let first_doc = held.recv().await.unwrap();
    await_view(&watcher, |v| matches!(v.link, Link::Online)).await;
    let _ = drop_sub_tx.send(());
    await_view(&watcher, |v| matches!(v.link, Link::Offline { .. })).await;

    replies_tx
        .send((
            first_doc["id"].clone(),
            json!({"value": {"ok": true}, "version": 7}),
        ))
        .await
        .unwrap();
    let settled = await_view(&watcher, |v| {
        v.documents.values().any(|d| d.version == 7 && !d.fetching)
    })
    .await;
    assert!(
        settled
            .documents
            .values()
            .any(|d| d.version == 7 && d.value == json!({"ok": true})),
        "completed doc.get must publish when sub is Offline with sibling pending"
    );
    watcher.stop();
    fake.abort();
}
