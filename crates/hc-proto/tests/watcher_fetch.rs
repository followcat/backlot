//! Refresh scheduling under the watcher: staleness tags, fairness
//! bounds, per-machine error isolation and capability recovery.

mod common;

use common::attach::DEADLINE;
use common::fakecore::hello_result_framed;
use common::watcher::{
    METHODS, await_view, held_fixture, hold_machines, machine, reconnect_params, reply, serve_held,
    serve_subscribe, spawn_fake, watcher, watcher_with,
};
use hc_proto::watch::{DocKey, Link};
use serde_json::{Value, json};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::{mpsc, oneshot};
use tokio::time::timeout;

const NO_DOC_GET: &[&str] = &["hello", "terminal.list", "machine.list", "events.subscribe"];

fn key(machine: Option<&str>, name: &str) -> DocKey {
    DocKey {
        machine: machine.map(str::to_string),
        name: name.into(),
    }
}

#[tokio::test]
async fn a_same_epoch_newer_request_supersedes_a_held_result() {
    let (watcher, mut held, replies, events_tx, _machines, fake) = held_fixture(
        vec!["hc-tui".into()],
        vec![machine("host-b")],
        hold_machines(),
    );
    await_view(&watcher, |v| v.link == Link::Online).await;

    // The initial fetch of host-b/hc-tui parks as req1.
    let req1 = timeout(DEADLINE, held.recv()).await.unwrap().unwrap();
    assert_eq!(req1["params"]["on"], "host-b");

    // A doc event invalidates the in-flight fetch in the same epoch —
    // the re-request queues behind host-b's per-source slot. Awaiting
    // seq==2 proves THIS event (not an unrelated publish) landed before
    // req1's reply goes out.
    events_tx
        .send((
            "event".into(),
            json!({"seq": 2, "type": "doc", "machine": "host-b",
                   "data": {"name": "hc-tui", "version": 9}}),
        ))
        .await
        .unwrap();
    await_view(&watcher, |v| v.inventory.seq() == 2).await;
    replies
        .send(reply(&req1, json!({"value": {"stale": 1}, "version": 5})))
        .await
        .unwrap();

    // req2 on the wire proves req1's reply was fully processed: the
    // stale value must never have installed, not even transiently.
    let req2 = timeout(DEADLINE, held.recv())
        .await
        .expect("the superseding request never spawned")
        .unwrap();
    assert_ne!(
        watcher.view().documents[&key(Some("host-b"), "hc-tui")].value["stale"],
        1,
        "a superseded result installed"
    );
    replies
        .send(reply(&req2, json!({"value": {"fresh": 1}, "version": 6})))
        .await
        .unwrap();
    await_view(&watcher, |v| {
        v.documents
            .get(&key(Some("host-b"), "hc-tui"))
            .is_some_and(|d| !d.fetching && d.value["fresh"] == 1)
    })
    .await;
    watcher.stop();
    fake.abort();
}

#[tokio::test]
async fn a_result_from_before_machine_readd_is_stale() {
    let (watcher, mut held, replies, events_tx, machines, fake) = held_fixture(
        vec!["hc-tui".into()],
        vec![machine("host-b")],
        hold_machines(),
    );
    await_view(&watcher, |v| v.link == Link::Online).await;
    let req1 = timeout(DEADLINE, held.recv()).await.unwrap().unwrap();

    // Machine drops, then returns — the doc slot is recreated but its
    // generation must not reset to one that req1's reply can match.
    // `missed` publishes, so the next view proves each step landed.
    *machines.lock().unwrap() = vec![];
    events_tx
        .send(("missed".into(), json!({"seq": 1, "terminals": []})))
        .await
        .unwrap();
    await_view(&watcher, |v| {
        !v.documents.contains_key(&key(Some("host-b"), "hc-tui"))
    })
    .await;

    *machines.lock().unwrap() = vec![machine("host-b")];
    events_tx
        .send(("missed".into(), json!({"seq": 1, "terminals": []})))
        .await
        .unwrap();
    await_view(&watcher, |v| {
        v.machines
            .as_ref()
            .is_some_and(|m| m.iter().any(|m| m.name == "host-b"))
    })
    .await;

    // req1's late reply is stale under the re-add's new generation.
    replies
        .send(reply(&req1, json!({"value": {"stale": 1}, "version": 5})))
        .await
        .unwrap();
    // req2 on the wire proves req1's reply was processed: the stale
    // value must never have installed, not even transiently.
    let req2 = timeout(DEADLINE, held.recv())
        .await
        .expect("the re-add request never spawned")
        .unwrap();
    assert_ne!(
        watcher.view().documents[&key(Some("host-b"), "hc-tui")].value["stale"],
        1,
        "a pre-removal result installed after re-add"
    );
    replies
        .send(reply(&req2, json!({"value": {"fresh": 1}, "version": 6})))
        .await
        .unwrap();
    await_view(&watcher, |v| {
        v.documents
            .get(&key(Some("host-b"), "hc-tui"))
            .is_some_and(|d| !d.fetching && d.value["fresh"] == 1)
    })
    .await;
    watcher.stop();
    fake.abort();
}

#[tokio::test]
async fn a_slow_source_with_many_pending_keys_cannot_starve_locals() {
    // One machine holds its fetch while four more of its keys queue
    // behind it — five pending keys on one slow source. Local docs must
    // still complete *before* the slow source is released.
    let (watcher, mut held, replies, _events, _machines, fake) = held_fixture(
        vec!["a".into(), "b".into(), "c".into(), "d".into(), "e".into()],
        vec![machine("slow")],
        hold_machines(),
    );

    // Exactly one slow-source fetch is in flight (per-source cap 1);
    // its other four keys queue without consuming global slots.
    let req1 = timeout(DEADLINE, held.recv()).await.unwrap().unwrap();
    assert_eq!(req1["params"]["on"], "slow");

    let view = await_view(&watcher, |v| {
        ["a", "b", "c", "d", "e"].iter().all(|n| {
            v.documents
                .get(&key(None, n))
                .is_some_and(|d| !d.fetching && !d.value.is_null())
        })
    })
    .await;
    // All five locals landed while the slow source still held its slot.
    assert!(view.documents[&key(Some("slow"), "a")].fetching);
    assert!(
        held.try_recv().is_err(),
        "a second fetch on the held source must not be in flight"
    );

    // Releasing the slow fetch lets its next queued key spawn.
    replies
        .send(reply(&req1, json!({"value": {"done": 1}, "version": 1})))
        .await
        .unwrap();
    let req2 = timeout(DEADLINE, held.recv())
        .await
        .expect("the next queued key never spawned")
        .unwrap();
    assert_eq!(req2["params"]["on"], "slow");
    watcher.stop();
    fake.abort();
}

#[tokio::test]
async fn a_machine_doc_error_is_isolated_to_its_source() {
    let (watcher, accepted) = watcher(vec!["hc-tui".into()]);
    let machines = Arc::new(Mutex::new(vec![machine("m1"), machine("m2")]));
    let fake = {
        let machines = Arc::clone(&machines);
        spawn_fake(accepted, move |core, framed| {
            let machines = Arc::clone(&machines);
            Box::pin(async move {
                if framed {
                    common::watcher::serve_rpc(core, METHODS, move |req| {
                        match req["method"].as_str().unwrap() {
                            "machine.list" => {
                                let m = machines.lock().unwrap().clone();
                                Some(Ok(json!({"machines": m})))
                            }
                            "doc.get" if req["params"]["on"] == "m1" => {
                                Some(Err(("doc.get".into(), "document gone".into())))
                            }
                            "doc.get" => Some(Ok(json!({"value": {"ok": 1}, "version": 2}))),
                            _ => Some(Ok(Value::Null)),
                        }
                    })
                    .await;
                } else {
                    let mut core = core;
                    serve_subscribe(&mut core, METHODS, json!({"seq": 1, "terminals": []})).await;
                    let _park = core.read_message().await;
                }
            })
        })
    };
    let view = await_view(&watcher, |v| {
        v.link == Link::Online
            && v.documents
                .get(&key(Some("m1"), "hc-tui"))
                .is_some_and(|d| d.error.is_some())
            && v.documents
                .get(&key(Some("m2"), "hc-tui"))
                .is_some_and(|d| !d.value.is_null())
    })
    .await;
    assert!(view.documents[&key(None, "hc-tui")].error.is_none());
    assert!(!view.documents[&key(None, "hc-tui")].value.is_null());
    watcher.stop();
    fake.abort();
}

/// A configured `doc_timeout` bounds a parked fetch: the slot frees,
/// the error lands on that document, and event processing is
/// unaffected — a following event still reaches the view and a fresh
/// request can succeed.
#[tokio::test]
async fn a_short_doc_timeout_frees_the_slot_without_blocking_events() {
    let params = hc_proto::watch::WatchParams {
        doc_timeout: Duration::from_millis(80),
        ..reconnect_params()
    };
    let (watcher, accepted) = watcher_with(vec!["hc-tui".into()], params);
    let (held_tx, mut held) = mpsc::channel::<Value>(16);
    let (replies_tx, replies_rx) = mpsc::channel::<(Value, Value)>(16);
    let (events_tx, events_rx) = mpsc::channel::<(String, Value)>(16);
    let mut replies_rx = Some(replies_rx);
    let mut events_rx = Some(events_rx);
    let machines = Arc::new(Mutex::new(vec![]));
    let fake = {
        let machines = Arc::clone(&machines);
        spawn_fake(accepted, move |core, framed| {
            if framed {
                let held_tx = held_tx.clone();
                let machines = Arc::clone(&machines);
                let replies = replies_rx.take().expect("one rpc conn");
                Box::pin(async move {
                    serve_held(core, held_tx, replies, machines, &|_: &Value| true).await;
                })
            } else {
                let mut events_rx = events_rx.take().expect("one sub conn");
                Box::pin(async move {
                    let mut core = core;
                    let mut t = common::minimal_terminal();
                    t["address"] = "t7".into();
                    serve_subscribe(&mut core, METHODS, json!({"seq": 1, "terminals": [t]})).await;
                    while let Some((method, params)) = events_rx.recv().await {
                        core.notify(&method, params).await;
                    }
                })
            }
        })
    };
    await_view(&watcher, |v| v.link == Link::Online).await;
    let req1 = timeout(DEADLINE, held.recv()).await.unwrap().unwrap();

    // req1 never gets a reply: the 80 ms budget lands the error and
    // frees the slot.
    await_view(&watcher, |v| {
        v.documents
            .get(&key(None, "hc-tui"))
            .is_some_and(|d| !d.fetching && d.error.is_some())
    })
    .await;
    let _ = req1;

    // Healthy events keep flowing while the timed-out request sits
    // dead on the wire.
    events_tx
        .send((
            "event".into(),
            json!({"seq": 2, "type": "title", "terminal": "t7", "data": {"title": "alive"}}),
        ))
        .await
        .unwrap();
    await_view(&watcher, |v| {
        v.inventory.get("t7").is_some_and(|t| t.title == "alive")
    })
    .await;

    // A fresh request still works — the slot was really freed.
    events_tx
        .send((
            "event".into(),
            json!({"seq": 3, "type": "doc", "data": {"name": "hc-tui", "version": 4}}),
        ))
        .await
        .unwrap();
    let req2 = timeout(DEADLINE, held.recv())
        .await
        .expect("no refetch after the timeout")
        .unwrap();
    replies_tx
        .send(reply(&req2, json!({"value": {"ok": 1}, "version": 4})))
        .await
        .unwrap();
    await_view(&watcher, |v| {
        v.documents
            .get(&key(None, "hc-tui"))
            .is_some_and(|d| !d.fetching && d.error.is_none() && d.value["ok"] == 1)
    })
    .await;
    watcher.stop();
    fake.abort();
}

#[tokio::test]
async fn a_lost_capability_renegotiates_on_the_next_rpc_conn() {
    // Rpc conn 1 advertises no doc.get → the client latches the feature
    // off and notes it. When conn 1 dies and conn 2 advertises it, the
    // flags must reset and fetches must run.
    let (watcher, accepted) = watcher(vec!["hc-tui".into()]);
    let (kill_tx, kill_rx) = oneshot::channel::<()>();
    let mut kill_rx = Some(kill_rx);
    let conn_no = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let fake = {
        let conn_no = Arc::clone(&conn_no);
        spawn_fake(accepted, move |core, framed| {
            let conn_no = Arc::clone(&conn_no);
            let mut kill_rx = if framed { kill_rx.take() } else { None };
            Box::pin(async move {
                if framed {
                    let n = conn_no.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    let methods = if n == 0 { NO_DOC_GET } else { METHODS };
                    let mut core = core;
                    core.answer_hello(hello_result_framed(methods, &[], true))
                        .await;
                    loop {
                        tokio::select! {
                            req = core.try_read_request() => match req {
                                Some(req) => match req["method"].as_str().unwrap() {
                                    "machine.list" => {
                                        core.reply(&req["id"], json!({"machines": []})).await;
                                    }
                                    "doc.get" => {
                                        core.reply(
                                            &req["id"],
                                            json!({"value": {"back": 1}, "version": 3}),
                                        )
                                        .await;
                                    }
                                    _ => core.reply(&req["id"], Value::Null).await,
                                },
                                None => break,
                            },
                            _ = async {
                                match kill_rx.as_mut() {
                                    Some(rx) => {
                                        let _ = rx.await;
                                    }
                                    None => std::future::pending::<()>().await,
                                }
                            } => break,
                        }
                    }
                } else {
                    let mut core = core;
                    serve_subscribe(&mut core, METHODS, json!({"seq": 1, "terminals": []})).await;
                    let _park = core.read_message().await;
                }
            })
        })
    };

    // Conn 1 has no doc.get: the feature latches off with a note.
    await_view(&watcher, |v| {
        v.link == Link::Online && v.notes.contains_key("doc.get")
    })
    .await;

    // Kill conn 1; conn 2 re-advertises doc.get → fetches recover and
    // the stale note clears.
    kill_tx.send(()).unwrap();
    let view = await_view(&watcher, |v| {
        v.documents
            .get(&key(None, "hc-tui"))
            .is_some_and(|d| !d.fetching && d.value["back"] == 1)
    })
    .await;
    assert!(
        !view.notes.contains_key("doc.get"),
        "stale feature note survived"
    );
    watcher.stop();
    fake.abort();
}
