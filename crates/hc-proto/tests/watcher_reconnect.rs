//! Reconnect behavior: facility death, epoch handling, static
//! fallback, held openers and `stop`.

mod common;

use common::attach::DEADLINE;
use common::fakecore::{DuplexCore, hello_result_framed};
use common::watcher::{await_view, serve_subscribe, spawn_fake, watcher};
use hc_proto::watch::Link;
use serde_json::{Value, json};
use tokio::sync::{mpsc, oneshot};
use tokio::time::Duration;

const METHODS: &[&str] = &[
    "hello",
    "terminal.list",
    "machine.list",
    "doc.get",
    "events.subscribe",
];

fn terminal(address: &str) -> Value {
    let mut t = common::minimal_terminal();
    t["address"] = address.into();
    t
}

/// An rpc conn answering machine.list/terminal.list/`doc.get` sanely.
async fn serve_rpc_ok(mut core: DuplexCore) {
    core.answer_hello(hello_result_framed(METHODS, &[], true))
        .await;
    while let Some(req) = core.try_read_request().await {
        match req["method"].as_str().unwrap() {
            "machine.list" => core.reply(&req["id"], json!({"machines": []})).await,
            "terminal.list" => {
                core.reply(
                    &req["id"],
                    json!({"terminals": [terminal("t-static")], "hidden_finished": 0}),
                )
                .await;
            }
            "doc.get" => {
                core.reply(&req["id"], json!({"value": {"v": 1}, "version": 1}))
                    .await;
            }
            _ => core.reply(&req["id"], Value::Null).await,
        }
    }
}

#[tokio::test]
async fn sub_death_offlines_inventory_then_a_new_epoch_recovers() {
    let (watcher, accepted) = watcher(vec![]);
    let (kill_tx, kill_rx) = oneshot::channel::<()>();
    let (hold_tx, hold_rx) = oneshot::channel::<()>();
    let mut kill_rx = Some(kill_rx);
    let mut hold_rx = Some(hold_rx);
    let mut plain = 0u32;
    let fake = spawn_fake(accepted, move |core, framed| {
        // Conn 1 dies on command; conn 2's hello waits for the test —
        // a 20 ms backoff cannot reconnect past the Offline assertion.
        let (kill, hold) = if framed {
            (None, None)
        } else {
            plain += 1;
            (
                if plain == 1 { kill_rx.take() } else { None },
                if plain == 2 { hold_rx.take() } else { None },
            )
        };
        Box::pin(async move {
            if framed {
                serve_rpc_ok(core).await;
                return;
            }
            let mut core = core;
            if let Some(rx) = hold {
                let _ = rx.await;
            }
            let snapshot = if kill.is_some() {
                json!({"seq": 1, "terminals": [terminal("t7")]})
            } else {
                json!({"seq": 10, "terminals": [terminal("t9")]})
            };
            serve_subscribe(&mut core, METHODS, snapshot).await;
            match kill {
                Some(rx) => {
                    let _ = rx.await;
                }
                None => std::future::pending::<()>().await,
            }
        })
    });

    let view = await_view(&watcher, |v| {
        v.link == Link::Online && v.inventory.get("t7").is_some()
    })
    .await;
    assert_eq!(view.generation, 1);

    // The subscription conn dies: inventory stays, marked offline, and
    // the link reports the failure — stable while conn 2 is gated.
    kill_tx.send(()).unwrap();
    let view = await_view(&watcher, |v| {
        v.inventory.is_offline() && matches!(v.link, Link::Offline { .. })
    })
    .await;
    assert!(
        view.inventory.get("t7").is_some(),
        "inventory survives offline"
    );

    // Release the replacement: a new epoch installs a fresh snapshot.
    hold_tx.send(()).unwrap();
    let view = await_view(&watcher, |v| {
        v.generation == 2 && v.link == Link::Online && !v.inventory.is_offline()
    })
    .await;
    assert!(view.inventory.get("t9").is_some());
    assert!(
        view.inventory.get("t7").is_none(),
        "new epoch replaced the snapshot"
    );
    watcher.stop();
    fake.abort();
}

#[tokio::test]
async fn rpc_death_keeps_the_subscription_epoch() {
    let (watcher, accepted) = watcher(vec!["hc-tui".into()]);
    let (kill_tx, kill_rx) = oneshot::channel::<()>();
    let (hold_tx, hold_rx) = oneshot::channel::<()>();
    let mut kill_rx = Some(kill_rx);
    let mut hold_rx = Some(hold_rx);
    let mut framed_n = 0u32;
    let fake = spawn_fake(accepted, move |core, framed| {
        let (kill, hold) = if framed {
            framed_n += 1;
            (
                if framed_n == 1 { kill_rx.take() } else { None },
                if framed_n == 2 { hold_rx.take() } else { None },
            )
        } else {
            (None, None)
        };
        Box::pin(async move {
            if framed {
                let mut core = core;
                let mut kill = kill;
                if let Some(rx) = hold {
                    let _ = rx.await; // conn 2's hello is test-gated
                }
                core.answer_hello(hello_result_framed(METHODS, &[], true))
                    .await;
                loop {
                    tokio::select! {
                        req = core.try_read_request() => match req {
                            Some(req) => match req["method"].as_str().unwrap() {
                                "machine.list" => {
                                    core.reply(&req["id"], json!({"machines": []})).await;
                                }
                                "doc.get" => {
                                    core.reply(&req["id"], json!({"value": {"v":1}, "version": 1}))
                                        .await;
                                }
                                _ => core.reply(&req["id"], Value::Null).await,
                            },
                            None => break,
                        },
                        _ = async {
                            match kill.as_mut() {
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
                std::future::pending::<()>().await;
            }
        })
    });

    await_view(&watcher, |v| {
        v.link == Link::Online && v.rpc == Link::Online
    })
    .await;

    // The rpc conn dies: the subscription epoch is untouched, and the
    // Offline mark cannot race past while conn 2's hello is held.
    kill_tx.send(()).unwrap();
    let view = await_view(&watcher, |v| matches!(v.rpc, Link::Offline { .. })).await;
    assert_eq!(view.link, Link::Online, "sub epoch survives an rpc death");
    assert!(
        !view.inventory.is_offline(),
        "sub-driven inventory stays live"
    );

    // Release conn 2: a new rpc installs under the same generation.
    hold_tx.send(()).unwrap();
    let view = await_view(&watcher, |v| v.rpc == Link::Online).await;
    assert_eq!(view.generation, 1);
    watcher.stop();
    fake.abort();
}

/// Both facilities die; the rpc conn recovers first while the
/// replacement subscription's hello is held. Normal-mode `doc.get`s
/// must not start until that subscription succeeds — only
/// `machine.list` (no epoch needed) may flow.
#[tokio::test]
async fn recovered_rpc_reads_no_documents_before_the_new_subscription() {
    let (watcher, accepted) = watcher(vec!["hc-tui".into()]);
    let (kill_sub_tx, kill_sub_rx) = oneshot::channel::<()>();
    let (kill_rpc_tx, kill_rpc_rx) = oneshot::channel::<()>();
    let (hold_tx, hold_rx) = oneshot::channel::<()>();
    let (seen_tx, mut seen) = mpsc::channel::<String>(16);
    let mut kill_sub_rx = Some(kill_sub_rx);
    let mut kill_rpc_rx = Some(kill_rpc_rx);
    let mut hold_rx = Some(hold_rx);
    let mut plain = 0u32;
    let mut framed_n = 0u32;
    let fake = spawn_fake(accepted, move |core, framed| {
        let seen_tx = seen_tx.clone();
        let (kill, hold) = if framed {
            framed_n += 1;
            (
                if framed_n == 1 {
                    kill_rpc_rx.take()
                } else {
                    None
                },
                None,
            )
        } else {
            plain += 1;
            (
                if plain == 1 { kill_sub_rx.take() } else { None },
                if plain == 2 { hold_rx.take() } else { None },
            )
        };
        Box::pin(async move {
            if framed {
                let mut core = core;
                let mut kill = kill;
                core.answer_hello(hello_result_framed(METHODS, &[], true))
                    .await;
                loop {
                    tokio::select! {
                        req = core.try_read_request() => match req {
                            Some(req) => {
                                seen_tx
                                    .try_send(req["method"].as_str().unwrap().to_string())
                                    .ok();
                                match req["method"].as_str().unwrap() {
                                    "machine.list" => {
                                        core.reply(&req["id"], json!({"machines": []})).await;
                                    }
                                    "doc.get" => {
                                        core.reply(&req["id"], json!({"value": {"v":1}, "version":1}))
                                            .await;
                                    }
                                    _ => core.reply(&req["id"], Value::Null).await,
                                }
                            }
                            None => break,
                        },
                        _ = async {
                            match kill.as_mut() {
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
                if let Some(rx) = hold {
                    let _ = rx.await; // conn 2's hello is test-gated
                }
                serve_subscribe(&mut core, METHODS, json!({"seq": 1, "terminals": []})).await;
                match kill {
                    Some(rx) => {
                        let _ = rx.await;
                    }
                    None => std::future::pending::<()>().await,
                }
            }
        })
    });
    await_view(&watcher, |v| {
        v.link == Link::Online
            && v.rpc == Link::Online
            && v.generation == 1
            && v.documents
                .values()
                .any(|d| !d.fetching && d.value["v"] == 1)
    })
    .await;
    while seen.try_recv().is_ok() {} // drain epoch-1 request log

    // Both die. The rpc conn returns quickly; the new subscription is
    // held mid-hello — normal-mode doc.gets must wait for it.
    kill_sub_tx.send(()).unwrap();
    kill_rpc_tx.send(()).unwrap();
    await_view(&watcher, |v| {
        matches!(v.link, Link::Offline { .. }) && v.rpc == Link::Online
    })
    .await;
    let mut methods = Vec::new();
    let _ = tokio::time::timeout(Duration::from_millis(300), async {
        while let Some(method) = seen.recv().await {
            methods.push(method);
        }
    })
    .await;
    assert!(
        !methods.iter().any(|m| m == "doc.get"),
        "a doc.get ran while the replacement subscription was held: {methods:?}"
    );

    // Releasing the subscription unblocks document reads.
    hold_tx.send(()).unwrap();
    await_view(&watcher, |v| v.link == Link::Online && v.generation == 2).await;
    loop {
        let method = tokio::time::timeout(DEADLINE, seen.recv())
            .await
            .unwrap()
            .unwrap();
        if method == "doc.get" {
            break;
        }
    }
    watcher.stop();
    fake.abort();
}

#[tokio::test]
async fn a_malformed_missed_reconnects_into_a_new_epoch() {
    // `missed` without `terminals` is undecodable at the notification
    // boundary — the subscription dies and a fresh epoch replaces it.
    let (watcher, accepted) = watcher(vec![]);
    let (events_tx, events_rx) = mpsc::channel::<(String, Value)>(16);
    let (hold_tx, hold_rx) = oneshot::channel::<()>();
    let mut events_rx = Some(events_rx);
    let mut hold_rx = Some(hold_rx);
    let mut plain = 0u32;
    let fake = spawn_fake(accepted, move |core, framed| {
        // Conn 1 carries the event feed; conn 2's hello is test-gated
        // so the Offline state cannot race past the assertion.
        let (events, hold) = if framed {
            (None, None)
        } else {
            plain += 1;
            (
                if plain == 1 { events_rx.take() } else { None },
                if plain == 2 { hold_rx.take() } else { None },
            )
        };
        Box::pin(async move {
            if framed {
                serve_rpc_ok(core).await;
            } else {
                let mut core = core;
                if let Some(rx) = hold {
                    let _ = rx.await;
                }
                serve_subscribe(&mut core, METHODS, json!({"seq": 1, "terminals": []})).await;
                if let Some(mut rx) = events {
                    while let Some((method, params)) = rx.recv().await {
                        core.notify(&method, params).await;
                    }
                } else {
                    std::future::pending::<()>().await;
                }
            }
        })
    });
    await_view(&watcher, |v| v.link == Link::Online && v.generation == 1).await;

    events_tx
        .send(("missed".into(), json!({"seq": 5}))) // no `terminals`
        .await
        .unwrap();
    await_view(&watcher, |v| matches!(v.link, Link::Offline { .. })).await;
    hold_tx.send(()).unwrap();
    await_view(&watcher, |v| v.generation == 2 && v.link == Link::Online).await;
    watcher.stop();
    fake.abort();
}
